//! Adding secrets (`docs/specs/secrets.md`): for a CONNECT to a host that
//! has a secret bound, the proxy answers TLS itself with a leaf from the
//! session's CA, reads exactly one request with a strict parser, and writes
//! a new request upstream over its own verified TLS connection, with the
//! secret's header added. Nothing the agent sends after that request is
//! passed on, so no smuggled request can carry the header.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::secrets::{SecretInfo, SecretValue};
use ovra_core::services::{NetReporter, Secrets};
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf,
};
use tokio::time::timeout;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use zeroize::{Zeroize, Zeroizing};

use crate::ca::SessionCa;
use crate::dial::{Conn, Stream};
use crate::serve::{HEAD_TIMEOUT, IDLE_TIMEOUT, idle, read_head, respond};

const MAX_HEADERS: usize = 64;
/// A chunk-size line: hex digits, perhaps an extension, CRLF.
const MAX_CHUNK_LINE: usize = 1024;

/// The secrets of one session's project, fixed when its proxy started.
#[derive(Debug)]
pub(crate) struct Bound {
    pub(crate) ca: SessionCa,
    pub(crate) secrets: Vec<SecretInfo>,
    pub(crate) project: ProjectId,
    pub(crate) store: Arc<dyn Secrets>,
    pub(crate) client: Arc<ClientConfig>,
}

impl Bound {
    /// The secrets bound to `host` on `port`; a binding without a port
    /// means 443.
    pub(crate) fn for_host(&self, host: &str, port: u16) -> Vec<SecretInfo> {
        self.secrets
            .iter()
            .filter(|s| binding(&s.host) == Some((host.to_owned(), port)))
            .cloned()
            .collect()
    }
}

/// A binding's host and port; `None` for a port that does not parse, so
/// such a binding is never used.
pub(crate) fn binding(host: &str) -> Option<(String, u16)> {
    match host.split_once(':') {
        Some((name, port)) => Some((name.to_owned(), port.parse().ok()?)),
        None => Some((host.to_owned(), 443)),
    }
}

/// How the request's body is framed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Body {
    Length(u64),
    Chunked,
}

/// A request the proxy will rebuild: its head without the secrets, ending
/// before the blank line.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Checked {
    pub(crate) head: Vec<u8>,
    pub(crate) body: Body,
}

/// Headers never passed on: this hop's, and the framing the proxy writes
/// itself.
const DROPPED: [&str; 10] = [
    "host",
    "connection",
    "keep-alive",
    "te",
    "trailer",
    "upgrade",
    "content-length",
    "transfer-encoding",
    // The proxy reads the body without waiting for `100 Continue`.
    "expect",
    // It asks for an answer it can search for the value: uncompressed.
    "accept-encoding",
];

/// Parses one request head for `host` (`port`) strictly, refusing every
/// form two parsers could read differently, and rebuilds it without the
/// headers the secrets replace.
pub(crate) fn check(
    raw: &[u8],
    host: &str,
    port: u16,
    secrets: &[SecretInfo],
) -> Result<Checked, &'static str> {
    // Bare LF line ends and folded header lines.
    for (i, b) in raw.iter().enumerate() {
        if *b == b'\n' && (i == 0 || raw[i - 1] != b'\r') {
            return Err("a line ends without CR");
        }
        if *b == b'\n' && matches!(raw.get(i + 1), Some(b' ' | b'\t')) {
            return Err("folded header lines are not accepted");
        }
    }
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut parsed = httparse::Request::new(&mut headers);
    match parsed.parse(raw) {
        Ok(httparse::Status::Complete(n)) if n == raw.len() => {}
        _ => return Err("not one HTTP request"),
    }
    let (Some(method), Some(target), Some(1)) = (parsed.method, parsed.path, parsed.version) else {
        return Err("only HTTP/1.1 requests are accepted");
    };
    if method == "CONNECT" {
        return Err("CONNECT inside the tunnel is not accepted");
    }
    if !target.starts_with('/') {
        return Err("send the path only (origin-form)");
    }
    let mut hosts = parsed
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("host"));
    let named = hosts.next().ok_or("the request has no Host")?;
    if hosts.next().is_some() {
        return Err("more than one Host");
    }
    let named = std::str::from_utf8(named.value)
        .map_err(|_| "a bad Host")?
        .to_ascii_lowercase();
    let authority = if port == 443 {
        host.to_owned()
    } else {
        format!("{host}:{port}")
    };
    if named != host && named != format!("{host}:{port}") {
        return Err("the Host names another host");
    }
    let mut length: Option<u64> = None;
    let mut chunked = false;
    for h in parsed.headers.iter() {
        let name = h.name.to_ascii_lowercase();
        let value = std::str::from_utf8(h.value).map_err(|_| "a header that is not text")?;
        match name.as_str() {
            "upgrade" => return Err("upgrades are not accepted"),
            "transfer-encoding" => {
                if chunked || !value.trim().eq_ignore_ascii_case("chunked") {
                    return Err("only one Transfer-Encoding: chunked is accepted");
                }
                chunked = true;
            }
            "content-length" => {
                let ok = !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
                let parsed = value.parse::<u64>().ok().filter(|_| ok);
                if length.is_some() || parsed.is_none() {
                    return Err("one plain Content-Length is accepted");
                }
                length = parsed;
            }
            _ => {}
        }
    }
    if chunked && length.is_some() {
        return Err("Content-Length and Transfer-Encoding together");
    }
    let connection: Vec<String> = parsed
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("connection"))
        .flat_map(|h| {
            String::from_utf8_lossy(h.value)
                .split(',')
                .map(|n| n.trim().to_ascii_lowercase())
                .collect::<Vec<_>>()
        })
        .collect();
    let mut head = format!("{method} {target} HTTP/1.1\r\nHost: {authority}\r\n").into_bytes();
    for h in parsed.headers.iter() {
        let name = h.name.to_ascii_lowercase();
        let replaced = secrets
            .iter()
            .any(|s| s.header.eq_ignore_ascii_case(h.name));
        if DROPPED.contains(&name.as_str())
            || name.starts_with("proxy-")
            || connection.contains(&name)
            || replaced
        {
            continue;
        }
        head.extend_from_slice(h.name.as_bytes());
        head.extend_from_slice(b": ");
        head.extend_from_slice(h.value);
        head.extend_from_slice(b"\r\n");
    }
    let body = if chunked {
        head.extend_from_slice(b"Transfer-Encoding: chunked\r\n");
        Body::Chunked
    } else {
        let n = length.unwrap_or(0);
        if length.is_some() {
            head.extend_from_slice(format!("Content-Length: {n}\r\n").as_bytes());
        }
        Body::Length(n)
    };
    head.extend_from_slice(b"Accept-Encoding: identity\r\nConnection: close\r\n");
    Ok(Checked { head, body })
}

/// Serves one tunnel to a bound host: `client` has had its 200, `server`
/// is the TCP connection to the host (or the user's proxy's tunnel).
pub(crate) async fn serve(
    client: &mut impl Stream,
    server: Conn,
    host: &str,
    port: u16,
    secrets: Vec<SecretInfo>,
    bound: &Bound,
    reporter: &NetReporter,
) -> io::Result<()> {
    let config = bound.ca.server_for(host).map_err(io::Error::other)?;
    let Ok(accepted) = timeout(HEAD_TIMEOUT, TlsAcceptor::from(config).accept(client)).await else {
        return Ok(());
    };
    let mut tls = accepted?;
    let Ok(read) = timeout(HEAD_TIMEOUT, read_head(&mut tls)).await else {
        return Ok(());
    };
    let Some((raw, rest)) = read? else {
        return respond(&mut tls, 400, "the request head is too large or incomplete").await;
    };
    let checked = match check(&raw, host, port, &secrets) {
        Ok(checked) => checked,
        Err(why) => return respond(&mut tls, 400, why).await,
    };
    let values = match values(bound, &secrets).await {
        Ok(values) => values,
        Err(why) => return respond(&mut tls, 502, &why).await,
    };
    let Ok(name) = ServerName::try_from(host.to_owned()) else {
        return respond(&mut tls, 502, "the host is not a valid TLS name").await;
    };
    let connector = TlsConnector::from(bound.client.clone());
    let upstream = match timeout(HEAD_TIMEOUT, connector.connect(name, server)).await {
        Ok(Ok(upstream)) => upstream,
        Ok(Err(e)) => {
            let why = format!("TLS to the host failed: {e}");
            reporter.refused(&format!("{host}:{port}"), &why);
            return respond(&mut tls, 502, &why).await;
        }
        Err(_) => return respond(&mut tls, 502, "TLS to the host timed out").await,
    };
    let mut upstream = upstream;
    // Sized up front so it never moves, leaving no copy of a value behind.
    let extra: usize = secrets
        .iter()
        .zip(&values)
        .map(|(s, v)| s.header.len() + s.prefix.len() + v.expose().len() + 4)
        .sum();
    let mut head = Vec::with_capacity(checked.head.len() + extra + 2);
    head.extend_from_slice(&checked.head);
    for (secret, value) in secrets.iter().zip(&values) {
        head.extend_from_slice(secret.header.as_bytes());
        head.extend_from_slice(b": ");
        head.extend_from_slice(secret.prefix.as_bytes());
        head.extend_from_slice(value.expose().as_bytes());
        head.extend_from_slice(b"\r\n");
    }
    head.extend_from_slice(b"\r\n");
    // What the answer must not carry back.
    let hidden: Vec<Zeroizing<Vec<u8>>> = values
        .iter()
        .map(|v| v.expose().as_bytes())
        .filter(|v| v.len() >= MIN_MASKED)
        .map(|v| Zeroizing::new(v.to_vec()))
        .collect();
    drop(values);
    let written = upstream.write_all(&head).await;
    head.zeroize();
    written?;
    for secret in &secrets {
        reporter.secret_used(&secret.name, &secret.host);
    }
    let (tls_read, mut tls_write) = tokio::io::split(tls);
    let moved = AtomicBool::new(false);
    let watched = Watched {
        inner: tls_read,
        moved: &moved,
    };
    let mut body = BufReader::new(std::io::Cursor::new(rest).chain(watched));
    let copy = async {
        match checked.body {
            Body::Length(n) => copy_exact(&mut body, &mut upstream, n).await,
            Body::Chunked => copy_chunked(&mut body, &mut upstream).await,
        }
    };
    // A body that stops arriving ends the tunnel, as an idle one does.
    let sent = tokio::select! {
        sent = copy => sent,
        () = idle(&moved, IDLE_TIMEOUT) => return Ok(()),
    };
    if let Err(e) = sent {
        if e.kind() == io::ErrorKind::InvalidData {
            // Nothing reached the host as a whole request; tell the agent.
            let _ = respond(&mut tls_write, 400, "a malformed chunked body").await;
        }
        return Ok(());
    }
    upstream.flush().await?;
    let moved = AtomicBool::new(false);
    tokio::select! {
        relayed = relay_masked(&mut upstream, &mut tls_write, &hidden, &moved) => relayed,
        () = idle(&moved, IDLE_TIMEOUT) => Ok(()),
    }
}

/// Values shorter than this are not masked in answers: they are no real
/// token, and masking them would garble the answer.
const MIN_MASKED: usize = 8;

/// Relays the answer, every copy of each of `hidden` replaced by as many
/// `*`, so an endpoint that echoes the request cannot hand a value back
/// (`docs/specs/secrets-consent.md`). The length stays, so the framing
/// does too. The last bytes wait for the next read: a copy may span two.
async fn relay_masked(
    from: &mut (impl AsyncRead + Unpin),
    to: &mut (impl AsyncWrite + Unpin),
    hidden: &[Zeroizing<Vec<u8>>],
    moved: &AtomicBool,
) -> io::Result<()> {
    let keep = hidden.iter().map(|h| h.len()).max().unwrap_or(1) - 1;
    let mut pending = Zeroizing::new(Vec::<u8>::new());
    let mut buf = Zeroizing::new(vec![0u8; 16 * 1024]);
    loop {
        let n = from.read(&mut buf).await?;
        if n > 0 {
            moved.store(true, Ordering::Relaxed);
        }
        pending.extend_from_slice(&buf[..n]);
        mask(&mut pending, hidden);
        let ready = if n == 0 {
            pending.len()
        } else {
            pending.len().saturating_sub(keep)
        };
        to.write_all(&pending[..ready]).await?;
        pending.drain(..ready);
        if n == 0 {
            let _ = to.shutdown().await;
            return Ok(());
        }
    }
}

/// Replaces every copy of each of `hidden` in `bytes` with `*`.
pub(crate) fn mask(bytes: &mut [u8], hidden: &[Zeroizing<Vec<u8>>]) {
    for needle in hidden {
        let mut at = 0;
        while at + needle.len() <= bytes.len() {
            if bytes[at..at + needle.len()] == needle[..] {
                bytes[at..at + needle.len()].fill(b'*');
                at += needle.len();
            } else {
                at += 1;
            }
        }
    }
}

/// A reader that notes when bytes arrive, for the idle timeout.
struct Watched<'a, R> {
    inner: R,
    moved: &'a AtomicBool,
}

impl<R: AsyncRead + Unpin> AsyncRead for Watched<'_, R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let polled = Pin::new(&mut self.inner).poll_read(cx, buf);
        if buf.filled().len() > before {
            self.moved.store(true, Ordering::Relaxed);
        }
        polled
    }
}

/// Reads the values on a blocking thread: the keychain may block.
async fn values(bound: &Bound, secrets: &[SecretInfo]) -> Result<Vec<SecretValue>, String> {
    let store = bound.store.clone();
    let project = bound.project.clone();
    let names: Vec<String> = secrets.iter().map(|s| s.name.clone()).collect();
    let read = tokio::task::spawn_blocking(move || {
        names
            .iter()
            .map(|name| store.value(&project, name))
            .collect::<Result<Vec<_>, _>>()
    });
    // A keychain waiting on a prompt must not hold the tunnel.
    let Ok(read) = timeout(HEAD_TIMEOUT, read).await else {
        return Err("reading a secret from the keychain timed out".into());
    };
    match read {
        Ok(Ok(values)) => Ok(values),
        Ok(Err(e)) => Err(format!(
            "a secret for this host cannot be read, so the request was not sent: {}",
            e.message
        )),
        Err(_) => Err("reading a secret stopped unexpectedly".into()),
    }
}

async fn copy_exact(
    from: &mut (impl AsyncRead + Unpin),
    to: &mut (impl AsyncWrite + Unpin),
    n: u64,
) -> io::Result<()> {
    let copied = tokio::io::copy(&mut from.take(n), to).await?;
    if copied != n {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the body ended early",
        ));
    }
    Ok(())
}

fn malformed() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "a malformed chunked body")
}

/// One CRLF-terminated line, at most `MAX_CHUNK_LINE` bytes, without its
/// CRLF.
async fn line(from: &mut (impl AsyncBufReadExt + Unpin)) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let n = from
        .take(MAX_CHUNK_LINE as u64)
        .read_until(b'\n', &mut out)
        .await?;
    if n == 0 || !out.ends_with(b"\r\n") {
        return Err(malformed());
    }
    out.truncate(out.len() - 2);
    if out.contains(&b'\r') || out.contains(&b'\n') {
        return Err(malformed());
    }
    Ok(out)
}

/// Decodes a chunked body strictly and writes it again as chunks: what the
/// host reads is exactly what the proxy read. No trailers.
async fn copy_chunked(
    from: &mut (impl AsyncBufReadExt + Unpin),
    to: &mut (impl AsyncWrite + Unpin),
) -> io::Result<()> {
    loop {
        let size_line = line(from).await?;
        let digits = size_line.split(|b| *b == b';').next().unwrap_or_default();
        let ok =
            !digits.is_empty() && digits.len() <= 16 && digits.iter().all(u8::is_ascii_hexdigit);
        let size = std::str::from_utf8(digits)
            .ok()
            .filter(|_| ok)
            .and_then(|d| u64::from_str_radix(d, 16).ok())
            .ok_or_else(malformed)?;
        if size == 0 {
            if !line(from).await?.is_empty() {
                return Err(malformed());
            }
            return to.write_all(b"0\r\n\r\n").await;
        }
        to.write_all(format!("{size:x}\r\n").as_bytes()).await?;
        copy_exact(from, to, size).await?;
        if !line(from).await?.is_empty() {
            return Err(malformed());
        }
        to.write_all(b"\r\n").await?;
    }
}

#[cfg(test)]
mod tests;
