//! Finds the address to advertise to peers when `--external-ip` is not given,
//! by asking an IP-echo service, as rskj's `public.ipCheckService` does.
//!
//! The service is reached over HTTPS with certificate and hostname
//! verification against Mozilla's roots, so an on-path attacker cannot choose
//! the address the node advertises. Plain HTTP is rejected.
//!
//! The lookup accepts only IPv4: the discovery and P2P listeners bind IPv4
//! wildcard addresses, so an IPv6 endpoint could be advertised but never
//! reached. The address is also the outbound one; behind NAT it is not
//! inbound-reachable until the ports are forwarded.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{self, ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

pub const DEFAULT_URL: &str = "https://checkip.amazonaws.com";
pub const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RESPONSE_BYTES: usize = 4096;
const MAX_URL_BYTES: usize = 2048;

/// The value of `--ip-check-url` that turns the lookup off.
pub const DISABLED: &str = "off";

struct Target {
    host: String,
    port: u16,
    path: String,
}

fn parse_url(url: &str) -> Result<Target> {
    if url.len() > MAX_URL_BYTES {
        bail!("URL is longer than {MAX_URL_BYTES} bytes");
    }
    if !url.bytes().all(|b| b.is_ascii_graphic()) {
        bail!("URL contains whitespace, control or non-ASCII characters");
    }
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| anyhow!("only https:// URLs are supported"))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().context("invalid port")?),
        None => (authority, 443),
    };
    let host_ok = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.');
    if !host_ok {
        bail!("invalid host");
    }
    Ok(Target {
        host: host.to_string(),
        port,
        path: path.to_string(),
    })
}

/// The host alone: the URL may carry credentials or a token in its path.
pub fn host_of(url: &str) -> String {
    parse_url(url)
        .map(|t| t.host)
        .unwrap_or_else(|_| "<invalid url>".to_string())
}

/// Not in a block of the IANA IPv4 Special-Purpose Address Registry that is
/// marked as not globally reachable.
pub fn is_public(ip: Ipv4Addr) -> bool {
    const SPECIAL_USE: &[([u8; 4], u32)] = &[
        ([0, 0, 0, 0], 8),
        ([10, 0, 0, 0], 8),
        ([100, 64, 0, 0], 10),
        ([127, 0, 0, 0], 8),
        ([169, 254, 0, 0], 16),
        ([172, 16, 0, 0], 12),
        ([192, 0, 0, 0], 24),
        ([192, 0, 2, 0], 24),
        ([192, 88, 99, 0], 24),
        ([192, 168, 0, 0], 16),
        ([198, 18, 0, 0], 15),
        ([198, 51, 100, 0], 24),
        ([203, 0, 113, 0], 24),
        ([224, 0, 0, 0], 4),
        ([240, 0, 0, 0], 4),
    ];
    // PCP and TURN anycast: inside 192.0.0.0/24 but globally reachable.
    const GLOBAL_EXCEPTIONS: [Ipv4Addr; 2] =
        [Ipv4Addr::new(192, 0, 0, 9), Ipv4Addr::new(192, 0, 0, 10)];
    if GLOBAL_EXCEPTIONS.contains(&ip) {
        return true;
    }
    let addr = u32::from(ip);
    !SPECIAL_USE.iter().any(|&(base, prefix)| {
        let mask = u32::MAX << (32 - prefix);
        addr & mask == u32::from(Ipv4Addr::from(base)) & mask
    })
}

/// The IP in an HTTP/1.0 response, or why the response is unusable.
fn parse_response(raw: &[u8]) -> Result<IpAddr> {
    let text = std::str::from_utf8(raw).context("response is not text")?;
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("incomplete response"))?;
    let status = head.lines().next().unwrap_or_default();
    if status.split_whitespace().nth(1) != Some("200") {
        bail!("service answered '{status}'");
    }
    let line = body.lines().next().unwrap_or_default();
    // A forwarding proxy may answer with a comma-separated chain; the last
    // entry is the one nearest to the service.
    let candidate = line.rsplit(',').next().unwrap_or_default().trim();
    let ip: Ipv4Addr = candidate
        .parse()
        .map_err(|_| anyhow!("service did not return an IPv4 address"))?;
    if !is_public(ip) {
        bail!("service returned {ip}, which is not a public address");
    }
    Ok(IpAddr::V4(ip))
}

fn connector(roots: RootCertStore) -> Result<TlsConnector> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .context("no usable TLS protocol version")?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

fn default_connector() -> Result<TlsConnector> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    connector(roots)
}

async fn lookup_with(tls: &TlsConnector, url: &str, timeout: Duration) -> Result<IpAddr> {
    let target = parse_url(url)?;
    tokio::time::timeout(timeout, fetch(tls, &target))
        .await
        .map_err(|_| anyhow!("timed out after {}s", timeout.as_secs()))?
}

async fn fetch(tls: &TlsConnector, target: &Target) -> Result<IpAddr> {
    let tcp = TcpStream::connect((target.host.as_str(), target.port))
        .await
        .context("connect failed")?;
    let name = ServerName::try_from(target.host.clone()).context("invalid server name")?;
    let mut stream = tls
        .connect(name, tcp)
        .await
        .context("TLS handshake failed")?;
    let request = format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nUser-Agent: rustock\r\nAccept: text/plain\r\n\r\n",
        target.path, target.host
    );
    stream
        .write_all(request.as_bytes())
        .await
        .context("request failed")?;

    let mut raw = Vec::new();
    let mut chunk = [0u8; 1024];
    while raw.len() < MAX_RESPONSE_BYTES {
        match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n.min(MAX_RESPONSE_BYTES - raw.len())]),
            // A server that closes without close_notify has still sent its answer.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(anyhow::Error::new(e).context("read failed")),
        }
    }
    parse_response(&raw)
}

/// The address to advertise: the operator's, else the looked-up one. Never
/// fails; `None` means the node advertises loopback.
pub async fn resolve(
    explicit: Option<IpAddr>,
    url: &str,
    network_id: u64,
    closed_network: bool,
) -> Option<IpAddr> {
    resolve_with(default_connector, explicit, url, network_id, closed_network).await
}

async fn resolve_with(
    connector: impl FnOnce() -> Result<TlsConnector>,
    explicit: Option<IpAddr>,
    url: &str,
    network_id: u64,
    closed_network: bool,
) -> Option<IpAddr> {
    if explicit.is_some() {
        return explicit;
    }
    let public_network = network_id == 30 || network_id == 31;
    if url == DISABLED || !public_network || closed_network {
        return None;
    }
    let result = match connector() {
        Ok(tls) => lookup_with(&tls, url, TIMEOUT).await,
        Err(e) => Err(e),
    };
    match result {
        Ok(ip) => {
            tracing::info!("External IP {ip} from {}", host_of(url));
            Some(ip)
        }
        Err(e) => {
            tracing::warn!(
                "Could not determine the external IP from {}: {e:#}; advertising 127.0.0.1, \
                 so peers cannot dial this node. Set --external-ip.",
                host_of(url)
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::net::TcpListener;
    use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use tokio_rustls::rustls::ServerConfig;
    use tokio_rustls::TlsAcceptor;

    // The fixtures in crates/cli/testdata/tls are ECDSA P-256 chains valid for
    // 100 years: `ca` signs `localhost` (SAN localhost) and `other` (SAN
    // other.example); `untrusted` (SAN localhost) is signed by a root the
    // tests never trust.
    fn der(name: &str) -> Vec<u8> {
        let bytes: &[u8] = match name {
            "ca" => include_bytes!("../testdata/tls/ca.der"),
            "localhost" => include_bytes!("../testdata/tls/localhost.der"),
            "localhost.key" => include_bytes!("../testdata/tls/localhost.key.der"),
            "other" => include_bytes!("../testdata/tls/other.der"),
            "other.key" => include_bytes!("../testdata/tls/other.key.der"),
            "untrusted" => include_bytes!("../testdata/tls/untrusted.der"),
            "untrusted.key" => include_bytes!("../testdata/tls/untrusted.key.der"),
            _ => panic!("no fixture {name}"),
        };
        bytes.to_vec()
    }

    fn trusting_test_ca() -> TlsConnector {
        let mut roots = RootCertStore::empty();
        roots.add(CertificateDer::from(der("ca"))).unwrap();
        connector(roots).unwrap()
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Close {
        Notify,
        Abrupt,
        Never,
    }

    struct Server {
        url: String,
        hits: Arc<AtomicUsize>,
    }

    impl Server {
        fn hits(&self) -> usize {
            self.hits.load(Ordering::SeqCst)
        }
    }

    async fn serve_tls(cert: &'static str, response: Vec<u8>, close: Close) -> Server {
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(der(&format!("{cert}.key"))));
        let config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(vec![CertificateDer::from(der(cert))], key)
                .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        // Bound to the address `localhost` resolves to first, which the lookup
        // tries first. On Windows that is ::1, and Windows takes two seconds to
        // refuse a connection, longer than SHORT, before trying 127.0.0.1.
        let listener = TcpListener::bind("localhost:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        tokio::spawn(async move {
            loop {
                let (tcp, _) = listener.accept().await.unwrap();
                counter.fetch_add(1, Ordering::SeqCst);
                let acceptor = acceptor.clone();
                let response = response.clone();
                tokio::spawn(async move {
                    let Ok(mut tls) = acceptor.accept(tcp).await else {
                        return;
                    };
                    let mut buf = [0u8; 512];
                    let _ = tls.read(&mut buf).await;
                    if close == Close::Never {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                    }
                    let _ = tls.write_all(&response).await;
                    let _ = tls.flush().await;
                    if close == Close::Notify {
                        let _ = tls.shutdown().await;
                    }
                });
            }
        });
        Server {
            url: format!("https://localhost:{port}/"),
            hits,
        }
    }

    async fn serve_ok(body: &str) -> Server {
        let response = format!("HTTP/1.1 200 OK\r\n\r\n{body}\n").into_bytes();
        serve_tls("localhost", response, Close::Notify).await
    }

    async fn raw_listener(reply: &'static [u8]) -> Server {
        // Where `localhost` resolves first, as in `serve_tls`.
        let listener = TcpListener::bind("localhost:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        tokio::spawn(async move {
            loop {
                let (mut tcp, _) = listener.accept().await.unwrap();
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let _ = tcp.write_all(reply).await;
                    tokio::time::sleep(Duration::from_secs(30)).await;
                });
            }
        });
        Server {
            url: format!("https://localhost:{port}/"),
            hits,
        }
    }

    const SHORT: Duration = Duration::from_millis(500);

    async fn lookup_err(tls: &TlsConnector, url: &str) -> String {
        format!("{:#}", lookup_with(tls, url, SHORT).await.unwrap_err())
    }

    #[tokio::test]
    async fn returns_the_address_when_the_certificate_is_trusted() {
        let server = serve_ok("8.8.4.4").await;
        let ip = lookup_with(&trusting_test_ca(), &server.url, SHORT)
            .await
            .unwrap();
        assert_eq!(ip, "8.8.4.4".parse::<IpAddr>().unwrap());
    }

    #[tokio::test]
    async fn takes_the_last_entry_of_a_comma_separated_chain() {
        let server = serve_ok("10.0.0.1, 8.8.8.8").await;
        let ip = lookup_with(&trusting_test_ca(), &server.url, SHORT)
            .await
            .unwrap();
        assert_eq!(ip, "8.8.8.8".parse::<IpAddr>().unwrap());
    }

    #[tokio::test]
    async fn still_reads_an_answer_from_a_server_that_closes_without_close_notify() {
        let response = b"HTTP/1.1 200 OK\r\n\r\n8.8.4.4\n".to_vec();
        let server = serve_tls("localhost", response, Close::Abrupt).await;
        let ip = lookup_with(&trusting_test_ca(), &server.url, SHORT)
            .await
            .unwrap();
        assert_eq!(ip, "8.8.4.4".parse::<IpAddr>().unwrap());
    }

    #[tokio::test]
    async fn rejects_a_certificate_for_another_host() {
        let response = b"HTTP/1.1 200 OK\r\n\r\n8.8.4.4\n".to_vec();
        let server = serve_tls("other", response, Close::Notify).await;
        let err = lookup_err(&trusting_test_ca(), &server.url).await;
        assert!(err.contains("TLS handshake failed"), "{err}");
    }

    #[tokio::test]
    async fn rejects_a_certificate_from_an_untrusted_root() {
        let response = b"HTTP/1.1 200 OK\r\n\r\n8.8.4.4\n".to_vec();
        let server = serve_tls("untrusted", response, Close::Notify).await;
        let err = lookup_err(&trusting_test_ca(), &server.url).await;
        assert!(err.contains("TLS handshake failed"), "{err}");
    }

    #[tokio::test]
    async fn the_default_roots_do_not_trust_a_private_ca() {
        let server = serve_ok("8.8.4.4").await;
        let err = lookup_err(&default_connector().unwrap(), &server.url).await;
        assert!(err.contains("TLS handshake failed"), "{err}");
    }

    #[tokio::test]
    async fn plain_http_is_rejected_without_connecting() {
        let server = raw_listener(b"HTTP/1.1 200 OK\r\n\r\n8.8.8.8\n").await;
        let http = server.url.replacen("https://", "http://", 1);
        let err = lookup_err(&trusting_test_ca(), &http).await;
        assert!(err.contains("only https://"), "{err}");
        assert_eq!(server.hits(), 0);
    }

    #[tokio::test]
    async fn a_server_that_answers_in_plaintext_on_an_https_url_fails_the_handshake() {
        let server = raw_listener(b"HTTP/1.1 200 OK\r\n\r\n8.8.8.8\n").await;
        let err = lookup_err(&trusting_test_ca(), &server.url).await;
        assert!(err.contains("TLS handshake failed"), "{err}");
    }

    #[tokio::test]
    async fn rejects_a_non_200_answer() {
        let response = b"HTTP/1.1 503 Service Unavailable\r\n\r\n8.8.8.8\n".to_vec();
        let server = serve_tls("localhost", response, Close::Notify).await;
        let err = lookup_err(&trusting_test_ca(), &server.url).await;
        assert!(err.contains("503"), "{err}");
    }

    #[tokio::test]
    async fn rejects_a_non_public_answer() {
        for body in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.5",
            "100.64.0.1",
            "169.254.1.1",
            "0.0.0.0",
        ] {
            let server = serve_ok(body).await;
            let err = lookup_err(&trusting_test_ca(), &server.url).await;
            assert!(err.contains("not a public address"), "{body}: {err}");
        }
    }

    #[tokio::test]
    async fn rejects_every_ipv6_answer() {
        for body in [
            "2606:4700::1111",
            "::1",
            "::ffff:8.8.8.8",
            "::ffff:10.0.0.1",
            "2001:db8::1",
        ] {
            let server = serve_ok(body).await;
            let err = lookup_err(&trusting_test_ca(), &server.url).await;
            assert!(err.contains("IPv4"), "{body}: {err}");
        }
    }

    #[tokio::test]
    async fn rejects_a_body_that_is_not_an_address() {
        let server = serve_ok("<html>blocked</html>").await;
        assert!(lookup_with(&trusting_test_ca(), &server.url, SHORT)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn rejects_an_empty_body() {
        let response = b"HTTP/1.1 200 OK\r\n\r\n".to_vec();
        let server = serve_tls("localhost", response, Close::Notify).await;
        assert!(lookup_with(&trusting_test_ca(), &server.url, SHORT)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn an_answer_beyond_the_response_limit_is_not_read() {
        let mut response = b"HTTP/1.1 200 OK\r\nX-Pad: ".to_vec();
        response.extend(std::iter::repeat_n(b'a', MAX_RESPONSE_BYTES + 1000));
        response.extend_from_slice(b"\r\n\r\n8.8.8.8\n");
        let server = serve_tls("localhost", response, Close::Notify).await;
        let err = lookup_err(&trusting_test_ca(), &server.url).await;
        assert!(err.contains("incomplete response"), "{err}");
    }

    #[tokio::test]
    async fn the_timeout_covers_a_server_that_never_completes_the_handshake() {
        let server = raw_listener(b"").await;
        let err = lookup_err(&trusting_test_ca(), &server.url).await;
        assert!(err.contains("timed out"), "{err}");
    }

    #[tokio::test]
    async fn the_timeout_covers_a_server_that_never_sends_its_answer() {
        let server = serve_tls("localhost", Vec::new(), Close::Never).await;
        let err = lookup_err(&trusting_test_ca(), &server.url).await;
        assert!(err.contains("timed out"), "{err}");
    }

    #[tokio::test]
    async fn fails_when_nothing_is_listening() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        // Windows takes two seconds to refuse each of `localhost`'s two
        // addresses, so this lookup is given longer than SHORT to fail.
        let url = format!("https://localhost:{port}/");
        let err = format!(
            "{:#}",
            lookup_with(&trusting_test_ca(), &url, Duration::from_secs(10))
                .await
                .unwrap_err()
        );
        assert!(err.contains("connect failed"), "{err}");
    }

    #[test]
    fn only_https_urls_are_accepted() {
        assert!(parse_url("http://checkip.amazonaws.com").is_err());
        assert!(parse_url("checkip.amazonaws.com").is_err());
        assert!(parse_url("https://user:pw@host/").is_err());
        assert!(parse_url("https:///path").is_err());
        assert!(parse_url("https://host:notaport/").is_err());
        assert!(parse_url("https://[::1]/").is_err());
    }

    #[test]
    fn urls_that_could_alter_the_request_are_rejected() {
        for url in [
            "https://host/\r\nX-Injected: 1",
            "https://host/a b",
            "https://host/\n",
            "https://host/\t",
            "https://host/\u{e9}",
            "https://ho st/",
            "https://host\r\n/",
        ] {
            assert!(parse_url(url).is_err(), "{url:?}");
        }
        let long = format!("https://host/{}", "a".repeat(MAX_URL_BYTES));
        assert!(parse_url(&long).is_err());
    }

    #[test]
    fn url_parts_default_to_port_443_and_root_path() {
        let t = parse_url("https://example.org").unwrap();
        assert_eq!(
            (t.host.as_str(), t.port, t.path.as_str()),
            ("example.org", 443, "/")
        );
        let t = parse_url("https://example.org:8443/ip").unwrap();
        assert_eq!(
            (t.host.as_str(), t.port, t.path.as_str()),
            ("example.org", 8443, "/ip")
        );
    }

    #[test]
    fn the_logged_host_never_includes_the_path() {
        assert_eq!(
            host_of("https://example.org/ip?token=secret"),
            "example.org"
        );
        assert_eq!(host_of("http://example.org/"), "<invalid url>");
    }

    #[test]
    fn public_address_classification() {
        for ip in [
            "8.8.8.8",
            "1.1.1.1",
            "100.128.0.1",
            "100.63.255.255",
            "172.32.0.1",
            "198.20.0.1",
            "223.255.255.255",
            "192.0.0.9",
            "192.0.0.10",
        ] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "0.0.0.0",
            "0.255.255.255",
            "10.0.0.1",
            "10.255.255.255",
            "100.64.0.1",
            "100.127.255.255",
            "127.0.0.1",
            "127.255.255.255",
            "169.254.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.0",
            "192.0.0.1",
            "192.0.0.8",
            "192.0.0.11",
            "192.0.0.255",
            "192.0.2.1",
            "192.88.99.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.19.255.255",
            "198.51.100.1",
            "203.0.113.5",
            "224.0.0.1",
            "239.255.255.255",
            "240.0.0.1",
            "255.255.255.255",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn an_explicit_address_is_used_without_any_lookup() {
        let server = serve_ok("8.8.8.8").await;
        let ip: IpAddr = "203.0.113.42".parse().unwrap();
        let got = resolve_with(|| Ok(trusting_test_ca()), Some(ip), &server.url, 30, false).await;
        assert_eq!(got, Some(ip));
        assert_eq!(server.hits(), 0);
    }

    #[tokio::test]
    async fn no_lookup_when_disabled_or_off_the_public_networks() {
        let server = serve_ok("8.8.8.8").await;
        let tls = || Ok(trusting_test_ca());
        assert_eq!(resolve_with(tls, None, DISABLED, 30, false).await, None);
        assert_eq!(resolve_with(tls, None, &server.url, 33, false).await, None);
        assert_eq!(resolve_with(tls, None, &server.url, 30, true).await, None);
        assert_eq!(server.hits(), 0);
    }

    #[tokio::test]
    async fn one_request_is_made_when_a_lookup_is_due() {
        let server = serve_ok("8.8.8.8").await;
        let got = resolve_with(|| Ok(trusting_test_ca()), None, &server.url, 30, false).await;
        assert_eq!(got, Some("8.8.8.8".parse().unwrap()));
        assert_eq!(server.hits(), 1);
    }

    #[tokio::test]
    async fn a_failed_lookup_falls_back_to_none() {
        let server = serve_ok("8.8.8.8").await;
        let http = server.url.replacen("https://", "http://", 1);
        assert_eq!(
            resolve_with(|| Ok(trusting_test_ca()), None, &http, 30, false).await,
            None
        );
        assert_eq!(
            resolve_with(|| Err(anyhow!("no roots")), None, &server.url, 30, false).await,
            None
        );
    }
}
