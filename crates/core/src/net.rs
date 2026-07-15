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

/// Returns true if `host` resolves to (or literally is) an address we must never
/// let a *redirect* send us to: loopback, private, link-local (incl. cloud
/// metadata 169.254.169.254), or unspecified. This is the core SSRF guard.
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

/// A redirect policy that follows up to `max` hops but refuses any hop whose
/// scheme is not http/https or whose host is blocked by [`is_blocked_host`].
#[cfg(feature = "web")]
fn ssrf_redirect_policy(max: usize) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= max {
            return attempt.stop();
        }
        let url = attempt.url();
        let scheme_ok = matches!(url.scheme(), "http" | "https");
        let host_ok = url.host_str().map(|h| !is_blocked_host(h)).unwrap_or(false);
        if scheme_ok && host_ok {
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
        .build()
}

/// Builds the hardened async client used by the crawler.
#[cfg(feature = "web")]
pub fn async_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(user_agent())
        .timeout(REQUEST_TIMEOUT)
        .redirect(ssrf_redirect_policy(10))
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
}
