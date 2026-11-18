//! The node's one HTTP client: a `GET` at a time over HTTP/1.1, for `--import`
//! to fetch Rootstock's signed state copies, and for the miner's stable
//! minimum gas price to fetch the price of a bitcoin.
//!
//! The server's certificate is checked against Mozilla's roots (`webpki-roots`)
//! through rustls on `ring`. Rootstock serves its copies from S3 through
//! CloudFront, as plain `200` answers carrying a `Content-Length`; this also
//! reads chunked bodies and bodies that run to the close, and follows
//! redirects. How it fetches is a [`Fetch`]: `--import`'s, `https://` only,
//! and the price's, as Java's `HttpURLConnection` fetches for rskj. Nothing
//! `--import` fetches is trusted for arriving this way: it holds every byte to
//! what the signers signed.

use anyhow::{anyhow, bail, ensure, Context, Result};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::net::TcpStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{self, ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

/// How one kind of fetch is made.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fetch {
    /// How long connecting, and then the TLS handshake, may each take; `None`
    /// for as long as they take.
    pub connect_timeout: Option<Duration>,
    /// How long the server may go quiet before the transfer is given up;
    /// `None` for as long as it likes.
    pub read_timeout: Option<Duration>,
    pub max_redirects: usize,
    /// Whether `http://` is fetched as well as `https://`.
    pub plain_http: bool,
    /// Whether every `2xx` answer is a body, or only `200`.
    pub any_success: bool,
    /// The redirect codes followed; any other `3xx` is an error.
    pub redirects: &'static [u16],
    /// Where a redirect's `Location` points, from the URL that answered.
    pub resolve: fn(&Url, &str) -> Result<String>,
}

/// `--import`'s: Rootstock's copies, over `https://` only.
pub(crate) const IMPORT: Fetch = Fetch {
    connect_timeout: Some(Duration::from_secs(30)),
    read_timeout: Some(Duration::from_secs(60)),
    max_redirects: 5,
    plain_http: false,
    any_success: false,
    redirects: &[301, 302, 303, 307, 308],
    resolve,
};

impl Fetch {
    /// As Java's `HttpURLConnection` fetches for rskj's `SimpleHttpClient`:
    /// `timeout` to connect and to each read, with no limit where it is zero;
    /// `http://` as well as `https://`; every `2xx` an answer; and the
    /// redirects Java follows, as it resolves them (`resolve_java`), for 300
    /// to 303 and 307. Java gives up at the 20th request (`http.maxRedirects`),
    /// so after 19 redirects. A 305, which Java takes as a proxy to go
    /// through, isn't followed: a server would choose where the node's
    /// request goes.
    pub(crate) fn java(timeout: Duration) -> Self {
        let limit = (!timeout.is_zero()).then_some(timeout);
        Self {
            connect_timeout: limit,
            read_timeout: limit,
            max_redirects: 19,
            plain_http: true,
            any_success: true,
            redirects: &[300, 301, 302, 303, 307],
            resolve: resolve_java,
        }
    }
}

/// The most the status line and the headers may take, together.
const MAX_HEAD: u64 = 64 * 1024;
/// The most a chunk-size line, or the trailers after the last chunk, may take.
const MAX_CHUNK_LINE: u64 = 4 * 1024;

/// Where a body goes as it arrives.
pub(crate) trait Sink: Send {
    fn write(&mut self, data: &[u8]) -> Result<()>;
}

/// Whether a URL is fetched in the clear or over TLS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scheme {
    Http,
    Https,
}

impl Scheme {
    fn prefix(self) -> &'static str {
        match self {
            Scheme::Http => "http://",
            Scheme::Https => "https://",
        }
    }

    fn default_port(self) -> u16 {
        match self {
            Scheme::Http => 80,
            Scheme::Https => 443,
        }
    }
}

/// A URL, split as a request needs it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Url {
    pub scheme: Scheme,
    pub host: String,
    pub port: u16,
    /// The path and query, from the first `/`; `/` for a URL without one.
    pub path: String,
}

/// An `https://` URL, as `--import` fetches.
pub(crate) fn parse_url(url: &str) -> Result<Url> {
    ensure!(
        url.starts_with("https://"),
        "{url}: only https:// URLs are fetched"
    );
    parse_any_url(url)
}

/// An `http://` or `https://` URL.
pub(crate) fn parse_any_url(url: &str) -> Result<Url> {
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (Scheme::Https, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (Scheme::Http, rest)
    } else {
        bail!("{url}: only http:// and https:// URLs are fetched")
    };
    let rest = rest.split('#').next().unwrap_or_default();
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) if rest.as_bytes()[i] == b'/' => (&rest[..i], rest[i..].to_string()),
        Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
        None => (rest, "/".to_string()),
    };
    // Anything else in a request line would let the URL write headers of its own.
    ensure!(
        path.bytes().all(|b| b.is_ascii_graphic()),
        "{url}: the path has to be printable ASCII, without spaces"
    );
    ensure!(
        !authority.contains('@'),
        "{url}: a URL carrying credentials is not fetched"
    );
    ensure!(
        !authority.starts_with('['),
        "{url}: an IPv6 literal is not supported"
    );
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (
            host,
            port.parse::<u16>()
                .with_context(|| format!("{url}: the port {port:?}"))?,
        ),
        None => (authority, scheme.default_port()),
    };
    ensure!(
        !host.is_empty()
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.'),
        "{url}: the host has to be a name or an IPv4 address"
    );
    Ok(Url {
        scheme,
        host: host.to_ascii_lowercase(),
        port,
        path,
    })
}

/// What one request came to.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// The body went to the sink: this many bytes of it.
    Body(u64),
    /// The server pointed elsewhere.
    Redirect(String),
}

/// Fetch `url` into `sink` as `--import` does, following redirects. Returns
/// the body's length.
pub(crate) async fn get(url: &str, sink: &mut dyn Sink) -> Result<u64> {
    get_with(url, &IMPORT, sink).await
}

/// Fetch `url` into `sink` as `fetch` says, following redirects. Returns the
/// body's length.
pub(crate) async fn get_with(url: &str, fetch: &Fetch, sink: &mut dyn Sink) -> Result<u64> {
    let config = tls_config()?;
    let mut url = url.to_string();
    for _ in 0..=fetch.max_redirects {
        let target = if fetch.plain_http {
            parse_any_url(&url)?
        } else {
            parse_url(&url)?
        };
        let outcome = match target.scheme {
            Scheme::Https => {
                let stream = connect_tls(&target, &config, fetch).await?;
                exchange(stream, &target, fetch, sink).await
            }
            Scheme::Http => {
                let stream = connect_tcp(&target, fetch).await?;
                exchange(stream, &target, fetch, sink).await
            }
        };
        match outcome.with_context(|| format!("fetching {url}"))? {
            Outcome::Body(n) => return Ok(n),
            Outcome::Redirect(location) => url = (fetch.resolve)(&target, &location)?,
        }
    }
    bail!("{url}: more than {} redirects", fetch.max_redirects)
}

fn tls_config() -> Result<Arc<ClientConfig>> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .context("TLS: the default protocol versions")?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

async fn connect_tcp(url: &Url, fetch: &Fetch) -> Result<TcpStream> {
    let (host, port) = (url.host.as_str(), url.port);
    within(fetch.connect_timeout, TcpStream::connect((host, port)))
        .await
        .map_err(|_| anyhow!("connecting to {host}:{port} timed out"))?
        .with_context(|| format!("connecting to {host}:{port}"))
}

async fn connect_tls(
    url: &Url,
    config: &Arc<ClientConfig>,
    fetch: &Fetch,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let tcp = connect_tcp(url, fetch).await?;
    let host = url.host.as_str();
    let name = ServerName::try_from(url.host.clone())
        .map_err(|_| anyhow!("{host} is not a name a certificate can be checked against"))?;
    within(
        fetch.connect_timeout,
        TlsConnector::from(config.clone()).connect(name, tcp),
    )
    .await
    .map_err(|_| anyhow!("the TLS handshake with {host} timed out"))?
    .with_context(|| format!("TLS with {host}"))
}

/// `f`, given up after `limit`, or never where there is none.
async fn within<T>(
    limit: Option<Duration>,
    f: impl Future<Output = T>,
) -> std::result::Result<T, tokio::time::error::Elapsed> {
    match limit {
        Some(limit) => tokio::time::timeout(limit, f).await,
        None => Ok(f.await),
    }
}

async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    url: &Url,
    fetch: &Fetch,
    sink: &mut dyn Sink,
) -> Result<Outcome> {
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: rustock/{}\r\nAccept: */*\r\n\
         Accept-Encoding: identity\r\nConnection: close\r\n\r\n",
        url.path,
        authority(url),
        env!("CARGO_PKG_VERSION")
    );
    timed(fetch.read_timeout, stream.write_all(request.as_bytes())).await?;
    timed(fetch.read_timeout, stream.flush()).await?;
    let mut reader = BufReader::with_capacity(1 << 18, stream);
    read_response(&mut reader, fetch, sink).await
}

/// The host, and the port where it isn't the scheme's, as a `Host` header
/// names them.
fn authority(url: &Url) -> String {
    if url.port == url.scheme.default_port() {
        url.host.clone()
    } else {
        format!("{}:{}", url.host, url.port)
    }
}

async fn timed<T>(
    limit: Option<Duration>,
    f: impl Future<Output = std::io::Result<T>>,
) -> Result<T> {
    match within(limit, f).await {
        Ok(done) => Ok(done?),
        Err(_) => bail!(
            "the server sent nothing for {} s",
            limit.unwrap_or_default().as_secs()
        ),
    }
}

async fn read_response<R: AsyncBufRead + Unpin>(
    r: &mut R,
    fetch: &Fetch,
    sink: &mut dyn Sink,
) -> Result<Outcome> {
    let limit = fetch.read_timeout;
    let mut budget = MAX_HEAD;
    let status_line = read_line(r, &mut budget, limit).await?;
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    ensure!(
        version.starts_with("HTTP/1."),
        "not an HTTP/1 answer: {status_line:?}"
    );
    let status: u16 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow!("no status code in {status_line:?}"))?;

    let mut length: Option<u64> = None;
    let mut chunked = false;
    let mut location: Option<String> = None;
    loop {
        let line = read_line(r, &mut budget, limit).await?;
        if line.is_empty() {
            break;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow!("a header line without a colon: {line:?}"))?;
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => {
                let n: u64 = value
                    .parse()
                    .with_context(|| format!("Content-Length {value:?}"))?;
                ensure!(
                    length.is_none_or(|l| l == n),
                    "two different Content-Length headers"
                );
                length = Some(n);
            }
            "transfer-encoding" => {
                let codings: Vec<String> = value
                    .split(',')
                    .map(|c| c.trim().to_ascii_lowercase())
                    .collect();
                ensure!(
                    codings == ["chunked"],
                    "Transfer-Encoding {value:?}: only chunked is read"
                );
                chunked = true;
            }
            "location" => location = Some(value.to_string()),
            _ => {}
        }
    }

    if fetch.redirects.contains(&status) {
        return location
            .map(Outcome::Redirect)
            .ok_or_else(|| anyhow!("HTTP {status} without a Location"));
    }
    if fetch.any_success {
        ensure!((200..300).contains(&status), "HTTP {status}");
    } else {
        ensure!(status == 200, "HTTP {status}");
    }
    // A chunked body says where it ends whatever the length says (RFC 9112).
    let n = if chunked {
        read_chunked(r, limit, sink).await?
    } else if let Some(len) = length {
        read_exact_body(r, len, limit, sink).await?
    } else {
        read_to_close(r, limit, sink).await?
    };
    Ok(Outcome::Body(n))
}

/// One line, without its line ending, its bytes taken from `budget`.
async fn read_line<R: AsyncBufRead + Unpin>(
    r: &mut R,
    budget: &mut u64,
    limit: Option<Duration>,
) -> Result<String> {
    let mut buf = Vec::new();
    let n = timed(limit, (&mut *r).take(*budget).read_until(b'\n', &mut buf)).await?;
    ensure!(n > 0, "the connection closed in the middle of an answer");
    ensure!(
        buf.ends_with(b"\n"),
        "the answer ends, or runs past its limit, inside a line"
    );
    *budget -= n as u64;
    while buf.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        buf.pop();
    }
    String::from_utf8(buf).context("a line of the answer that isn't UTF-8")
}

async fn read_exact_body<R: AsyncBufRead + Unpin>(
    r: &mut R,
    len: u64,
    limit: Option<Duration>,
    sink: &mut dyn Sink,
) -> Result<u64> {
    let mut left = len;
    while left > 0 {
        let buf = timed(limit, r.fill_buf()).await?;
        ensure!(
            !buf.is_empty(),
            "the body ends after {} of {len} bytes",
            len - left
        );
        let n = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
        sink.write(&buf[..n])?;
        r.consume(n);
        left -= n as u64;
    }
    Ok(len)
}

async fn read_chunked<R: AsyncBufRead + Unpin>(
    r: &mut R,
    limit: Option<Duration>,
    sink: &mut dyn Sink,
) -> Result<u64> {
    let mut total = 0u64;
    loop {
        let mut budget = MAX_CHUNK_LINE;
        let line = read_line(r, &mut budget, limit).await?;
        let size = line.split(';').next().unwrap_or_default().trim();
        ensure!(
            !size.is_empty() && size.bytes().all(|b| b.is_ascii_hexdigit()),
            "a chunk size {line:?}"
        );
        let size =
            u64::from_str_radix(size, 16).with_context(|| format!("a chunk size {line:?}"))?;
        if size == 0 {
            // The trailers, if any, then the blank line that ends the body.
            let mut budget = MAX_CHUNK_LINE;
            while !read_line(r, &mut budget, limit).await?.is_empty() {}
            return Ok(total);
        }
        read_exact_body(r, size, limit, sink).await?;
        total = total
            .checked_add(size)
            .context("a body of more than 2^64 bytes")?;
        let mut budget = 2;
        ensure!(
            read_line(r, &mut budget, limit).await?.is_empty(),
            "a chunk runs past its size"
        );
    }
}

async fn read_to_close<R: AsyncBufRead + Unpin>(
    r: &mut R,
    limit: Option<Duration>,
    sink: &mut dyn Sink,
) -> Result<u64> {
    let mut total = 0u64;
    loop {
        let buf = match within(limit, r.fill_buf()).await {
            Err(_) => bail!(
                "the server sent nothing for {} s",
                limit.unwrap_or_default().as_secs()
            ),
            Ok(Ok(buf)) => buf,
            // rustls reports a close without TLS's close_notify as an
            // unexpected end; for a body that runs to the close, it is the end.
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(total),
            Ok(Err(e)) => return Err(e.into()),
        };
        if buf.is_empty() {
            return Ok(total);
        }
        let n = buf.len();
        sink.write(buf)?;
        r.consume(n);
        total += n as u64;
    }
}

/// Where a redirect's `Location` points, from the URL that answered with it,
/// for `--import`: a URL of the scheme that answered, or an absolute path.
fn resolve(from: &Url, location: &str) -> Result<String> {
    let prefix = from.scheme.prefix();
    if location.starts_with(prefix) {
        Ok(location.to_string())
    } else if let Some(rest) = location.strip_prefix("//") {
        Ok(format!("{prefix}{rest}"))
    } else if location.starts_with('/') {
        Ok(format!("{prefix}{}{location}", authority(from)))
    } else {
        bail!("a redirect to {location:?}: only {prefix} URLs and absolute paths are followed")
    }
}

/// Where a redirect's `Location` points as Java's `HttpURLConnection` takes
/// it (`followRedirect`): a URL of the scheme that answered, written in any
/// case; or one without a scheme, resolved against the URL that answered as
/// Java's `URL(URL, String)` resolves it. Java follows none to another
/// scheme. One that names no host, which Java would take to this machine,
/// isn't followed (`parse_any_url`).
fn resolve_java(from: &Url, location: &str) -> Result<String> {
    // `URL`'s constructor drops what surrounds the reference up to a space,
    // and a `url:` before it.
    let location = location.trim_matches(|c: char| c <= ' ');
    let location = match location.get(..4) {
        Some(head) if head.eq_ignore_ascii_case("url:") => &location[4..],
        _ => location,
    };
    let prefix = from.scheme.prefix();
    // `isValidProtocol`: a letter, then letters, digits, `+`, `-` and `.`.
    let scheme = location
        .split_once(':')
        .map(|(scheme, _)| scheme)
        .filter(|scheme| {
            scheme.starts_with(char::is_alphabetic)
                && scheme
                    .chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '+' | '-' | '.'))
        });
    if let Some(scheme) = scheme {
        ensure!(
            prefix
                .strip_suffix("://")
                .is_some_and(|ours| ours.eq_ignore_ascii_case(scheme)),
            "a redirect to {location:?}: Java follows none to another scheme"
        );
        let Some(rest) = location[scheme.len() + 1..].strip_prefix("//") else {
            bail!("a redirect to {location:?} names no host")
        };
        return Ok(format!("{prefix}{rest}"));
    }
    // A host after `//`; after `////`, Java reads a path.
    if let Some(rest) = location
        .strip_prefix("//")
        .filter(|rest| !rest.starts_with("//"))
    {
        return Ok(format!("{prefix}{rest}"));
    }
    Ok(format!(
        "{prefix}{}{}",
        authority(from),
        java_relative(&from.path, location)
    ))
}

/// Java's `URL(URL, String)` for a reference without a scheme or a host
/// (`URLStreamHandler.parseURL`): the path and query it names, from the path
/// and query that answered. Empty, or a fragment alone, it names those again;
/// a query alone keeps the directory and not the file, as Java has it.
fn java_relative(from: &str, reference: &str) -> String {
    let reference = reference.split('#').next().unwrap_or_default();
    if reference.is_empty() {
        return from.to_string();
    }
    let base = from.split_once('?').map_or(from, |(path, _)| path);
    let (path, query) = match reference.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (reference, None),
    };
    let path = if path.starts_with('/') {
        path.to_string()
    } else if !path.is_empty() {
        let directory = base.rfind('/').map_or("", |at| &base[..=at]);
        java_dot_segments(format!("{directory}{path}"))
    } else {
        let directory = base.rfind('/').map_or("", |at| &base[..at]);
        format!("{directory}/")
    };
    match query {
        Some(query) => format!("{path}?{query}"),
        None => path,
    }
}

/// Java's `parseURL` on a path it joined: each `/./` gone, each `/../` with
/// the segment before it where there is one, a `/..` at the end likewise, a
/// `./` at the start and a `.` at the end.
fn java_dot_segments(mut path: String) -> String {
    while let Some(at) = path.find("/./") {
        path.replace_range(at..at + 2, "");
    }
    let mut from = 0;
    while let Some(at) = path[from..].find("/../").map(|found| found + from) {
        // `lastIndexOf('/', i - 1)`, and Java's check that what goes isn't
        // a `/../` the path starts with.
        match path[..at].rfind('/') {
            Some(before) if !(before == 0 && path.starts_with("/../")) => {
                path.replace_range(before..at + 3, "");
                from = 0;
            }
            _ => from = at + 3,
        }
    }
    while path.ends_with("/..") {
        let at = path.find("/..").unwrap_or_default();
        match path[..at].rfind('/') {
            Some(before) => path.truncate(before + 1),
            None => break,
        }
    }
    if path.starts_with("./") && path.len() > 2 {
        path.replace_range(..2, "");
    }
    if path.ends_with("/.") {
        path.pop();
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Sink for Vec<u8> {
        fn write(&mut self, data: &[u8]) -> Result<()> {
            self.extend_from_slice(data);
            Ok(())
        }
    }

    async fn answer(raw: &[u8]) -> (Result<Outcome>, Vec<u8>) {
        answer_to(&IMPORT, raw).await
    }

    async fn answer_to(fetch: &Fetch, raw: &[u8]) -> (Result<Outcome>, Vec<u8>) {
        let mut body = Vec::new();
        let mut reader = BufReader::new(raw);
        let outcome = read_response(&mut reader, fetch, &mut body).await;
        (outcome, body)
    }

    #[test]
    fn urls_are_split_as_a_request_needs_them() {
        let url =
            parse_url("https://Import.Mainnet.rskcomputing.net/dbs/mainnet/k/index.json").unwrap();
        assert_eq!(
            url,
            Url {
                scheme: Scheme::Https,
                host: "import.mainnet.rskcomputing.net".into(),
                port: 443,
                path: "/dbs/mainnet/k/index.json".into(),
            }
        );
        let url = parse_url("https://example.org:8443?x=1#top").unwrap();
        assert_eq!((url.port, url.path.as_str()), (8443, "/?x=1"));
        assert_eq!(parse_url("https://example.org").unwrap().path, "/");
        for bad in [
            "http://example.org/",
            "https:///path",
            "https://user@example.org/",
            "https://[::1]/",
            "https://example.org:x/",
            "https://example.org/a b",
            "https://example.org/a\r\nX-Injected: 1",
            "https://exa mple.org/",
            "https://example.org\r\nX: 1/",
        ] {
            assert!(parse_url(bad).is_err(), "{bad} was taken");
        }
    }

    #[tokio::test]
    async fn a_body_of_a_given_length_is_read_whole_and_no_further() {
        let (outcome, body) =
            answer(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nServer: x\r\n\r\nhelloEXTRA").await;
        assert_eq!(outcome.unwrap(), Outcome::Body(5));
        assert_eq!(body, b"hello");
    }

    #[tokio::test]
    async fn a_body_shorter_than_its_length_is_refused() {
        let (outcome, _) = answer(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nhello").await;
        assert!(outcome.unwrap_err().to_string().contains("5 of 9"));
    }

    #[tokio::test]
    async fn a_chunked_body_is_put_back_together() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 1\r\n\r\n\
                    5;ext=1\r\nhello\r\n6\r\n world\r\n0\r\nTrailer: t\r\n\r\n";
        let (outcome, body) = answer(raw).await;
        assert_eq!(outcome.unwrap(), Outcome::Body(11));
        assert_eq!(body, b"hello world");
    }

    #[tokio::test]
    async fn a_chunk_that_overruns_its_size_or_a_bad_size_is_refused() {
        let overrun =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nhello\r\n0\r\n\r\n";
        assert!(answer(overrun).await.0.is_err());
        let signed = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n+3\r\nhey\r\n0\r\n\r\n";
        assert!(answer(signed).await.0.is_err());
    }

    #[tokio::test]
    async fn without_a_length_the_body_runs_to_the_close() {
        let (outcome, body) = answer(b"HTTP/1.0 200 OK\r\n\r\nall of it").await;
        assert_eq!(outcome.unwrap(), Outcome::Body(9));
        assert_eq!(body, b"all of it");
    }

    #[tokio::test]
    async fn a_redirect_is_handed_back_and_its_body_left_unread() {
        let raw = b"HTTP/1.1 302 Found\r\nLocation: /elsewhere\r\nContent-Length: 4\r\n\r\nbody";
        let (outcome, body) = answer(raw).await;
        assert_eq!(outcome.unwrap(), Outcome::Redirect("/elsewhere".into()));
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn anything_but_200_or_a_redirect_is_an_error() {
        for raw in [
            &b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"[..],
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 1\r\n\r\nx",
            b"HTTP/1.1 301 Moved\r\n\r\n",
            b"SSH-2.0-OpenSSH\r\n\r\n",
        ] {
            assert!(answer(raw).await.0.is_err());
        }
    }

    #[tokio::test]
    async fn framing_that_can_be_read_two_ways_is_refused() {
        let two_lengths = b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nab";
        assert!(answer(two_lengths).await.0.is_err());
        let gzip = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n0\r\n\r\n";
        assert!(answer(gzip).await.0.is_err());
    }

    #[tokio::test]
    async fn headers_past_their_limit_are_refused() {
        let mut raw = b"HTTP/1.1 200 OK\r\nX-Long: ".to_vec();
        raw.extend(std::iter::repeat_n(b'a', MAX_HEAD as usize));
        raw.extend_from_slice(b"\r\n\r\n");
        assert!(answer(&raw).await.0.is_err());
    }

    #[test]
    fn redirects_resolve_to_https_only() {
        let from = parse_url("https://a.example:8443/x").unwrap();
        assert_eq!(
            resolve(&from, "/y?z=1").unwrap(),
            "https://a.example:8443/y?z=1"
        );
        assert_eq!(
            resolve(&from, "//b.example/y").unwrap(),
            "https://b.example/y"
        );
        assert_eq!(
            resolve(&from, "https://c.example/y").unwrap(),
            "https://c.example/y"
        );
        assert!(resolve(&from, "http://c.example/y").is_err());
        assert!(resolve(&from, "y").is_err());
    }

    /// The price's fetch takes `http://` too, each scheme on its own port, and
    /// a redirect stays on the scheme it came from, as Java's does.
    #[test]
    fn a_plain_http_url_is_split_and_redirected_on_its_own_scheme() {
        let url = parse_any_url("http://Price.Example/ticker?ids=bitcoin").unwrap();
        assert_eq!(
            url,
            Url {
                scheme: Scheme::Http,
                host: "price.example".into(),
                port: 80,
                path: "/ticker?ids=bitcoin".into(),
            }
        );
        assert_eq!(parse_any_url("https://price.example").unwrap().port, 443);
        assert!(parse_any_url("ftp://price.example/").is_err());
        assert!(parse_any_url("url").is_err());

        let from = parse_any_url("http://a.example:8080/x").unwrap();
        assert_eq!(resolve(&from, "/y").unwrap(), "http://a.example:8080/y");
        assert_eq!(
            resolve(&from, "//b.example/y").unwrap(),
            "http://b.example/y"
        );
        assert_eq!(
            resolve(&from, "http://c.example/y").unwrap(),
            "http://c.example/y"
        );
        assert!(resolve(&from, "https://c.example/y").is_err());
        let plain = parse_any_url("http://a.example/x").unwrap();
        assert_eq!(authority(&plain), "a.example");
    }

    /// As Java fetches for rskj: every `2xx` is an answer, 300 to 303 and 307
    /// are followed and 308 isn't, and a timeout of zero is no limit.
    /// `--import` keeps to `200` and follows 308.
    #[tokio::test]
    async fn a_java_fetch_takes_every_2xx_and_follows_javas_redirects() {
        let java = Fetch::java(Duration::from_secs(3));
        assert_eq!(java.read_timeout, Some(Duration::from_secs(3)));
        assert_eq!(Fetch::java(Duration::ZERO).connect_timeout, None);
        assert_eq!(java.max_redirects, 19, "Java's 20th request is its last");

        let no_content = b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n";
        assert_eq!(
            answer_to(&java, no_content).await.0.unwrap(),
            Outcome::Body(0)
        );
        assert!(answer(no_content).await.0.is_err());
        let (outcome, body) = answer_to(
            &java,
            b"HTTP/1.1 206 Partial\r\nContent-Length: 2\r\n\r\n42",
        )
        .await;
        assert_eq!((outcome.unwrap(), body), (Outcome::Body(2), b"42".to_vec()));

        let moved =
            |code: u16| format!("HTTP/1.1 {code} Moved\r\nLocation: /p\r\n\r\n").into_bytes();
        for code in [300, 301, 302, 303, 307] {
            assert_eq!(
                answer_to(&java, &moved(code)).await.0.unwrap(),
                Outcome::Redirect("/p".into()),
                "{code}"
            );
        }
        for code in [305, 308] {
            assert!(answer_to(&java, &moved(code)).await.0.is_err(), "{code}");
        }
        assert_eq!(
            answer(&moved(308)).await.0.unwrap(),
            Outcome::Redirect("/p".into())
        );
    }

    /// Java's `HttpURLConnection` follows a redirect within its scheme, the
    /// scheme written in any case, and resolves one without a scheme as
    /// `URL(URL, String)` does.
    #[test]
    fn a_java_redirect_resolves_as_javas_url_does() {
        let from = parse_any_url("https://a.example:8443/x/y?q=1").unwrap();
        let to = |location: &str| resolve_java(&from, location);
        assert_eq!(to("https://b.example/z").unwrap(), "https://b.example/z");
        assert_eq!(to("HTTPS://b.example/z").unwrap(), "https://b.example/z");
        assert!(to("http://b.example/z").is_err(), "another scheme");
        assert!(to("ftp://b.example/z").is_err());
        assert!(to("mailto:someone").is_err());
        assert!(to("https:z").is_err(), "no host");
        assert_eq!(to("//c.example/z").unwrap(), "https://c.example/z");
        assert_eq!(to("/z?w=2").unwrap(), "https://a.example:8443/z?w=2");
        assert_eq!(to("z").unwrap(), "https://a.example:8443/x/z");
        assert_eq!(to("./z").unwrap(), "https://a.example:8443/x/z");
        assert_eq!(to("../z").unwrap(), "https://a.example:8443/z");
        assert_eq!(to("z/..").unwrap(), "https://a.example:8443/x/");
        assert_eq!(to("z/.").unwrap(), "https://a.example:8443/x/z/");
        assert_eq!(
            to("../../z").unwrap(),
            "https://a.example:8443/../z",
            "Java keeps a /../ it can't take back"
        );
        assert_eq!(
            to("?w=2").unwrap(),
            "https://a.example:8443/x/?w=2",
            "Java keeps the directory, not the file"
        );
        assert_eq!(to("").unwrap(), "https://a.example:8443/x/y?q=1");
        assert_eq!(to("#top").unwrap(), "https://a.example:8443/x/y?q=1");
        assert_eq!(
            to("a/b:c").unwrap(),
            "https://a.example:8443/x/a/b:c",
            "a colon after a slash is no scheme"
        );
        assert_eq!(
            to("////c.example/z").unwrap(),
            "https://a.example:8443////c.example/z",
            "four slashes are a path"
        );
        assert_eq!(to(" \t/z\u{1} ").unwrap(), "https://a.example:8443/z");
        assert_eq!(to("URL:/z").unwrap(), "https://a.example:8443/z");
        assert_eq!(
            to("url:https://b.example/z").unwrap(),
            "https://b.example/z"
        );
        assert!(to("é:z").is_err(), "a protocol Java has no handler for");
        let plain = parse_any_url("http://a.example/x").unwrap();
        assert_eq!(
            resolve_java(&plain, "Http://b.example/").unwrap(),
            "http://b.example/"
        );
    }
}
