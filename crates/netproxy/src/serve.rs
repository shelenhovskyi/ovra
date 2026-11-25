//! One client connection: read the proxy request, decide it, connect, and
//! pass bytes both ways. Plain HTTP gets one request per connection.

mod loopback;

pub(crate) use loopback::{Grants, grants};

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use ovra_core::netproxy::is_reachable;
use ovra_core::services::NetReporter;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;
use tokio::time::timeout;

use crate::dial::{Conn, Dial, Stream};
use crate::inject::Bound;
use crate::rules::{Rules, Verdict};
use crate::upstream::{Server, Upstream};

/// Request heads are small; anything larger is not one.
const MAX_HEAD: usize = 16 * 1024;
const MAX_HEADERS: usize = 64;
pub(crate) const HEAD_TIMEOUT: Duration = Duration::from_secs(30);
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);
/// An open connection where nothing moves this long is closed.
pub(crate) const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// What every session's proxy shares.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) dial: Arc<dyn Dial>,
    pub(crate) upstream: Upstream,
}

/// One session's proxy.
#[derive(Debug)]
pub(crate) struct SessionNet {
    pub(crate) rules: RwLock<Rules>,
    pub(crate) reporter: NetReporter,
    /// The project's secrets, if it has any.
    pub(crate) bound: Option<Bound>,
    /// The managed servers it may reach as `localhost:<port>`.
    pub(crate) grants: watch::Sender<Arc<Grants>>,
}

pub(crate) async fn serve(mut client: impl Stream, shared: Arc<Shared>, session: Arc<SessionNet>) {
    let _ = handle(&mut client, &shared, &session).await;
    let _ = client.shutdown().await;
}

/// Why a connection was not made.
enum Problem {
    /// The rules refuse it: reported to the core.
    Refused(String),
    /// It was allowed but could not be made.
    Failed(String),
}

/// A parsed proxy request.
struct Request {
    connect: bool,
    host: String,
    port: u16,
    /// Plain HTTP: the request line and headers to send on.
    method: String,
    target: String,
    version: u8,
    headers: Vec<(String, Vec<u8>)>,
    /// Plain HTTP: the request body's length; nothing after it is passed on.
    body: u64,
    /// The port, if the target is exactly `localhost[:P]` or
    /// `127.0.0.1[:P]`, or the connection began with `OVRA-LOCAL <P>`.
    loopback: Option<u16>,
    /// The `Host` and `Origin` headers as sent, for the loopback rule.
    hosts: Vec<Vec<u8>>,
    origins: Vec<Vec<u8>>,
    /// A WebSocket upgrade: `GET`, `Upgrade: websocket`,
    /// `Connection: upgrade`.
    websocket: bool,
}

impl Request {
    /// `host:port`, with brackets around an IPv6 address.
    fn name(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// The head to send for plain HTTP: origin-form to the server itself,
    /// absolute-form to an upstream proxy. One request, then the server
    /// closes, unless it is a WebSocket `upgrade`.
    fn head(&self, upstream: Option<&Server>, upgrade: bool) -> Vec<u8> {
        let authority = if self.port == 80 {
            self.name()
                .rsplit_once(':')
                .map_or_else(String::new, |(h, _)| h.to_owned())
        } else {
            self.name()
        };
        let target = match upstream {
            Some(_) => format!("http://{authority}{}", self.target),
            None => self.target.clone(),
        };
        let mut out = format!("{} {target} HTTP/1.{}\r\n", self.method, self.version).into_bytes();
        for (name, value) in &self.headers {
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(value);
            out.extend_from_slice(b"\r\n");
        }
        let connection = if upgrade {
            "Upgrade\r\nUpgrade: websocket"
        } else {
            "close"
        };
        out.extend_from_slice(
            format!("Host: {authority}\r\nConnection: {connection}\r\n").as_bytes(),
        );
        if let Some(auth) = upstream.and_then(|s| s.auth.as_ref()) {
            out.extend_from_slice(format!("Proxy-Authorization: {auth}\r\n").as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        out
    }
}

async fn handle(client: &mut impl Stream, shared: &Shared, session: &SessionNet) -> io::Result<()> {
    let Ok(read) = timeout(HEAD_TIMEOUT, read_head(client)).await else {
        return Ok(());
    };
    let Some((head, rest)) = read? else {
        return respond(client, 400, "the request head is too large or incomplete").await;
    };
    // The sandbox's forwarder: a client that connected to `localhost:P`.
    let (local, head) = match head.strip_prefix(b"OVRA-LOCAL ") {
        None => (None, &head[..]),
        Some(after) => match loopback::preamble(after) {
            Some((port, head)) => (Some(port), head),
            None => return respond(client, 400, "a bad OVRA-LOCAL line").await,
        },
    };
    let request = match parse(head, local) {
        Ok(request) => request,
        Err(why) => return respond(client, 400, why).await,
    };
    if request.loopback.is_some() && (local.is_some() || !configured(&request, session)) {
        return loopback::serve(client, request, rest, session).await;
    }
    let opened = open(&request, shared, session).await;
    let (mut server, early) = match opened {
        Ok(opened) => opened,
        Err(Problem::Refused(why)) => {
            session.reporter.refused(&request.name(), &why);
            return respond(client, 403, &why).await;
        }
        Err(Problem::Failed(why)) => return respond(client, 502, &why).await,
    };
    let secrets = match &session.bound {
        Some(bound) if request.connect => bound.for_host(&request.host, request.port),
        _ => Vec::new(),
    };
    if let (Some(bound), false) = (&session.bound, secrets.is_empty()) {
        if !early.is_empty() || !rest.is_empty() {
            // TLS must start after our answer; anything sooner is not TLS
            // the proxy can answer.
            return respond(client, 400, "wait for the CONNECT answer before TLS").await;
        }
        client
            .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
            .await?;
        let (host, port) = (request.host.as_str(), request.port);
        let reporter = &session.reporter;
        return crate::inject::serve(client, server, host, port, secrets, bound, reporter).await;
    }
    if request.connect {
        client
            .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
            .await?;
        client.write_all(&early).await?;
        server.write_all(&rest).await?;
        return relay(client, &mut server, IDLE_TIMEOUT).await;
    }
    let upstream = shared.upstream.server(&request.host, false);
    let head = request.head(upstream, false);
    one_request(client, server, &head, &rest, request.body).await
}

/// Whether the target is an endpoint the user configured on this machine
/// (the base list's `private_ok`), which keeps today's path.
fn configured(request: &Request, session: &SessionNet) -> bool {
    let verdict = session
        .rules
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .verdict(&request.host, request.port);
    verdict == Verdict::Allowed { private_ok: true }
}

/// Sends `head` and exactly one body of `body` bytes, the first of them
/// in `rest`, and passes the answer back: a pipelined second request never
/// passes, so it cannot reach a host the rules did not check.
async fn one_request(
    client: &mut impl Stream,
    mut server: impl Stream,
    head: &[u8],
    rest: &[u8],
    body: u64,
) -> io::Result<()> {
    server.write_all(head).await?;
    let first = rest.len().min(usize::try_from(body).unwrap_or(usize::MAX));
    server.write_all(&rest[..first]).await?;
    let remaining = body - first as u64;
    let (from_client, mut to_client) = tokio::io::split(client);
    let (mut from_server, mut to_server) = tokio::io::split(server);
    let mut body_in = from_client.take(remaining);
    let moved = AtomicBool::new(false);
    // The server's side stays open after the body: some servers close on a
    // half-closed request.
    let both = async {
        tokio::try_join!(
            pump(&mut body_in, &mut to_server, &moved, false),
            pump(&mut from_server, &mut to_client, &moved, true),
        )
    };
    tokio::select! {
        result = both => result.map(|_| ()),
        () = idle(&moved, IDLE_TIMEOUT) => Ok(()),
    }
}

/// Returns once nothing moved for `period`.
pub(crate) async fn idle(moved: &AtomicBool, period: Duration) {
    loop {
        tokio::time::sleep(period).await;
        if !moved.swap(false, Ordering::Relaxed) {
            return;
        }
    }
}

/// Passes bytes both ways until either side closes or nothing moves for
/// `idle`.
async fn relay(a: &mut impl Stream, b: &mut impl Stream, period: Duration) -> io::Result<()> {
    let (mut a_read, mut a_write) = tokio::io::split(a);
    let (mut b_read, mut b_write) = tokio::io::split(b);
    let moved = AtomicBool::new(false);
    let both = async {
        tokio::try_join!(
            pump(&mut a_read, &mut b_write, &moved, true),
            pump(&mut b_read, &mut a_write, &moved, true)
        )
    };
    tokio::select! {
        result = both => result.map(|_| ()),
        () = idle(&moved, period) => Ok(()),
    }
}

/// Passes `from` to `to` until `from` closes or nothing moves for
/// `period`.
pub(crate) async fn relay_one_way(
    from: &mut (impl tokio::io::AsyncRead + Unpin),
    to: &mut (impl tokio::io::AsyncWrite + Unpin),
    period: Duration,
) -> io::Result<()> {
    let moved = AtomicBool::new(false);
    tokio::select! {
        result = pump(from, to, &moved, true) => result,
        () = idle(&moved, period) => Ok(()),
    }
}

async fn pump(
    from: &mut (impl tokio::io::AsyncRead + Unpin),
    to: &mut (impl tokio::io::AsyncWrite + Unpin),
    moved: &AtomicBool,
    shutdown: bool,
) -> io::Result<()> {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = from.read(&mut buf).await?;
        if n == 0 {
            if shutdown {
                let _ = to.shutdown().await;
            }
            return Ok(());
        }
        to.write_all(&buf[..n]).await?;
        moved.store(true, Ordering::Relaxed);
    }
}

/// Reads up to the end of the head: the head and the bytes after it, or
/// `None` if it is too large or the client stopped before its end.
pub(crate) async fn read_head(
    stream: &mut (impl Stream + ?Sized),
) -> io::Result<Option<(Vec<u8>, Vec<u8>)>> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 2048];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(end + 4);
            return Ok(Some((buf, rest)));
        }
        if buf.len() > MAX_HEAD {
            return Ok(None);
        }
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Headers that belong to this hop, never passed on, besides every
/// `Proxy-*` header and those the client's `Connection` names.
const HOP: [&str; 7] = [
    "host",
    "connection",
    "keep-alive",
    "te",
    "trailer",
    "upgrade",
    "content-length",
];

/// `local`: the port an `OVRA-LOCAL` line named; the request is then in
/// origin-form, for `localhost:<local>`.
fn parse(head: &[u8], local: Option<u16>) -> Result<Request, &'static str> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut parsed = httparse::Request::new(&mut headers);
    match parsed.parse(head) {
        Ok(httparse::Status::Complete(_)) => {}
        _ => return Err("not an HTTP request"),
    }
    let (Some(method), Some(target), Some(version)) = (parsed.method, parsed.path, parsed.version)
    else {
        return Err("not an HTTP request");
    };
    let named: Vec<String> = parsed
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
    let mut body: Option<u64> = None;
    for h in parsed.headers.iter() {
        if h.name.eq_ignore_ascii_case("transfer-encoding") {
            return Err("chunked request bodies are not supported: send Content-Length");
        }
        if h.name.eq_ignore_ascii_case("content-length") {
            let len = std::str::from_utf8(h.value)
                .ok()
                .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or("a bad Content-Length")?;
            if body.is_some_and(|b| b != len) || body.is_some() {
                return Err("more than one Content-Length");
            }
            body = Some(len);
        }
    }
    let body = body.unwrap_or(0);
    let all = |name: &str| -> Vec<Vec<u8>> {
        parsed
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.to_vec())
            .collect()
    };
    let (hosts, origins) = (all("host"), all("origin"));
    let websocket = method == "GET"
        && named.iter().any(|n| n == "upgrade")
        && matches!(all("upgrade").as_slice(), [one] if one.trim_ascii().eq_ignore_ascii_case(b"websocket"));
    let mut headers: Vec<(String, Vec<u8>)> = parsed
        .headers
        .iter()
        .filter(|h| {
            let name = h.name.to_ascii_lowercase();
            !HOP.contains(&name.as_str()) && !name.starts_with("proxy-") && !named.contains(&name)
        })
        .map(|h| (h.name.to_owned(), h.value.to_vec()))
        .collect();
    if body > 0 {
        headers.push(("Content-Length".to_owned(), body.to_string().into_bytes()));
    }
    let request = |connect, host, port, target, headers, body, loopback| Request {
        connect,
        host,
        port,
        method: method.to_owned(),
        target,
        version,
        headers,
        body,
        loopback,
        hosts: hosts.clone(),
        origins: origins.clone(),
        websocket,
    };
    if let Some(port) = local {
        let host = "localhost".to_owned();
        if method == "CONNECT" {
            return Ok(request(
                true,
                host,
                port,
                String::new(),
                Vec::new(),
                0,
                local,
            ));
        }
        if !target.starts_with('/') {
            return Err("after OVRA-LOCAL, send an origin-form request");
        }
        return Ok(request(
            false,
            host,
            port,
            target.to_owned(),
            headers,
            body,
            local,
        ));
    }
    if method == "CONNECT" {
        let (host, port) = crate::url::host_port(target).ok_or("CONNECT needs host:port")?;
        let port = port.ok_or("CONNECT needs host:port")?;
        let loopback = loopback::authority(target).filter(|p| *p == port);
        return Ok(request(
            true,
            host,
            port,
            String::new(),
            Vec::new(),
            0,
            loopback,
        ));
    }
    let lower = target.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Err("send HTTPS through CONNECT");
    }
    if !lower.starts_with("http://") {
        return Err("this is a proxy: send an absolute http:// URL or CONNECT");
    }
    let rest = &target["http://".len()..];
    let cut = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(cut);
    if authority.contains('@') {
        return Err("user names in URLs are not passed on");
    }
    let (host, port) = crate::url::host_port(authority).ok_or("the URL has no valid host")?;
    let path = match path.chars().next() {
        Some('/') => path.to_owned(),
        _ => format!("/{path}"),
    };
    let loopback = loopback::authority(authority);
    Ok(request(
        false,
        host,
        port.unwrap_or(80),
        path,
        headers,
        body,
        loopback,
    ))
}

/// Decides the request and connects: to the server, or through the
/// user's proxy. Returns the connection and any bytes the user's proxy
/// sent after its answer to CONNECT.
async fn open(
    request: &Request,
    shared: &Shared,
    session: &SessionNet,
) -> Result<(Conn, Vec<u8>), Problem> {
    let (host, port) = (request.host.as_str(), request.port);
    let verdict = session
        .rules
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .verdict(host, port);
    let private_ok = match verdict {
        Verdict::Allowed { private_ok } => private_ok,
        Verdict::NotListed => {
            return Err(Problem::Refused(
                "the host is not in the project's allowlist".into(),
            ));
        }
        Verdict::WrongPort => {
            return Err(Problem::Refused(format!(
                "port {port} is not allowed for this host"
            )));
        }
    };
    // This machine is never reached through the user's proxy, which would
    // reach its own loopback instead.
    let local = is_localhost(host) || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    let upstream = shared
        .upstream
        .server(host, request.connect)
        .filter(|_| !local);
    let addrs = match host.parse::<IpAddr>() {
        Ok(ip) => Ok(vec![SocketAddr::new(ip, port)]),
        // Always this machine (RFC 6761). Resolved as `localhost.` it may
        // not be found, and the user's proxy would then reach its own
        // loopback unchecked.
        Err(_) if is_localhost(host) => Ok(vec![
            SocketAddr::from(([127, 0, 0, 1], port)),
            SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port)),
        ]),
        Err(_) => match timeout(DIAL_TIMEOUT, shared.dial.resolve(host.to_owned(), port)).await {
            Ok(Ok(addrs)) => Ok(addrs),
            Ok(Err(e)) => Err(format!("could not resolve the host: {e}")),
            Err(_) => Err("resolving the host timed out".to_owned()),
        },
    };
    // With the user's proxy, the name may not resolve here: that proxy
    // resolves it. What does resolve here is still checked.
    let addrs = match (addrs, upstream) {
        (Ok(addrs), _) => addrs,
        (Err(_), Some(_)) => Vec::new(),
        (Err(why), None) => return Err(Problem::Failed(why)),
    };
    if !private_ok && addrs.iter().any(|a| !is_reachable(a.ip())) {
        return Err(Problem::Refused(
            "the host resolves to a loopback, private or link-local address".into(),
        ));
    }
    match upstream {
        None => Ok((first(&*shared.dial, &addrs).await?, Vec::new())),
        Some(server) => through(request, server, &*shared.dial).await,
    }
}

/// `localhost` and the names under it, as the agent may spell them.
fn is_localhost(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost" || host.ends_with(".localhost")
}

async fn first(dial: &dyn Dial, addrs: &[SocketAddr]) -> Result<Conn, Problem> {
    let mut last = "the host has no address".to_owned();
    for addr in addrs {
        match timeout(DIAL_TIMEOUT, dial.connect(*addr)).await {
            Ok(Ok(conn)) => return Ok(conn),
            Ok(Err(e)) => last = format!("could not connect: {e}"),
            Err(_) => last = "connecting timed out".to_owned(),
        }
    }
    Err(Problem::Failed(last))
}

/// Connects through the user's proxy; for CONNECT, asks it for the tunnel.
async fn through(
    request: &Request,
    server: &Server,
    dial: &dyn Dial,
) -> Result<(Conn, Vec<u8>), Problem> {
    let failed = |why: String| Problem::Failed(format!("the upstream proxy: {why}"));
    let addrs = match timeout(DIAL_TIMEOUT, dial.resolve(server.host.clone(), server.port)).await {
        Ok(Ok(addrs)) => addrs,
        Ok(Err(e)) => return Err(failed(e.to_string())),
        Err(_) => return Err(failed("resolving it timed out".into())),
    };
    let mut conn = first(dial, &addrs)
        .await
        .map_err(|_| failed("could not connect".into()))?;
    if !request.connect {
        return Ok((conn, Vec::new()));
    }
    let name = request.name();
    let mut ask = format!("CONNECT {name} HTTP/1.1\r\nHost: {name}\r\n");
    if let Some(auth) = &server.auth {
        ask.push_str(&format!("Proxy-Authorization: {auth}\r\n"));
    }
    ask.push_str("\r\n");
    let answer = async {
        conn.write_all(ask.as_bytes()).await?;
        read_head(&mut conn).await
    };
    let (head, early) = match timeout(DIAL_TIMEOUT, answer).await {
        Ok(Ok(Some(read))) => read,
        Ok(Ok(None)) => return Err(failed("no answer".into())),
        Ok(Err(e)) => return Err(failed(e.to_string())),
        Err(_) => return Err(failed("no answer in time".into())),
    };
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut response = httparse::Response::new(&mut headers);
    match (response.parse(&head), response.code) {
        (Ok(httparse::Status::Complete(_)), Some(code)) if (200..300).contains(&code) => {
            Ok((conn, early))
        }
        (_, Some(code)) => Err(failed(format!("refused with status {code}"))),
        _ => Err(failed("a malformed answer".into())),
    }
}

pub(crate) async fn respond(
    client: &mut (impl tokio::io::AsyncWrite + Unpin),
    status: u16,
    body: &str,
) -> io::Result<()> {
    let reason = match status {
        400 => "Bad Request",
        403 => "Forbidden",
        _ => "Bad Gateway",
    };
    let body = format!("Ovra netproxy: {body}\n");
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    client.write_all(head.as_bytes()).await?;
    client.write_all(body.as_bytes()).await
}
