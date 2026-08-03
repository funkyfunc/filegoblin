use crate::parsers::gobble::Gobble;
use crate::parsers::html::HtmlGobbler;
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

/// Maximum number of pages a single crawl will fetch.
const MAX_PAGES: usize = 500;

/// Decides whether a discovered `href` should join the crawl frontier, and
/// returns it normalized (fragment stripped).
///
/// Split out of the crawl loop so it can be tested without a network: the
/// crawler had no tests at all, and its scoping rules are exactly the part
/// where a mistake means crawling somewhere it shouldn't.
///
/// Same-origin means host **and port**: comparing `host_str()` alone let a
/// crawl of `example.com` follow links to `example.com:8443` or `:22`.
fn normalize_link(base: &Url, href: &str, seed_host: &str, seed_port: Option<u16>) -> Option<Url> {
    let next = base.join(href).ok()?;
    if !matches!(next.scheme(), "http" | "https") {
        return None;
    }
    if next.host_str() != Some(seed_host) {
        return None;
    }
    if next.port_or_known_default() != seed_port {
        return None;
    }
    let mut clean = next;
    clean.set_fragment(None);
    Some(clean)
}

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

    /// Hardened HTTP client (timeouts + SSRF-guarded redirects + UA).
    client: reqwest::Client,

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

        let client = crate::net::async_client().context("Failed to build HTTP client")?;
        let robots = Self::fetch_robots(&client, seed_url).await;

        Ok(Self {
            visited: Arc::new(DashSet::new()),
            seed_domain,
            rate_limiter,
            robots,
            client,
            extract_full,
        })
    }

    async fn fetch_robots(client: &reqwest::Client, url: &Url) -> Option<Robots> {
        let mut robots_url = url.clone();
        robots_url.set_path("/robots.txt");
        robots_url.set_query(None);

        // A robots.txt is a small text file; cap it so a hostile endpoint
        // cannot stream gigabytes into memory before the crawl even starts.
        const MAX_ROBOTS_BYTES: u64 = 512 * 1024;
        if let Ok(resp) = client.get(robots_url).send().await
            && let Ok(bytes) = crate::net::read_body_capped(resp, MAX_ROBOTS_BYTES).await
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
        let target_domain = Arc::new(self.seed_domain.clone());
        let shared_args = Arc::new(args.clone());

        let mut join_set = tokio::task::JoinSet::new();
        let in_flight = Arc::new(AtomicUsize::new(1));
        // Pages actually fetched. `visited` counts everything *discovered*, so
        // budgeting against it meant one page linking to 600 others exhausted
        // the crawl before a second page was ever read.
        let fetched = Arc::new(AtomicUsize::new(0));
        let seed_port = seed_url.port_or_known_default();

        loop {
            if in_flight.load(Ordering::SeqCst) == 0 {
                break;
            }

            tokio::select! {
                Some(current_url) = rx.recv() => {
                    // Stop fetching once we hit the page budget.
                    if fetched.load(Ordering::SeqCst) >= MAX_PAGES {
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                    fetched.fetch_add(1, Ordering::SeqCst);

                    if !args.quiet {
                        eprintln!("{}", format!("🕸️  Crawling: {}", current_url).truecolor(100, 150, 255));
                    }

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
                    let flags_clone = shared_args.clone();
                    let client = self.client.clone();

                    join_set.spawn(async move {
                        rate_limiter.until_key_ready(&target_domain_clone).await;
                        let permit = active_tasks.acquire_owned().await.unwrap();

                        let display_url = current_url.to_string();
                        // Every hop is guarded, not just the seed: a crawl can
                        // follow a link to any host-shaped target.
                        let guarded = crate::net::guard_url(&current_url);
                        let output_chunk = match guarded.map(|_| client.get(current_url.clone()).send()) {
                            Ok(pending) => match pending.await {
                            Ok(resp) => {
                                let content_type = resp.headers().get(reqwest::header::CONTENT_TYPE)
                                    .and_then(|v| v.to_str().ok())
                                    .unwrap_or("")
                                    .to_string();
                                let too_large = resp.content_length()
                                    .map(|l| l > crate::net::MAX_RESPONSE_BYTES)
                                    .unwrap_or(false);

                                if too_large {
                                    "".to_string()
                                } else if content_type.starts_with("text/html") || content_type.starts_with("text/plain") {
                                    // Capped read: `content_length()` is None
                                    // for chunked responses, so the check above
                                    // is not on its own a bound.
                                    if let Ok(html) = crate::net::read_body_capped(resp, crate::net::MAX_RESPONSE_BYTES)
                                        .await
                                        .map(|b| String::from_utf8_lossy(&b).into_owned()) {
                                        // Run the HTML→markdown engine on this page
                                        let gobbler = HtmlGobbler {
                                            extract_full,
                                            source_url: Some(display_url.clone()),
                                            ..Default::default()
                                        };

                                        // Find links for BFS frontier expansion
                                        let mut new_links = Vec::new();
                                        {
                                            let document = scraper::Html::parse_document(&html);
                                            let selector = scraper::Selector::parse("a").unwrap();
                                            for element in document.select(&selector) {
                                                if let Some(href) = element.value().attr("href")
                                                    && let Some(clean_url) = normalize_link(&current_url, href, &target_domain_clone, seed_port)
                                                    && visited_clone.insert(clean_url.to_string())
                                                {
                                                    new_links.push(clean_url);
                                                }
                                            }
                                        }

                                        // Send links
                                        for link in new_links {
                                            if tx_clone.send(link).is_ok() {
                                                in_flight_clone.fetch_add(1, Ordering::SeqCst);
                                            }
                                        }

                                        let opts = crate::parsers::gobble::ParseOptions::from(flags_clone.as_ref());
                                        if let Ok(markdown) = gobbler.gobble_str(&html, &opts) {
                                            markdown
                                        } else { "".to_string() }
                                    } else { "".to_string() }
                                } else {
                                    "".to_string() // Skip non-text content (images, PDFs, binaries, etc)
                                }
                            },
                            Err(_) => "".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> Url {
        Url::parse("https://example.com/docs/index.html").unwrap()
    }

    #[test]
    fn follows_same_origin_links_and_strips_fragments() {
        let base = seed();
        let follow = |href: &str| normalize_link(&base, href, "example.com", Some(443));

        assert_eq!(
            follow("guide.html").map(|u| u.to_string()),
            Some("https://example.com/docs/guide.html".to_string())
        );
        assert_eq!(
            follow("/about").map(|u| u.to_string()),
            Some("https://example.com/about".to_string())
        );
        // A fragment is not a distinct page; keeping it would crawl the same
        // document once per anchor.
        assert_eq!(
            follow("guide.html#section-3").map(|u| u.to_string()),
            Some("https://example.com/docs/guide.html".to_string())
        );
        // A query string *is* distinct and must survive.
        assert_eq!(
            follow("list?page=2").map(|u| u.to_string()),
            Some("https://example.com/docs/list?page=2".to_string())
        );
    }

    #[test]
    fn refuses_to_leave_the_seed_origin() {
        let base = seed();
        let follow = |href: &str| normalize_link(&base, href, "example.com", Some(443));

        assert!(follow("https://evil.com/x").is_none(), "other host");
        assert!(follow("https://sub.example.com/x").is_none(), "subdomain");
        assert!(follow("mailto:a@example.com").is_none(), "mailto");
        assert!(follow("javascript:alert(1)").is_none(), "javascript");
        assert!(follow("ftp://example.com/f").is_none(), "non-http scheme");
    }

    /// Regression: the host check ignored the port, so a crawl of a web site
    /// would happily follow links to other services on the same machine.
    #[test]
    fn a_different_port_is_a_different_origin() {
        let base = seed();
        let follow = |href: &str| normalize_link(&base, href, "example.com", Some(443));

        assert!(follow("https://example.com:8443/admin").is_none());
        assert!(follow("http://example.com:22/").is_none());
        // The seed's own port is still fine, written explicitly or not.
        assert!(follow("https://example.com:443/ok").is_some());
        assert!(follow("https://example.com/ok").is_some());
    }

    /// A crawl seeded on a non-default port stays on that port.
    #[test]
    fn non_default_seed_port_is_honoured() {
        let base = Url::parse("http://example.com:8080/app/").unwrap();
        let follow = |href: &str| normalize_link(&base, href, "example.com", Some(8080));

        assert!(follow("next").is_some());
        assert!(follow("http://example.com/elsewhere").is_none());
    }

    #[test]
    fn malformed_hrefs_are_skipped_not_fatal() {
        let base = seed();
        assert!(normalize_link(&base, "", "example.com", Some(443)).is_some()); // self-link
        assert!(normalize_link(&base, "http://", "example.com", Some(443)).is_none());
        assert!(normalize_link(&base, "///", "example.com", Some(443)).is_none());
    }
}
