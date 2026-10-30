use crate::parsers::gobble::Gobble;
use crate::parsers::html::HtmlGobbler;
use crate::pipeline::GobbleDocument;
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

/// What a crawl is allowed to reach.
///
/// `--depth` and `--include`/`--exclude` were applied only by the *local* file
/// walker; the crawler never read them, so `--horde --depth 1` against a URL
/// silently crawled an entire site. There was no way at all to scope a web
/// crawl, which is the whole reason pointing filegoblin at a docs site returned
/// the changelog and the 2019 blog archive along with the four pages you wanted.
///
/// Both prune the frontier rather than filtering the output: a section that is
/// out of scope is never fetched, so scoping saves requests and time instead of
/// just trimming what gets printed. The seed is always fetched — otherwise
/// `--include "/guide/*"` starting at `/` would match nothing and crawl nothing.
pub(crate) struct CrawlScope {
    max_depth: Option<usize>,
    include: Option<globset::GlobSet>,
    exclude: Option<globset::GlobSet>,
}

impl CrawlScope {
    pub(crate) fn from_args(args: &crate::cli::Cli) -> Result<Self> {
        let build = |pats: &[String]| -> Result<Option<globset::GlobSet>> {
            if pats.is_empty() {
                Ok(None)
            } else {
                crate::walk::build_globset(pats).map(Some)
            }
        };
        Ok(Self {
            max_depth: args.depth,
            include: build(&args.include)?,
            exclude: build(&args.exclude)?,
        })
    }

    /// Whether a link discovered at `depth` (the seed is 0) may be fetched.
    ///
    /// Patterns match the URL **path**, so `--include "/guide/*"` and the
    /// shorthand `--include guide` both scope a crawl to that section.
    pub(crate) fn allows(&self, url: &Url, depth: usize) -> bool {
        if let Some(max) = self.max_depth
            && depth > max
        {
            return false;
        }
        let path = url.path();
        if let Some(ex) = &self.exclude
            && ex.is_match(path)
        {
            return false;
        }
        match &self.include {
            Some(inc) => inc.is_match(path),
            None => true,
        }
    }
}

/// Decides whether a discovered `href` should join the crawl frontier, and
/// returns it normalized (fragment stripped).
///
/// Split out of the crawl loop so it can be tested without a network: the
/// crawler had no tests at all, and its scoping rules are exactly the part
/// where a mistake means crawling somewhere it shouldn't.
///
/// Same-origin means host **and port**: comparing `host_str()` alone let a
/// crawl of `example.com` follow links to `example.com:8443` or `:22`.
pub(crate) fn normalize_link(
    base: &Url,
    href: &str,
    seed_host: &str,
    seed_port: Option<u16>,
) -> Option<Url> {
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
    ) -> Result<Vec<GobbleDocument>> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut results = Vec::new();

        let mut tree_out = String::new();
        tree_out.push_str("```tree\n🌐 ");
        tree_out.push_str(seed_url.host_str().unwrap_or("unknown"));
        tree_out.push_str("\n```\n\n");
        results.push(GobbleDocument::extracted(
            "_tree.md".to_string(),
            tree_out,
            "tree",
            None,
        ));

        self.visited.insert(seed_url.to_string());
        // The frontier carries depth, so `--depth` can bound how far from the
        // seed a crawl wanders. The seed itself is depth 0.
        tx.send((seed_url.clone(), 0usize))?;

        let active_tasks = Arc::new(tokio::sync::Semaphore::new(10));
        let target_domain = Arc::new(self.seed_domain.clone());
        let shared_args = Arc::new(args.clone());
        let scope = Arc::new(CrawlScope::from_args(args)?);

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
                Some((current_url, depth)) = rx.recv() => {
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
                    let scope = scope.clone();

                    join_set.spawn(async move {
                        rate_limiter.until_key_ready(&target_domain_clone).await;
                        let permit = active_tasks.acquire_owned().await.unwrap();

                        let display_url = current_url.to_string();
                        let outcome = fetch_page(&client, &current_url).await;

                        // Find links for BFS frontier expansion
                        if let Ok(Some(html)) = &outcome {
                            let mut new_links = Vec::new();
                            {
                                let document = scraper::Html::parse_document(html);
                                let selector = scraper::Selector::parse("a").unwrap();
                                for element in document.select(&selector) {
                                    if let Some(href) = element.value().attr("href")
                                        && let Some(clean_url) = normalize_link(&current_url, href, &target_domain_clone, seed_port)
                                        // Out-of-scope links are dropped here, so a
                                        // pruned section is never fetched at all.
                                        && scope.allows(&clean_url, depth + 1)
                                        && visited_clone.insert(clean_url.to_string())
                                    {
                                        new_links.push(clean_url);
                                    }
                                }
                            }

                            // Count the links in before they are sent: the main loop
                            // can receive one and finish with it (budget spent,
                            // robots.txt) before this task decrements, and counting
                            // after the send let `in_flight` touch 0 with this page's
                            // links still queued, ending the crawl early.
                            in_flight_clone.fetch_add(new_links.len(), Ordering::SeqCst);
                            for link in new_links {
                                if tx_clone.send((link, depth + 1)).is_err() {
                                    in_flight_clone.fetch_sub(1, Ordering::SeqCst);
                                }
                            }
                        }

                        // Run the HTML→markdown engine on this page
                        let document = match outcome {
                            Ok(Some(html)) => {
                                let gobbler = HtmlGobbler {
                                    extract_full,
                                    source_url: Some(display_url.clone()),
                                    ..Default::default()
                                };
                                let opts = crate::parsers::gobble::ParseOptions::from(flags_clone.as_ref());
                                match gobbler.gobble_str(&html, &opts) {
                                    Ok(markdown) => Some(GobbleDocument::extracted(display_url, markdown, "html", None)),
                                    Err(e) => Some(GobbleDocument::failed(display_url, "html", format!("{:#}", e))),
                                }
                            }
                            // Not a page (an image, a PDF, …): the crawler reads HTML only.
                            Ok(None) => None,
                            Err(e) => {
                                if !flags_clone.quiet {
                                    eprintln!("{} {}: {}", "⚠️".yellow(), display_url, e);
                                }
                                Some(GobbleDocument::failed(display_url, "web", e))
                            }
                        };

                        in_flight_clone.fetch_sub(1, Ordering::SeqCst);
                        drop(permit);
                        document
                    });
                }
                Some(res) = join_set.join_next(), if !join_set.is_empty() => {
                    if let Ok(Some(doc)) = res {
                        results.push(doc);
                    }
                }
            }
        }

        while let Some(res) = join_set.join_next().await {
            if let Ok(Some(doc)) = res {
                results.push(doc);
            }
        }
        // Pages finish in whatever order the network allows; sort (the tree
        // map stays first) so the same site gives the same output every run.
        results[1..].sort_by(|a, b| a.path.cmp(&b.path));

        Ok(results)
    }
}

/// Fetches one crawled page. `Ok(None)` is a response that is not a page (the
/// crawler reads HTML and plain text only); every way of not getting the page
/// is an `Err` carrying the reason.
///
/// Errors used to become an empty string that was then dropped, so a crawl
/// whose pages answered 404 or 500 converted the server's error pages as
/// content, and pages that could not be fetched vanished without a trace.
async fn fetch_page(client: &reqwest::Client, url: &Url) -> Result<Option<String>, String> {
    // Every hop is guarded, not just the seed: a crawl can follow a link to
    // any host-shaped target.
    crate::net::guard_url(url).map_err(|e| format!("{:#}", e))?;
    let resp = client
        .get(url.clone())
        .send()
        .await
        .map_err(|e| format!("could not fetch the page: {}", crate::net::error_chain(&e)))?;
    crate::net::check_status(resp.status()).map_err(|e| format!("{:#}", e))?;

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let media_type = content_type
        .as_deref()
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if !matches!(
        media_type.as_str(),
        "text/html" | "application/xhtml+xml" | "text/plain"
    ) {
        return Ok(None);
    }
    if resp
        .content_length()
        .is_some_and(|l| l > crate::net::MAX_RESPONSE_BYTES)
    {
        return Err(format!(
            "the page is larger than the {} MiB cap",
            crate::net::MAX_RESPONSE_BYTES / (1024 * 1024)
        ));
    }
    // Capped read: `content_length()` is None for chunked responses, so the
    // check above is not on its own a bound.
    let bytes = crate::net::read_body_capped(resp, crate::net::MAX_RESPONSE_BYTES)
        .await
        .map_err(|e| format!("{:#}", e))?;
    // The declared (or `<meta>`) charset, as for a single URL: a Latin-1 page
    // was read as UTF-8 and every accented letter became U+FFFD.
    let body = crate::FetchedBody {
        bytes,
        media_type: Some(media_type),
        content_type,
    };
    Ok(Some(body.decode_text()))
}

pub fn crawl_web(url: &Url, args: &crate::cli::Cli) -> Result<Vec<GobbleDocument>> {
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

    /// Serves `pages` (path, status line, content type, body) on loopback,
    /// one thread per connection, until the test process ends.
    fn serve(pages: Vec<(&'static str, &'static str, &'static str, Vec<u8>)>) -> Url {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let pages = std::sync::Arc::new(pages);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let pages = pages.clone();
                std::thread::spawn(move || {
                    let mut s = stream;
                    let mut buf = [0u8; 4096];
                    let n = s.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let (status, ctype, body) = pages
                        .iter()
                        .find(|p| p.0 == path)
                        .map(|p| (p.1, p.2, p.3.clone()))
                        .unwrap_or(("404 Not Found", "text/html", b"<html><body><h1>Error response</h1><p>Error code: 404</p></body></html>".to_vec()));
                    let head = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = s.write_all(head.as_bytes());
                    let _ = s.write_all(&body);
                });
            }
        });
        Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap()
    }

    /// Regression: every failure became an empty string that was dropped, so
    /// a 404 or 500 page was converted as content and a page that could not
    /// be fetched vanished. And bodies were read as UTF-8 whatever the page
    /// declared, so a Latin-1 page came out as U+FFFD soup.
    #[test]
    fn errors_are_reported_and_charsets_honoured() {
        use clap::Parser;
        let index = b"<html><head><meta charset=\"iso-8859-1\"></head><body><main><h1>Index</h1>\
            <p>Caf\xe9 cr\xe8me br\xfbl\xe9e, lorem ipsum dolor sit amet consectetur.</p>\
            <a href=\"/good\">good</a> <a href=\"/missing\">missing</a> <a href=\"/broken\">broken</a>\
            </main></body></html>"
            .to_vec();
        let good = b"<html><body><main><h1>Good</h1><p>Good page content, lorem ipsum dolor sit amet.</p></main></body></html>".to_vec();
        let seed = serve(vec![
            ("/", "200 OK", "text/html", index),
            ("/good", "200 OK", "text/html; charset=utf-8", good),
            (
                "/broken",
                "500 Internal Server Error",
                "text/html",
                b"<html><body>oops</body></html>".to_vec(),
            ),
        ]);

        let _lock = crate::net::LOCAL_URL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::net::set_allow_local_urls(true);
        let args = crate::cli::Cli::parse_from(["filegoblin", "-q", "--horde", "dummy"]);
        let docs = crawl_web(&seed, &args);
        crate::net::set_allow_local_urls(false);
        let docs = docs.unwrap();

        let find = |suffix: &str| {
            docs.iter()
                .find(|d| d.path.ends_with(suffix))
                .unwrap_or_else(|| panic!("no {suffix} in {docs:?}"))
        };
        let root = find(&format!("{}/", seed.port().unwrap()));
        assert!(
            root.content.contains("Café crème brûlée"),
            "{}",
            root.content
        );
        assert!(find("/good").error.is_none());
        let missing = find("/missing");
        assert!(
            missing.error.as_deref().unwrap_or("").contains("404"),
            "{missing:?}"
        );
        assert!(!missing.content.contains("Error response"));
        assert!(
            find("/broken")
                .error
                .as_deref()
                .unwrap_or("")
                .contains("500")
        );
    }

    #[test]
    fn malformed_hrefs_are_skipped_not_fatal() {
        let base = seed();
        assert!(normalize_link(&base, "", "example.com", Some(443)).is_some()); // self-link
        assert!(normalize_link(&base, "http://", "example.com", Some(443)).is_none());
        assert!(normalize_link(&base, "///", "example.com", Some(443)).is_none());
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    use clap::Parser;

    fn scope(extra: &[&str]) -> CrawlScope {
        let mut argv = vec!["filegoblin", "https://example.com", "--horde"];
        argv.extend_from_slice(extra);
        CrawlScope::from_args(&crate::cli::Cli::parse_from(argv)).unwrap()
    }

    fn url(path: &str) -> Url {
        Url::parse(&format!("https://example.com{path}")).unwrap()
    }

    #[test]
    fn no_flags_allows_everything() {
        let s = scope(&[]);
        assert!(s.allows(&url("/"), 0));
        assert!(s.allows(&url("/blog/2019"), 9));
    }

    /// Regression: `--depth` was read by the local walker and ignored by the
    /// crawler, so this silently crawled an entire site.
    #[test]
    fn depth_bounds_how_far_from_the_seed_a_crawl_goes() {
        let s = scope(&["--depth", "1"]);
        assert!(s.allows(&url("/"), 0), "the seed");
        assert!(s.allows(&url("/guide"), 1), "one hop out");
        assert!(!s.allows(&url("/guide/deep"), 2), "two hops is too far");
    }

    /// Regression: `--include`/`--exclude` were likewise ignored for crawls.
    #[test]
    fn include_scopes_a_crawl_to_a_section() {
        let s = scope(&["--include", "/guide/*"]);
        assert!(s.allows(&url("/guide/start.html"), 1));
        assert!(!s.allows(&url("/blog/old.html"), 1));
    }

    /// The bare-word shorthand has to work on URL paths, not just filenames —
    /// `*guide*` must match across `/` separators.
    #[test]
    fn a_bare_word_matches_anywhere_in_the_path() {
        let s = scope(&["--include", "guide"]);
        assert!(s.allows(&url("/docs/guide/start.html"), 1));
        assert!(!s.allows(&url("/docs/blog/old.html"), 1));
    }

    #[test]
    fn exclude_prunes_and_beats_include() {
        let s = scope(&["--exclude", "*changelog*"]);
        assert!(s.allows(&url("/guide/start"), 1));
        assert!(!s.allows(&url("/docs/changelog/v2"), 1));

        // A page matching both is excluded: the narrower intent wins.
        let s = scope(&["--include", "docs", "--exclude", "*changelog*"]);
        assert!(s.allows(&url("/docs/guide"), 1));
        assert!(!s.allows(&url("/docs/changelog"), 1));
    }

    #[test]
    fn filters_and_depth_compose() {
        let s = scope(&["--depth", "2", "--include", "guide"]);
        assert!(s.allows(&url("/guide/a"), 1));
        assert!(s.allows(&url("/guide/a/b"), 2));
        assert!(!s.allows(&url("/guide/a/b/c"), 3), "depth still applies");
        assert!(!s.allows(&url("/blog/a"), 1), "include still applies");
    }

    #[test]
    fn an_invalid_pattern_is_an_error_not_a_silent_empty_scope() {
        let args = crate::cli::Cli::parse_from([
            "filegoblin",
            "https://example.com",
            "--horde",
            "--include",
            "[unclosed",
        ]);
        assert!(CrawlScope::from_args(&args).is_err());
    }
}
