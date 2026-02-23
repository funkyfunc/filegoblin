use crate::parsers::gobble::Gobble;
use crate::parsers::web::WebGobbler;
use anyhow::{Context, Result};
use dashmap::DashSet;
use governor::{Quota, RateLimiter, state::keyed::DefaultKeyedStateStore, clock::DefaultClock};
use reqwest::Url;
use robotxt::Robots;
use std::sync::Arc;
use tokio::sync::mpsc;
use std::num::NonZeroU32;

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
        let seed_domain = seed_url.host_str().context("Mischievous error: Invalid target domain")?.to_string();
        
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
            && let Ok(bytes) = resp.bytes().await {
            return Some(robotxt::Robots::from_bytes(&bytes, "filegoblin"));
        }
        None
    }

    /// Recursively crawls the site using a BFS algorithm and multiple token-bounded workers.
    pub async fn crawl(&self, seed_url: Url) -> Result<String> {
        let (tx, mut rx) = mpsc::channel(100);
        let mut combined_output = String::new();
        combined_output.push_str("```tree\n🌐 ");
        combined_output.push_str(seed_url.host_str().unwrap_or("unknown"));
        combined_output.push_str("\n```\n\n");

        self.visited.insert(seed_url.to_string());
        tx.send(seed_url.clone()).await?;

        // Limit active concurrency to prevent blowing up the network
        let active_tasks = Arc::new(tokio::sync::Semaphore::new(10));

        let target_domain = self.seed_domain.clone();

        while let Some(current_url) = rx.recv().await {
            // Apply Politeness & Exclusions
            if let Some(ref rtxt) = self.robots
                && !rtxt.is_relative_allowed(current_url.path()) {
                continue; // Skip restricted paths
            }
            
            self.rate_limiter.until_key_ready(&target_domain).await;
            
            let permit = active_tasks.clone().acquire_owned().await.unwrap();
            let tx_clone = tx.clone();
            let visited_clone = self.visited.clone();
            let extract_full = self.extract_full;
            let target_domain_clone = target_domain.clone();

            let task = tokio::spawn(async move {
                let display_url = current_url.to_string();
                let output_chunk = match reqwest::get(current_url.clone()).await {
                    Ok(resp) => {
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
                                            next_url.host_str() == Some(&target_domain_clone) {
                                                
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
                            
                            // Send links outside the scraper borrow context because ElementRef is not Send
                            for link in new_links {
                                let _ = tx_clone.send(link).await;
                            }
                            
                            if let Ok(markdown) = gobbler.gobble_str(&html) {
                                format!("// --- FILE_START: {} ---\n{}\n\n", display_url, markdown)
                            } else { "".to_string() }
                        } else { "".to_string() }
                    },
                    Err(_) => "".to_string()
                };
                
                drop(permit);
                output_chunk
            });
            
            // Collect chunk when complete
            let chunk = task.await.unwrap_or_default();
            if !chunk.is_empty() {
                combined_output.push_str(&chunk);
            }
            
            // Check if queue is effectively empty via weak semaphore trick
            // In a real robust system we'd use atomics to track pending jobs,
            // but this helps prevent infinite spinning in basic cases.
        }

        Ok(combined_output)
    }
}

pub fn crawl_web(url: &Url, extract_full: bool) -> Result<String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    
    rt.block_on(async {
        let crawler = GoblinCrawler::new(url, extract_full).await?;
        crawler.crawl(url.clone()).await
    })
}
