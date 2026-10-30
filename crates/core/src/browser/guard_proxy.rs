//! A loopback HTTP proxy that every connection of a `--browser` render goes
//! through, so the SSRF guard applies to Chrome too.
//!
//! The static fetch is guarded at resolution time (`net::GuardedResolver`),
//! but Chrome resolves and connects on its own. Vetting only the seed URL left
//! everything after it open: a redirect, a `<meta refresh>`, or one line of
//! page JavaScript (`location = "http://169.254.169.254/…"`) navigated the
//! headless browser to a private address, and `--dump-dom` handed that page
//! back as the "rendered" document.
//!
//! Chrome is pointed here with `--proxy-server` and told not to bypass it for
//! loopback. HTTPS (and WebSocket) arrive as `CONNECT host:port`; plain HTTP as
//! an absolute-form request. Either way the host is resolved once, here, every
//! address it resolves to is checked, and the connection goes to the address
//! that was checked, so there is no second lookup to rebind. A refusal answers
//! 403, and the render fails if the page it produced is a refusal.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Text in the body of a refusal, so a refused *navigation* (the page the
/// render returns) can be told apart from a refused image.
pub(super) const REFUSAL_MARKER: &str = "filegoblin-guard-proxy-refused";

/// Decides whether a host may be reached and, if so, which addresses to use.
pub(super) type Policy = Arc<dyn Fn(&str, u16) -> Result<Vec<SocketAddr>, String> + Send + Sync>;

/// Where an allowed connection goes: straight to the vetted address, or to the
/// user's own proxy (`HTTPS_PROXY`), which then reaches the host itself.
#[derive(Clone)]
pub(super) struct Upstream {
    pub addr: String,
    pub no_proxy: Vec<String>,
}

pub(super) struct GuardProxy {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
    refused: Arc<Mutex<Vec<String>>>,
}

impl GuardProxy {
    pub(super) fn start(policy: Policy, upstream: Option<Upstream>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let refused = Arc::new(Mutex::new(Vec::new()));
        let (stop_flag, refusals) = (stop.clone(), refused.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let (policy, upstream, refusals) =
                    (policy.clone(), upstream.clone(), refusals.clone());
                std::thread::spawn(move || {
                    let _ = handle(stream, &policy, upstream.as_ref(), &refusals);
                });
            }
        });
        Ok(Self {
            addr,
            stop,
            refused,
        })
    }

    /// Every `host:port` refused so far.
    pub(super) fn refused(&self) -> Vec<String> {
        self.refused
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Drop for GuardProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop so it sees the flag.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
    }
}

/// The real guard: refuse private, loopback and link-local targets, by name
/// and by every address the name resolves to (like `net::GuardedResolver`).
pub(super) fn ssrf_policy() -> Policy {
    Arc::new(|host: &str, port: u16| {
        if crate::net::allow_local_urls() {
            return (host, port)
                .to_socket_addrs()
                .map(|a| a.collect())
                .map_err(|e| e.to_string());
        }
        if crate::net::is_blocked_host(host) {
            return Err(format!("{host} is a private or loopback address"));
        }
        let addrs: Vec<SocketAddr> = (host.trim_matches(['[', ']']), port)
            .to_socket_addrs()
            .map_err(|e| format!("{host} did not resolve: {e}"))?
            .collect();
        if let Some(bad) = addrs.iter().find(|a| crate::net::is_blocked_ip(a.ip())) {
            return Err(format!(
                "{host} resolves to the private or loopback address {}",
                bad.ip()
            ));
        }
        Ok(addrs)
    })
}

const IO_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_HEAD_BYTES: usize = 64 * 1024;

fn handle(
    client: TcpStream,
    policy: &Policy,
    upstream: Option<&Upstream>,
    refused: &Mutex<Vec<String>>,
) -> std::io::Result<()> {
    client.set_read_timeout(Some(IO_TIMEOUT))?;
    let mut reader = BufReader::new(client.try_clone()?);
    let mut head: Vec<String> = Vec::new();
    let mut size = 0;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        size += n;
        if n == 0 || size > MAX_HEAD_BYTES {
            return Ok(());
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        head.push(line.trim_end_matches(['\r', '\n']).to_string());
    }
    let Some(request_line) = head.first().cloned() else {
        return Ok(());
    };
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return Ok(());
    };

    let connect = method.eq_ignore_ascii_case("CONNECT");
    let (host, port, origin_form) = if connect {
        match split_host_port(target, 443) {
            Some((h, p)) => (h, p, String::new()),
            None => return Ok(()),
        }
    } else {
        match url::Url::parse(target) {
            Ok(u) if u.scheme() == "http" => {
                let host = u.host_str().unwrap_or("").to_string();
                let port = u.port_or_known_default().unwrap_or(80);
                let mut path = u.path().to_string();
                if let Some(q) = u.query() {
                    path.push('?');
                    path.push_str(q);
                }
                (host, port, path)
            }
            // Chrome sends only http:// in absolute form; anything else is not
            // ours to forward.
            _ => return refuse(client, refused, target, "not an http:// request"),
        }
    };

    let addrs = match policy(&host, port) {
        Ok(addrs) => addrs,
        Err(reason) => return refuse(client, refused, &format!("{host}:{port}"), &reason),
    };

    let direct = upstream.filter(|u| !bypasses(&host, &u.no_proxy)).is_none();
    let mut server = if direct {
        connect_any(&addrs)?
    } else {
        let addr = &upstream.expect("checked above").addr;
        TcpStream::connect(addr)?
    };
    server.set_read_timeout(Some(IO_TIMEOUT))?;

    if connect && direct {
        let mut c = client.try_clone()?;
        c.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
    } else if direct {
        // Origin-form for the server, one request per connection so a reused
        // proxy connection cannot carry a second request to another host.
        let mut out = format!("{method} {origin_form} {version}\r\n");
        for line in &head[1..] {
            let name = line.split(':').next().unwrap_or("").trim();
            if ["proxy-connection", "proxy-authorization", "connection"]
                .iter()
                .any(|h| name.eq_ignore_ascii_case(h))
            {
                continue;
            }
            out.push_str(line);
            out.push_str("\r\n");
        }
        out.push_str("Connection: close\r\n\r\n");
        server.write_all(out.as_bytes())?;
    } else {
        // The user's proxy speaks the same protocol: pass the request on.
        let mut out = head.join("\r\n");
        out.push_str("\r\n\r\n");
        server.write_all(out.as_bytes())?;
    }
    // Bytes the client sent after the head (a request body, or a TLS hello).
    let pending = reader.buffer().to_vec();
    if !pending.is_empty() {
        server.write_all(&pending)?;
    }
    splice(client, server)
}

fn refuse(
    mut client: TcpStream,
    refused: &Mutex<Vec<String>>,
    target: &str,
    reason: &str,
) -> std::io::Result<()> {
    refused
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(format!("{target} ({reason})"));
    let body = format!(
        "<html><head><title>Refused</title></head><body><p>{REFUSAL_MARKER}: {}</p></body></html>",
        reason.replace(['<', '>', '&'], "_")
    );
    let resp = format!(
        "HTTP/1.1 403 Forbidden\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    client.write_all(resp.as_bytes())
}

/// `host:port` or `[v6]:port`.
fn split_host_port(target: &str, default_port: u16) -> Option<(String, u16)> {
    let (host, port) = match target.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && !p.contains(']') => (h, p.parse().ok()?),
        _ => (target, default_port),
    };
    Some((host.trim_matches(['[', ']']).to_string(), port))
}

fn connect_any(addrs: &[SocketAddr]) -> std::io::Result<TcpStream> {
    let mut last = std::io::Error::other("no address to connect to");
    for addr in addrs {
        match TcpStream::connect_timeout(addr, Duration::from_secs(15)) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// `NO_PROXY` matching: `*`, an exact host, or a domain suffix (`.corp`, `corp`).
fn bypasses(host: &str, no_proxy: &[String]) -> bool {
    let host = host.to_ascii_lowercase();
    no_proxy.iter().any(|entry| {
        let e = entry.trim().trim_start_matches('.').to_ascii_lowercase();
        !e.is_empty() && (e == "*" || host == e || host.ends_with(&format!(".{e}")))
    })
}

/// Copies both ways until either side closes.
fn splice(client: TcpStream, server: TcpStream) -> std::io::Result<()> {
    let (mut c_read, mut s_write) = (client.try_clone()?, server.try_clone()?);
    let up = std::thread::spawn(move || {
        let _ = std::io::copy(&mut c_read, &mut s_write);
        let _ = s_write.shutdown(std::net::Shutdown::Write);
    });
    let (mut s_read, mut c_write) = (server, client);
    let _ = std::io::copy(&mut s_read, &mut c_write);
    let _ = c_write.shutdown(std::net::Shutdown::Both);
    let _ = up.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn serve_once(body: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut s in listener.incoming().flatten() {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let _ = s.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            }
        });
        port
    }

    fn get_via(proxy: SocketAddr, url: &str) -> String {
        let mut s = TcpStream::connect(proxy).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        write!(
            s,
            "GET {url} HTTP/1.1\r\nHost: x\r\nProxy-Connection: keep-alive\r\n\r\n"
        )
        .unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn the_real_policy_refuses_private_targets() {
        let policy = ssrf_policy();
        let _lock = crate::net::LOCAL_URL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for host in [
            "127.0.0.1",
            "localhost",
            "169.254.169.254",
            "::ffff:127.0.0.1",
            "10.1.2.3",
        ] {
            assert!(policy(host, 80).is_err(), "{host} must be refused");
        }
    }

    #[test]
    fn forwards_allowed_requests_and_refuses_the_rest() {
        let allowed = serve_once("hello from the allowed server");
        let forbidden = serve_once("SECRET");
        let policy: Policy = Arc::new(move |host: &str, port: u16| {
            if port == allowed {
                (host, port)
                    .to_socket_addrs()
                    .map(|a| a.collect())
                    .map_err(|e| e.to_string())
            } else {
                Err("blocked for the test".to_string())
            }
        });
        let proxy = GuardProxy::start(policy, None).unwrap();

        let ok = get_via(proxy.addr, &format!("http://127.0.0.1:{allowed}/x"));
        assert!(ok.contains("hello from the allowed server"), "{ok}");

        let no = get_via(proxy.addr, &format!("http://127.0.0.1:{forbidden}/x"));
        assert!(no.starts_with("HTTP/1.1 403"), "{no}");
        assert!(!no.contains("SECRET"));

        let mut s = TcpStream::connect(proxy.addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        write!(s, "CONNECT 127.0.0.1:{forbidden} HTTP/1.1\r\n\r\n").unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        assert!(out.starts_with("HTTP/1.1 403"), "{out}");
        assert_eq!(proxy.refused().len(), 2, "{:?}", proxy.refused());
    }

    #[test]
    fn host_port_and_no_proxy_parsing() {
        assert_eq!(
            split_host_port("example.com:8443", 443),
            Some(("example.com".into(), 8443))
        );
        assert_eq!(split_host_port("[::1]:443", 1), Some(("::1".into(), 443)));
        assert_eq!(split_host_port("[::1]", 443), Some(("::1".into(), 443)));
        let np = vec![".corp.example".to_string(), "intranet".to_string()];
        assert!(bypasses("wiki.corp.example", &np));
        assert!(bypasses("intranet", &np));
        assert!(!bypasses("example.com", &np));
    }
}
