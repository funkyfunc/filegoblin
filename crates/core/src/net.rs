//! Hardened HTTP client construction shared by the single-URL fetcher and the
//! `--horde` crawler. Centralizes timeouts, a User-Agent, response-size caps, and
//! an SSRF guard on redirect targets.

use std::net::IpAddr;
use std::time::Duration;

/// Request timeout for all network reads.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum bytes we will read from any single HTTP response body.
pub const MAX_RESPONSE_BYTES: u64 = 10 * 1024 * 1024; // 10 MiB

/// The User-Agent sent on every request (version tracks the crate).
pub fn user_agent() -> String {
    concat!("filegoblin/", env!("CARGO_PKG_VERSION")).to_string()
}

/// Opt-in escape hatch for the legitimate "convert my local dev server" case.
/// Set `FILEGOBLIN_ALLOW_LOCAL_URLS=1`, or call [`set_allow_local_urls`].
static ALLOW_LOCAL_URLS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Allows fetching loopback/private addresses for the rest of this process.
pub fn set_allow_local_urls(allow: bool) {
    ALLOW_LOCAL_URLS.store(allow, std::sync::atomic::Ordering::Relaxed);
}

pub fn allow_local_urls() -> bool {
    ALLOW_LOCAL_URLS.load(std::sync::atomic::Ordering::Relaxed)
        || std::env::var_os("FILEGOBLIN_ALLOW_LOCAL_URLS").is_some()
}

/// Serializes tests that flip [`set_allow_local_urls`], since it is process-global
/// and the harness runs tests in parallel.
#[cfg(test)]
pub(crate) static LOCAL_URL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Refuses a URL we must not fetch: a non-http(s) scheme, or a host that is —
/// or **resolves to** — loopback, private, link-local (incl. cloud metadata
/// 169.254.169.254), or unspecified.
///
/// Every fetch goes through this, not just redirects. Checking only redirect
/// hops meant `filegoblin http://169.254.169.254/…` was never examined at all,
/// and over MCP the URL comes from a model steered by whatever page it just
/// read, not from the user.
///
/// This is the *early, friendly* check — it fails before a connection is
/// attempted and can explain itself. [`GuardedResolver`] is the authoritative
/// one: it filters at resolution time, so the answer that gets validated is the
/// answer that gets connected to, closing the rebinding window this check alone
/// used to leave open.
pub fn guard_url(url: &url::Url) -> anyhow::Result<()> {
    if !matches!(url.scheme(), "http" | "https") {
        anyhow::bail!(
            "Refusing to fetch a non-http(s) URL: {} (scheme `{}`)",
            url,
            url.scheme()
        );
    }
    if allow_local_urls() {
        return Ok(());
    }
    let Some(host) = url.host_str() else {
        anyhow::bail!("Refusing to fetch a URL with no host: {}", url);
    };
    if is_blocked_host(host) {
        anyhow::bail!("{}", blocked_message(url, host));
    }
    // A public *name* can point at a private address (`localtest.me`,
    // `*.nip.io`, any internal DNS): the textual check alone is bypassable, so
    // resolve and inspect what we would actually connect to.
    let port = url.port_or_known_default().unwrap_or(80);
    if let Some(ip) = resolved_blocked_ip(host, port) {
        anyhow::bail!("{} (it resolves to {})", blocked_message(url, host), ip);
    }
    Ok(())
}

fn blocked_message(url: &url::Url, host: &str) -> String {
    format!(
        "Refusing to fetch {}: `{}` is a private or loopback address. \
         Set FILEGOBLIN_ALLOW_LOCAL_URLS=1 if you meant to reach a local service.",
        url, host
    )
}

/// The first address `host` resolves to that we refuse to connect to.
/// Resolution failures return `None` — the request will fail on its own.
fn resolved_blocked_ip(host: &str, port: u16) -> Option<IpAddr> {
    use std::net::ToSocketAddrs;
    (host, port)
        .to_socket_addrs()
        .ok()?
        .map(|sa| sa.ip())
        .find(|ip| is_blocked_ip(*ip))
}

/// Returns true if `host` literally is an address we must never connect to.
/// This is the textual fast path; [`guard_url`] also resolves names.
pub fn is_blocked_host(host: &str) -> bool {
    // Strip brackets from IPv6 literals like "[::1]".
    let h = host.trim_start_matches('[').trim_end_matches(']');

    if let Ok(ip) = h.parse::<IpAddr>() {
        return is_blocked_ip(ip);
    }

    // Hostname forms that conventionally point at the local machine.
    let lower = host.to_ascii_lowercase();
    lower == "localhost" || lower.ends_with(".localhost")
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                // Carrier-grade NAT 100.64.0.0/10
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                // Unique local fc00::/7
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // Link-local fe80::/10
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// DNS resolver that refuses to hand back a blocked address.
///
/// This is what actually closes the rebinding hole. [`guard_url`] resolves a
/// host to *check* it, and reqwest then resolves independently to *connect* —
/// two lookups, so a record that changes in between (or a round-robin that
/// returns a public address once and a private one next) slips through. Doing
/// the check inside the resolver makes the answer that gets validated the same
/// answer that gets connected to.
#[cfg(feature = "web")]
#[derive(Debug, Default)]
struct GuardedResolver;

#[cfg(feature = "web")]
impl reqwest::dns::Resolve for GuardedResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let allow_local = allow_local_urls();
            // Port 0: reqwest substitutes the real port afterwards.
            let lookup = tokio::task::spawn_blocking(move || {
                use std::net::ToSocketAddrs;
                (host.as_str(), 0u16)
                    .to_socket_addrs()
                    .map(|addrs| (host, addrs.collect::<Vec<_>>()))
            })
            .await??;
            let (host, addrs) = lookup;

            if allow_local {
                return Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs);
            }

            let (allowed, blocked): (Vec<_>, Vec<_>) =
                addrs.into_iter().partition(|sa| !is_blocked_ip(sa.ip()));

            // Refuse the whole name if *any* answer was private, rather than
            // quietly connecting to the public one: a mixed answer is the
            // signature of a rebinding attempt, not an accident.
            if !blocked.is_empty() {
                return Err(format!(
                    "Refusing to connect to `{}`: it resolves to the private or loopback \
                     address {}. Set FILEGOBLIN_ALLOW_LOCAL_URLS=1 if that is intended.",
                    host, blocked[0]
                )
                .into());
            }
            if allowed.is_empty() {
                return Err(format!("`{}` did not resolve to any address", host).into());
            }
            Ok(Box::new(allowed.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// A redirect policy that follows up to `max` hops but refuses any hop whose
/// scheme is not http/https or whose host is blocked by [`is_blocked_host`].
#[cfg(feature = "web")]
fn ssrf_redirect_policy(max: usize) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= max {
            return attempt.stop();
        }
        // Same guard as the initial request, so a 302 to a public name that
        // resolves to 127.0.0.1 cannot walk around it either.
        if guard_url(attempt.url()).is_ok() {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

/// Builds the hardened blocking client used by the single-URL fetch path.
#[cfg(feature = "web")]
pub fn blocking_client() -> reqwest::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(user_agent())
        .timeout(REQUEST_TIMEOUT)
        .redirect(ssrf_redirect_policy(10))
        .dns_resolver(std::sync::Arc::new(GuardedResolver))
        .build()
}

/// Reads an async response body, refusing to buffer more than `cap` bytes.
///
/// `resp.text()` is unbounded, and the crawler's `content_length()` pre-check
/// silently passes on any `Transfer-Encoding: chunked` response (no declared
/// length), so a hostile or merely huge chunked endpoint could exhaust memory.
/// This counts bytes as they arrive instead of trusting a header.
#[cfg(feature = "web")]
pub async fn read_body_capped(resp: reqwest::Response, cap: u64) -> anyhow::Result<Vec<u8>> {
    let mut resp = resp;
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if buf.len() as u64 + chunk.len() as u64 > cap {
            anyhow::bail!("Response body exceeds the {} byte cap", cap);
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// Builds the hardened async client used by the crawler.
#[cfg(feature = "web")]
pub fn async_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(user_agent())
        .timeout(REQUEST_TIMEOUT)
        .redirect(ssrf_redirect_policy(10))
        .dns_resolver(std::sync::Arc::new(GuardedResolver))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_internal_targets() {
        assert!(is_blocked_host("127.0.0.1"));
        assert!(is_blocked_host("169.254.169.254")); // cloud metadata
        assert!(is_blocked_host("10.0.0.5"));
        assert!(is_blocked_host("192.168.1.1"));
        assert!(is_blocked_host("[::1]"));
        assert!(is_blocked_host("localhost"));
        assert!(is_blocked_host("foo.localhost"));
    }

    #[test]
    fn allows_public_targets() {
        assert!(!is_blocked_host("example.com"));
        assert!(!is_blocked_host("140.82.112.3")); // github
        assert!(!is_blocked_host("8.8.8.8"));
    }

    /// Regression: the guard only ran on redirect hops, so the *initial* URL —
    /// the one an MCP client supplies — was never checked at all.
    #[test]
    fn guard_rejects_the_initial_url_not_just_redirects() {
        let _guard = LOCAL_URL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for bad in [
            "http://127.0.0.1:8080/",
            "http://169.254.169.254/latest/meta-data/",
            "http://192.168.1.1/admin",
            "http://localhost:3000/",
            "http://[::1]/",
        ] {
            let url = url::Url::parse(bad).unwrap();
            assert!(
                guard_url(&url).is_err(),
                "{bad} must be refused before any request is made"
            );
        }
    }

    #[test]
    fn guard_rejects_non_http_schemes() {
        for bad in [
            "file:///etc/passwd",
            "gopher://example.com/",
            "ftp://x.com/",
        ] {
            let url = url::Url::parse(bad).unwrap();
            assert!(guard_url(&url).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn guard_allows_ordinary_public_urls() {
        let url = url::Url::parse("https://example.com/page").unwrap();
        assert!(guard_url(&url).is_ok());
    }

    /// The resolver is the authoritative guard, so it must refuse on its own —
    /// without `guard_url` having run first. Uses `localhost`, which the hosts
    /// file resolves to loopback, so no external DNS is involved.
    #[cfg(feature = "web")]
    #[test]
    fn the_resolver_refuses_loopback_even_without_the_pre_check() {
        let _guard = LOCAL_URL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let client = blocking_client().unwrap();
        // Port 9 (discard) is closed everywhere; if the resolver were not
        // filtering we would get a *connection* error instead of ours.
        let err = client
            .get("http://localhost:9/")
            .send()
            .expect_err("a loopback name must not be connectable");
        // reqwest's Display is just "error sending request"; the reason lives
        // in the source chain.
        let mut chain = vec![err.to_string()];
        let mut src: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(&err);
        while let Some(e) = src {
            chain.push(e.to_string());
            src = e.source();
        }
        let msg = chain.join(" | ");
        assert!(
            msg.contains("private or loopback"),
            "expected the resolver's refusal, got: {msg}"
        );
    }

    /// The escape hatch exists for "convert my local dev server", and must
    /// re-enable loopback when set.
    #[test]
    fn opt_in_allows_loopback() {
        let _guard = LOCAL_URL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let url = url::Url::parse("http://127.0.0.1:9/").unwrap();
        assert!(guard_url(&url).is_err());
        set_allow_local_urls(true);
        let allowed = guard_url(&url).is_ok();
        set_allow_local_urls(false);
        assert!(
            allowed,
            "FILEGOBLIN_ALLOW_LOCAL_URLS must re-enable loopback"
        );
    }
}
