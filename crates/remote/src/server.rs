//! The listener: one port that speaks TLS for everything, and plain HTTP
//! only to hand out Ovra's CA certificate while a pairing window is open.

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex, PoisonError};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures_util::stream::FuturesUnordered;
use futures_util::{SinkExt, StreamExt};
use ovra_core::Core;
use ovra_core::protocol::info::InfoEvent;
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::time::Sleep;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::server::TlsStream;
use tokio_util::either::Either;
use tokio_util::sync::CancellationToken;

use crate::{Shared, auth};

/// A WebSocket lasts this long; then the phone signs a new challenge.
const SESSION_LIFE: Duration = Duration::from_secs(15 * 60);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FRAME: usize = 1 << 20;
const PAIRS_PER_MINUTE: u32 = 10;
const HANDSHAKES_PER_MINUTE: u32 = 30;
/// Connections open at once, in all and from one address.
const MAX_CONNECTIONS: usize = 64;
const MAX_PER_ADDRESS: usize = 8;
/// A connection that neither reads nor writes for this long is closed;
/// sessions ping more often.
const IDLE: Duration = Duration::from_secs(90);
const PING_EVERY: Duration = Duration::from_secs(30);
/// Requests one phone may have running at once.
const MAX_IN_FLIGHT: usize = 32;
/// The app's CSP. `connect-src` names the WebSocket's `wss://` origin as
/// well, since older Safari does not count it as `'self'`.
fn csp(origin: &str) -> String {
    let wss = origin.replacen("https://", "wss://", 1);
    format!(
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data:; connect-src 'self' {wss}; frame-ancestors 'none'; \
         base-uri 'none'; form-action 'none'"
    )
}

/// One file of the mobile app.
#[derive(Debug, Clone)]
pub struct Asset {
    bytes: Vec<u8>,
    mime: &'static str,
}

impl Asset {
    /// The file `path` holds `bytes`; its type comes from its extension.
    pub fn new(path: &str, bytes: Vec<u8>) -> Self {
        let mime = match path.rsplit('.').next().unwrap_or("") {
            "html" => "text/html; charset=utf-8",
            "js" | "mjs" => "text/javascript",
            "css" => "text/css",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "json" | "webmanifest" => "application/manifest+json",
            "woff2" => "font/woff2",
            _ => "application/octet-stream",
        };
        Self { bytes, mime }
    }
}

/// Requests per address in the current minute.
#[derive(Debug, Default)]
pub(crate) struct Limits {
    by_ip: HashMap<IpAddr, (Instant, u32, u32)>,
}

impl Limits {
    /// Counts one try; `false` when the address is over its limit.
    fn take(&mut self, ip: IpAddr, pairing: bool) -> bool {
        let now = Instant::now();
        self.by_ip
            .retain(|_, (since, ..)| now.duration_since(*since) < Duration::from_secs(60));
        let (_, pairs, handshakes) = self.by_ip.entry(ip).or_insert((now, 0, 0));
        let (count, max) = if pairing {
            (pairs, PAIRS_PER_MINUTE)
        } else {
            (handshakes, HANDSHAKES_PER_MINUTE)
        };
        *count += 1;
        *count <= max
    }
}

#[derive(Debug, Clone, Copy)]
struct Peer {
    addr: SocketAddr,
    tls: bool,
}

type Inner = Either<TlsStream<TcpStream>, TcpStream>;

/// A connection's slots: one of [`MAX_CONNECTIONS`] and one of its
/// address's [`MAX_PER_ADDRESS`], given back when it closes.
struct Slots {
    _all: OwnedSemaphorePermit,
    ip: IpAddr,
    by_ip: Arc<StdMutex<HashMap<IpAddr, usize>>>,
}

impl Slots {
    fn take(
        all: &Arc<Semaphore>,
        by_ip: &Arc<StdMutex<HashMap<IpAddr, usize>>>,
        ip: IpAddr,
    ) -> Option<Self> {
        let permit = all.clone().try_acquire_owned().ok()?;
        let mut counts = by_ip.lock().unwrap_or_else(PoisonError::into_inner);
        let count = counts.entry(ip).or_insert(0);
        if *count >= MAX_PER_ADDRESS {
            return None;
        }
        *count += 1;
        Some(Self {
            _all: permit,
            ip,
            by_ip: by_ip.clone(),
        })
    }
}

impl Drop for Slots {
    fn drop(&mut self) {
        let mut counts = self.by_ip.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = counts.get_mut(&self.ip) {
            *count -= 1;
            if *count == 0 {
                counts.remove(&self.ip);
            }
        }
    }
}

/// A connection that holds its slots and fails once idle for [`IDLE`].
struct Io {
    inner: Inner,
    idle: Pin<Box<Sleep>>,
    _slots: Slots,
}

impl Io {
    fn new(inner: Inner, slots: Slots) -> Self {
        Self {
            inner,
            idle: Box::pin(tokio::time::sleep(IDLE)),
            _slots: slots,
        }
    }

    fn active(&mut self) {
        self.idle.as_mut().reset(tokio::time::Instant::now() + IDLE);
    }

    fn timed_out<T>(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<T>> {
        match self.idle.as_mut().poll(cx) {
            Poll::Ready(()) => Poll::Ready(Err(std::io::ErrorKind::TimedOut.into())),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncRead for Io {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(r) => {
                self.active();
                Poll::Ready(r)
            }
            Poll::Pending => self.timed_out(cx),
        }
    }
}

impl AsyncWrite for Io {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_write(cx, buf) {
            Poll::Ready(r) => {
                self.active();
                Poll::Ready(r)
            }
            Poll::Pending => self.timed_out(cx),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

struct PhoneListener {
    rx: mpsc::Receiver<(Io, Peer)>,
    local: SocketAddr,
}

impl axum::serve::Listener for PhoneListener {
    type Io = Io;
    type Addr = Peer;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.rx.recv().await {
            Some(next) => next,
            // The accept loop stopped: wait for the shutdown that follows.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(Peer {
            addr: self.local,
            tls: true,
        })
    }
}

impl axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, PhoneListener>>
    for Peer
{
    fn connect_info(stream: axum::serve::IncomingStream<'_, PhoneListener>) -> Self {
        *stream.remote_addr()
    }
}

pub(crate) fn spawn(
    shared: Arc<Shared>,
    listener: std::net::TcpListener,
    tls: Arc<ServerConfig>,
    stop: CancellationToken,
) {
    tokio::spawn(async move {
        let Ok(local) = listener.local_addr() else {
            return;
        };
        let listener = match listener
            .set_nonblocking(true)
            .and_then(|()| TcpListener::from_std(listener))
        {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(error = %e, "phone access cannot listen");
                return;
            }
        };
        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(accept_loop(
            listener,
            TlsAcceptor::from(tls),
            tx,
            stop.clone(),
        ));
        let app = router(shared);
        let served = axum::serve(
            PhoneListener { rx, local },
            app.into_make_service_with_connect_info::<Peer>(),
        )
        .with_graceful_shutdown(stop.cancelled_owned())
        .await;
        if let Err(e) = served {
            tracing::warn!(error = %e, "phone access stopped");
        }
    });
}

/// Accepts connections and sorts them by their first byte: a TLS
/// handshake (`0x16`) or plain HTTP. Handshakes run on their own, so a
/// slow one holds up nobody.
async fn accept_loop(
    listener: TcpListener,
    tls: TlsAcceptor,
    tx: mpsc::Sender<(Io, Peer)>,
    stop: CancellationToken,
) {
    let all = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let by_ip = Arc::new(StdMutex::new(HashMap::new()));
    loop {
        let (stream, addr) = tokio::select! {
            () = stop.cancelled() => return,
            accepted = listener.accept() => match accepted {
                Ok(a) => a,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            },
        };
        // Over the limit: closed at once, before any work.
        let Some(slots) = Slots::take(&all, &by_ip, addr.ip()) else {
            continue;
        };
        let (tls, tx) = (tls.clone(), tx.clone());
        tokio::spawn(async move {
            let mut first = [0u8; 1];
            let peeked = tokio::time::timeout(HANDSHAKE_TIMEOUT, stream.peek(&mut first)).await;
            if !matches!(peeked, Ok(Ok(1))) {
                return;
            }
            let next = if first[0] == 0x16 {
                match tokio::time::timeout(HANDSHAKE_TIMEOUT, tls.accept(stream)).await {
                    Ok(Ok(stream)) => (
                        Io::new(Either::Left(stream), slots),
                        Peer { addr, tls: true },
                    ),
                    _ => return,
                }
            } else {
                (
                    Io::new(Either::Right(stream), slots),
                    Peer { addr, tls: false },
                )
            };
            let _ = tx.send(next).await;
        });
    }
}

fn router(shared: Arc<Shared>) -> Router {
    Router::new()
        .route("/ws", get(ws))
        .route("/api/pair", post(pair))
        .fallback(get(asset))
        .layer(middleware::from_fn_with_state(shared.clone(), guard))
        .with_state(shared)
}

/// Plain HTTP gets only the CA certificate, and only during pairing;
/// everything else goes to HTTPS. HTTPS requests must name this host, so a
/// page elsewhere cannot rebind a name of its own to this address.
async fn guard(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    request: Request,
    next: Next,
) -> Response {
    let (origin, host, pairing, ca) = {
        let state = shared.lock();
        let Some(running) = &state.running else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let pairing = state
            .pairing
            .as_ref()
            .is_some_and(|w| w.until > Instant::now());
        (
            running.origin(),
            running.host(),
            pairing,
            running.ca.clone(),
        )
    };
    if !peer.tls {
        if request.uri().path() == "/ca.crt" && pairing {
            return (
                [
                    (header::CONTENT_TYPE, "application/x-x509-ca-cert"),
                    (header::CACHE_CONTROL, "no-store"),
                ],
                ca.der().to_vec(),
            )
                .into_response();
        }
        let to = format!("{origin}{}", request.uri().path());
        return (StatusCode::PERMANENT_REDIRECT, [(header::LOCATION, to)]).into_response();
    }
    let named = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h == host);
    if !named {
        return StatusCode::MISDIRECTED_REQUEST.into_response();
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    // The origin is an IP address and port, always a valid header value.
    if let Ok(csp) = HeaderValue::from_str(&csp(&origin)) {
        headers.insert(header::CONTENT_SECURITY_POLICY, csp);
    }
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=31536000"),
    );
    response
}

async fn asset(State(shared): State<Arc<Shared>>, request: Request) -> Response {
    let path = request.uri().path();
    let file = if path == "/" { "/mobile.html" } else { path };
    let found = (shared.assets)(file).or_else(|| {
        // Client-side routes load the app.
        (!file.rsplit('/').next().unwrap_or("").contains('.'))
            .then(|| (shared.assets)("/mobile.html"))
            .flatten()
    });
    match found {
        Some(asset) => (
            [
                (header::CONTENT_TYPE, asset.mime),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            Body::from(asset.bytes),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct PairBody {
    secret: String,
    key: String,
    name: String,
}

/// A device name for lists: one line, printable, at most 40 characters.
fn clean_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).take(40).collect();
    let name = name.trim();
    if name.is_empty() {
        "Phone".into()
    } else {
        name.to_owned()
    }
}

async fn pair(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    axum::Json(body): axum::Json<PairBody>,
) -> Response {
    if !shared.lock().limits.take(peer.addr.ip(), true) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    if !auth::key_ok(&body.key) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match shared
        .pair(&body.secret, &body.key, &clean_name(&body.name))
        .await
    {
        Some(Some(device)) => {
            axum::Json(serde_json::json!({"device": device.id, "name": device.name}))
                .into_response()
        }
        Some(None) => (
            StatusCode::FORBIDDEN,
            "the computer did not allow this phone",
        )
            .into_response(),
        None => (
            StatusCode::FORBIDDEN,
            "this pairing code does not work any more",
        )
            .into_response(),
    }
}

async fn ws(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let (origin, core, stop) = {
        let mut state = shared.lock();
        if !state.limits.take(peer.addr.ip(), false) {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
        let Some(running) = &state.running else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        (running.origin(), running.core.clone(), running.stop.clone())
    };
    let same_origin = headers
        .get(header::ORIGIN)
        .and_then(|o| o.to_str().ok())
        .is_some_and(|o| o == origin);
    if !same_origin {
        return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
    }
    upgrade
        .max_message_size(MAX_FRAME)
        .on_upgrade(move |socket| session(socket, shared, core, origin, stop))
}

#[derive(Debug, Deserialize)]
struct Proof {
    device: String,
    signature: String,
}

async fn session(
    mut socket: WebSocket,
    shared: Arc<Shared>,
    core: Core,
    origin: String,
    stop: CancellationToken,
) {
    let challenge = auth::random_token();
    let hello = serde_json::json!({"challenge": challenge}).to_string();
    if socket.send(Message::Text(hello.into())).await.is_err() {
        return;
    }
    let proof = match tokio::time::timeout(HANDSHAKE_TIMEOUT, socket.recv()).await {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<Proof>(&text).ok(),
        _ => None,
    };
    // Subscribed before the device is looked up, so a forget from now on
    // reaches this session.
    let mut kicks = shared.kicks.subscribe();
    let device = proof.and_then(|proof| {
        let mut state = shared.lock();
        state.devices.expire(crate::now_ms());
        let stored = state.devices.get(&proof.device)?;
        auth::verify(
            &stored.key,
            &auth::message(&origin, &challenge),
            &proof.signature,
        )
        .then(|| stored.clone())
    });
    let Some(device) = device else {
        let refused = serde_json::json!({"error": "not_paired"}).to_string();
        let _ = socket.send(Message::Text(refused.into())).await;
        return;
    };
    let welcome = serde_json::json!({"paired": device.name}).to_string();
    if socket.send(Message::Text(welcome.into())).await.is_err() {
        return;
    }
    shared.connected(&device.id, true);
    serve(socket, &core, &device.id, &device.name, &mut kicks, &stop).await;
    shared.connected(&device.id, false);
}

/// Frames as in `ovra-transport`, through a phone client, until the phone
/// leaves, is forgotten, phone access stops or the session is too old.
async fn serve(
    socket: WebSocket,
    core: &Core,
    id: &str,
    name: &str,
    kicks: &mut tokio::sync::broadcast::Receiver<String>,
    stop: &CancellationToken,
) {
    let (mut tx, mut rx) = socket.split();
    let mut events = core.subscribe();
    // The name kept at pairing, never one the phone sends.
    let client = core.connect_phone(name);
    let ready = ovra_transport::event_frame(InfoEvent::Ready.into());
    if tx.send(Message::Text(ready.into())).await.is_err() {
        return;
    }
    let client = &client;
    let mut running = FuturesUnordered::new();
    let expires = tokio::time::sleep(SESSION_LIFE);
    tokio::pin!(expires);
    let mut ping = tokio::time::interval(PING_EVERY);
    loop {
        tokio::select! {
            _ = ping.tick() => {
                if tx.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            },
            incoming = rx.next(), if running.len() < MAX_IN_FLIGHT => match incoming {
                Some(Ok(Message::Text(text))) => {
                    running.push(async move { ovra_transport::dispatch(client, &text).await });
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            },
            Some(reply) = running.next(), if !running.is_empty() => {
                if tx.send(Message::Text(reply.into())).await.is_err() {
                    break;
                }
            },
            event = Core::next_event(&mut events) => match event {
                Some(event) if client.shows(&event) => {
                    if tx.send(Message::Text(ovra_transport::event_frame(event).into())).await.is_err() {
                        break;
                    }
                }
                Some(_) => {}
                None => break,
            },
            kicked = kicks.recv() => match kicked {
                Ok(who) if who == id || who == "*" => break,
                Ok(_) => {}
                // Missed kicks may have been for this phone.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => break,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            () = stop.cancelled() => break,
            () = &mut expires => break,
        }
    }
    let _ = tx.send(Message::Close(None)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_csp_lets_the_app_reach_its_own_socket_only() {
        let csp = csp("https://192.168.1.20:52044");
        assert!(
            csp.contains("connect-src 'self' wss://192.168.1.20:52044;"),
            "{csp}"
        );
        assert!(csp.contains("default-src 'self';"));
    }

    #[test]
    fn app_files_get_their_types() {
        let mime = |path: &str| Asset::new(path, Vec::new()).mime;
        assert_eq!(mime("sw.js"), "text/javascript");
        assert_eq!(mime("mobile.webmanifest"), "application/manifest+json");
        assert_eq!(mime("phone-icon-256.png"), "image/png");
        assert_eq!(mime("mobile.html"), "text/html; charset=utf-8");
    }

    #[test]
    fn names_are_one_short_printable_line() {
        assert_eq!(clean_name("  "), "Phone");
        assert_eq!(clean_name("Ana's\niPhone\u{7}"), "Ana'siPhone");
        assert_eq!(clean_name(&"x".repeat(100)).len(), 40);
    }

    #[test]
    fn each_address_gets_a_few_tries_a_minute() {
        let mut limits = Limits::default();
        let ip: IpAddr = "192.168.1.5".parse().expect("ip");
        for _ in 0..PAIRS_PER_MINUTE {
            assert!(limits.take(ip, true));
        }
        assert!(!limits.take(ip, true));
        assert!(limits.take(ip, false), "handshakes count apart");
        assert!(limits.take("192.168.1.6".parse().expect("ip"), true));
    }

    #[test]
    fn one_address_cannot_take_every_connection() {
        let all = Arc::new(Semaphore::new(MAX_CONNECTIONS));
        let by_ip = Arc::new(StdMutex::new(HashMap::new()));
        let ip: IpAddr = "192.168.1.5".parse().expect("ip");
        let held: Vec<_> = (0..MAX_PER_ADDRESS)
            .map(|_| Slots::take(&all, &by_ip, ip).expect("slot"))
            .collect();
        assert!(Slots::take(&all, &by_ip, ip).is_none());
        assert_eq!(all.available_permits(), MAX_CONNECTIONS - MAX_PER_ADDRESS);
        assert!(Slots::take(&all, &by_ip, "192.168.1.6".parse().expect("ip")).is_some());
        drop(held);
        assert!(Slots::take(&all, &by_ip, ip).is_some());
        assert!(by_ip.lock().expect("lock").get(&ip).is_none_or(|n| *n == 0));
    }
}
