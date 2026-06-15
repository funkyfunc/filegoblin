use crate::parsers::gobble::Gobble;
use crate::parsers::web::WebGobbler;
use anyhow::{Context, Result};
use colored::Colorize;
use dashmap::DashSet;
use governor::{Quota, RateLimiter, clock::DefaultClock, state::keyed::DefaultKeyedStateStore};
use reqwest::Url;
use robotxt::Robots;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;

/// A unified struct managing web crawler state and orchestration.
pub struct GoblinCrawler {
    /// Tracks URLs we have already visited or queued to avoid infinite loops.
    /// DashSet allows lock-free concurrent ingestion from multiple workers.
    visited: Arc<DashSet<String>>,

    /// Target domain. Scoping is heavily restricted to prevent unbounded internet traversal.
    seed_domain: String,

    /// Politeness rate limiter (requests per second per domain).
    rate_limiter: Arc<RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>>,

    /// Parsed Robots.txt for the domain
    robots: Option<Robots>,

    pub extract_full: bool,
}

impl GoblinCrawler {
    pub async fn new(seed_url: &Url, extract_full: bool) -> Result<Self> {
        let seed_domain = seed_url
            .host_str()
            .context("Mischievous error: Invalid target domain")?
            .to_string();

        let quota = Quota::per_second(NonZeroU32::new(5).unwrap());
        let rate_limiter = Arc::new(RateLimiter::keyed(quota));

        let robots = Self::fetch_robots(seed_url).await;

        Ok(Self {
            visited: Arc::new(DashSet::new()),
            seed_domain,
            rate_limiter,
            robots,
            extract_full,
        })
    }

    async fn fetch_robots(url: &Url) -> Option<Robots> {
        let mut robots_url = url.clone();
        robots_url.set_path("/robots.txt");
        robots_url.set_query(None);

        if let Ok(resp) = reqwest::get(robots_url).await
            && let Ok(bytes) = resp.bytes().await
        {
            return Some(robotxt::Robots::from_bytes(&bytes, "filegoblin"));
        }
        None
    }

    /// Recursively crawls the site using a BFS algorithm and multiple token-bounded workers.
    pub async fn crawl(
        &self,
        seed_url: Url,
        args: &crate::cli::Cli,
    ) -> Result<Vec<(String, String)>> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut results = Vec::new();

        let mut tree_out = String::new();
        tree_out.push_str("```tree\n🌐 ");
        tree_out.push_str(seed_url.host_str().unwrap_or("unknown"));
        tree_out.push_str("\n```\n\n");
        results.push(("_tree.md".to_string(), tree_out));

        self.visited.insert(seed_url.to_string());
        tx.send(seed_url.clone())?;

        let active_tasks = Arc::new(tokio::sync::Semaphore::new(10));
        let target_domain = self.seed_domain.clone();

        let mut join_set = tokio::task::JoinSet::new();
        let in_flight = Arc::new(AtomicUsize::new(1));

        loop {
            if in_flight.load(Ordering::SeqCst) == 0 {
                break;
            }

            tokio::select! {
                Some(current_url) = rx.recv() => {
                    println!("{}", format!("🕸️  Crawling: {}", current_url).truecolor(100, 150, 255));

                    if let Some(ref rtxt) = self.robots
                        && !rtxt.is_relative_allowed(current_url.path()) {
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                        continue; // Skip restricted paths
                    }

                    let active_tasks = active_tasks.clone();
                    let tx_clone = tx.clone();
                    let visited_clone = self.visited.clone();
                    let extract_full = self.extract_full;
                    let target_domain_clone = target_domain.clone();
                    let rate_limiter = self.rate_limiter.clone();
                    let in_flight_clone = in_flight.clone();
                    let flags_clone = args.clone();

                    join_set.spawn(async move {
                        rate_limiter.until_key_ready(&target_domain_clone).await;
                        let permit = active_tasks.acquire_owned().await.unwrap();

                        let display_url = current_url.to_string();
                        let output_chunk = match reqwest::get(current_url.clone()).await {
                            Ok(resp) => {
                                let content_type = resp.headers().get(reqwest::header::CONTENT_TYPE)
                                    .and_then(|v| v.to_str().ok())
                                    .unwrap_or("");

                                if content_type.starts_with("text/html") || content_type.starts_with("text/plain") {
                                    if let Ok(html) = resp.text().await {
                                        // Run the standard WebGobbler engine on this page
                                        let gobbler = WebGobbler { extract_full };

                                        // Find links for BFS frontier expansion
                                        let mut new_links = Vec::new();
                                        {
                                            let document = scraper::Html::parse_document(&html);
                                            let selector = scraper::Selector::parse("a").unwrap();
                                            for element in document.select(&selector) {
                                                if let Some(href) = element.value().attr("href")
                                                    && let Ok(next_url) = current_url.join(href) {
                                                    // Normalization constraints: Only HTTP/HTTPS, matches base domain, no fragments.
                                                    let scheme = next_url.scheme();
                                                    if (scheme == "http" || scheme == "https") &&
                                                        next_url.host_str() == Some(target_domain_clone.as_str()) {

                                                            let mut clean_url = next_url.clone();
                                                            clean_url.set_fragment(None);

                                                            let clean_str = clean_url.to_string();

                                                            if visited_clone.insert(clean_str.clone()) {
                                                                new_links.push(clean_url);
                                                            }
                                                        }
                                                    }
                                                }
                                        }

                                        // Send links
                                        for link in new_links {
                                            if tx_clone.send(link).is_ok() {
                                                in_flight_clone.fetch_add(1, Ordering::SeqCst);
                                            }
                                        }

                                        if let Ok(markdown) = gobbler.gobble_str(&html, &flags_clone) {
                                            markdown
                                        } else { "".to_string() }
                                    } else { "".to_string() }
                                } else {
                                    "".to_string() // Skip non-text content (images, PDFs, binaries, etc)
                                }
                            },
                            Err(_) => "".to_string()
                        };

                        in_flight_clone.fetch_sub(1, Ordering::SeqCst);
                        drop(permit);
                        (display_url, output_chunk)
                    });
                }
                Some(res) = join_set.join_next(), if !join_set.is_empty() => {
                    if let Ok(chunk_pair) = res
                        && !chunk_pair.1.is_empty() {
                        results.push(chunk_pair);
                    }
                }
            }
        }

        while let Some(res) = join_set.join_next().await {
            if let Ok(chunk_pair) = res
                && !chunk_pair.1.is_empty()
            {
                results.push(chunk_pair);
            }
        }

        Ok(results)
    }
}

pub fn crawl_web(url: &Url, args: &crate::cli::Cli) -> Result<Vec<(String, String)>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let crawler = GoblinCrawler::new(url, args.full).await?;
        crawler.crawl(url.clone(), args).await
    })
}
