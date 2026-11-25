use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use ovra_core::Core;
use ovra_core::protocol::Event;
use ovra_core::protocol::agents::SessionId;
use ovra_core::protocol::netproxy::{AllowedHost, NetproxyEvent};
use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::servers::ServerId;
use ovra_core::services::{BoxFuture, LoopbackGrant, NetReporter, PodConn};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, DuplexStream, duplex};
use tokio::sync::{broadcast, watch};

use crate::dial::{Conn, Dial};
use crate::lock;
use crate::rules::{Rule, Rules};
use crate::serve::{SessionNet, Shared, grants, serve};
use crate::upstream::Upstream;

/// The host's network: every connection is refused and recorded, so a
/// test sees whether a loopback request ever left for a host port.
#[derive(Debug, Default)]
struct HostNet {
    connected: Mutex<Vec<SocketAddr>>,
}

impl Dial for HostNet {
    fn resolve(&self, _: String, _: u16) -> BoxFuture<'_, io::Result<Vec<SocketAddr>>> {
        Box::pin(async { Err(io::ErrorKind::NotFound.into()) })
    }
    fn resolve_as_given(
        &self,
        host: String,
        port: u16,
    ) -> BoxFuture<'_, io::Result<Vec<SocketAddr>>> {
        self.resolve(host, port)
    }
    fn connect(&self, addr: SocketAddr) -> BoxFuture<'_, io::Result<Conn>> {
        lock(&self.connected).push(addr);
        Box::pin(async { Err(io::ErrorKind::ConnectionRefused.into()) })
    }
}

/// What a fake pod's server does with a connection.
#[derive(Debug, Clone, Copy)]
enum Pod {
    /// Keeps the head, the body and anything after it; answers `200 ok`.
    Http,
    /// Keeps the head, answers `101`, then echoes.
    WebSocket,
    /// Keeps the head and anything after it; answers `400`.
    NoUpgrade,
    /// The dial never finishes.
    Hang,
}

async fn head_of(stream: &mut (impl AsyncRead + Unpin)) -> String {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).await.unwrap_or(0) == 0 {
            break;
        }
        buf.push(byte[0]);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

async fn pod(pod: Pod, mut theirs: DuplexStream, heads: Arc<Mutex<Vec<String>>>) {
    let mut head = head_of(&mut theirs).await;
    let answer: &[u8] = match pod {
        Pod::Http => b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
        Pod::NoUpgrade => b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n",
        Pod::Hang => unreachable!(),
        Pod::WebSocket => {
            lock(&heads).push(head);
            let _ = theirs
                .write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n")
                .await;
            let mut buf = [0u8; 256];
            while let Ok(n) = theirs.read(&mut buf).await {
                if n == 0 || theirs.write_all(&buf[..n]).await.is_err() {
                    return;
                }
            }
            return;
        }
    };
    let length = head
        .lines()
        .find_map(|l| l.strip_prefix("Content-Length: "))
        .and_then(|l| l.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    let _ = theirs.read_exact(&mut body).await;
    head.push_str(&String::from_utf8_lossy(&body));
    let _ = theirs.write_all(answer).await;
    let _ = theirs.shutdown().await;
    let mut after = Vec::new();
    let _ = theirs.read_to_end(&mut after).await;
    if !after.is_empty() {
        head.push_str("<after>");
        head.push_str(&String::from_utf8_lossy(&after));
    }
    lock(&heads).push(head);
}

struct Proxy {
    shared: Arc<Shared>,
    net: Arc<SessionNet>,
    host: Arc<HostNet>,
    /// Heads the pods' servers received, and how often a pod was dialled.
    heads: Arc<Mutex<Vec<String>>>,
    dials: Arc<AtomicUsize>,
    events: broadcast::Receiver<Event>,
    _core: Core,
}

fn proxy(base: Vec<Rule>, project: &[&str]) -> Proxy {
    let core = Core::new("test");
    let events = core.subscribe();
    let host = Arc::new(HostNet::default());
    let project = project
        .iter()
        .map(|h| AllowedHost {
            host: (*h).into(),
            port: Some(5173),
        })
        .collect();
    Proxy {
        shared: Arc::new(Shared {
            dial: host.clone(),
            upstream: Upstream::default(),
        }),
        net: Arc::new(SessionNet {
            rules: RwLock::new(Rules {
                base: base.into(),
                project,
            }),
            reporter: NetReporter::new(ProjectId("p1".into()), SessionId("s1".into()), core.sink()),
            bound: None,
            grants: watch::Sender::new(Arc::default()),
        }),
        host,
        heads: Arc::default(),
        dials: Arc::default(),
        events,
        _core: core,
    }
}

impl Proxy {
    fn grant(&self, port: u16, generation: u64, kind: Pod) -> LoopbackGrant {
        let (heads, dials) = (self.heads.clone(), self.dials.clone());
        LoopbackGrant {
            server: ServerId(format!("srv-{port}")),
            port,
            generation,
            dial: Arc::new(move || {
                let heads = heads.clone();
                dials.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    if let Pod::Hang = kind {
                        std::future::pending::<()>().await;
                    }
                    let (ours, theirs) = duplex(4096);
                    tokio::spawn(pod(kind, theirs, heads));
                    Ok(Box::new(ours) as PodConn)
                })
            }),
        }
    }

    fn set(&self, list: Vec<LoopbackGrant>) {
        self.net.grants.send_replace(Arc::new(grants(list)));
    }

    async fn ask(&self, request: &str) -> (String, DuplexStream) {
        let (mut client, theirs) = duplex(4096);
        tokio::spawn(serve(theirs, self.shared.clone(), self.net.clone()));
        client
            .write_all(request.as_bytes())
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let head = head_of(&mut client).await;
        (head, client)
    }

    fn refused(&mut self) -> Vec<String> {
        let mut seen = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            if let Event::Netproxy(NetproxyEvent::Refused { host, reason, .. }) = event {
                seen.push(format!("{host}: {reason}"));
            }
        }
        seen
    }

    /// Waits until the pods' servers kept `n` heads.
    async fn heads(&self, n: usize) -> Vec<String> {
        loop {
            let heads = lock(&self.heads).clone();
            if heads.len() >= n {
                return heads;
            }
            tokio::task::yield_now().await;
        }
    }
}

impl Proxy {
    async fn dialled(&self, n: usize) {
        while self.dials.load(Ordering::SeqCst) < n {
            tokio::task::yield_now().await;
        }
    }
}

/// A proxy with servers granted on 5173 (HTTP) and 8000 (WebSocket).
fn granted() -> Proxy {
    // Even listed, other spellings of this machine get nothing.
    let listed = [
        "localhost",
        "127.0.0.1",
        "::1",
        "0.0.0.0",
        "foo.localhost",
        "127.0.0.2",
    ];
    let p = proxy(Vec::new(), &listed);
    p.set(vec![
        p.grant(5173, 1, Pod::Http),
        p.grant(8000, 1, Pod::WebSocket),
    ]);
    p
}

#[tokio::test]
async fn a_granted_get_reaches_the_pod_as_one_request() {
    let mut p = granted();
    let (head, mut stream) = p
        .ask("GET http://localhost:5173/app?x=1 HTTP/1.1\r\nHost: localhost:5173\r\nOrigin: http://127.0.0.1:8000\r\nConnection: keep-alive\r\nProxy-Authorization: Basic eDp5\r\nAccept: */*\r\n\r\n")
        .await;
    assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
    let mut body = String::new();
    let _ = stream.read_to_string(&mut body).await;
    assert_eq!(body, "ok");
    // Through the forwarder, as the client spelled the host.
    let (head, _) = p
        .ask("OVRA-LOCAL 5173\r\nPOST /api HTTP/1.1\r\nHost: 127.0.0.1:5173\r\nContent-Length: 2\r\n\r\nhi")
        .await;
    assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
    insta::assert_debug_snapshot!(p.heads(2).await, @r#"
    [
        "GET /app?x=1 HTTP/1.1\r\nOrigin: http://127.0.0.1:8000\r\nAccept: */*\r\nHost: localhost:5173\r\nConnection: close\r\n\r\n",
        "POST /api HTTP/1.1\r\nContent-Length: 2\r\nHost: 127.0.0.1:5173\r\nConnection: close\r\n\r\nhi",
    ]
    "#);
    assert!(lock(&p.host.connected).is_empty(), "never a host port");
    assert!(p.refused().is_empty(), "allowed requests are not logged");
}

#[tokio::test]
async fn a_pipelined_second_request_never_reaches_the_pod() {
    let p = granted();
    for request in [
        "POST http://localhost:5173/up HTTP/1.1\r\nHost: localhost:5173\r\nContent-Length: 5\r\n\r\nhelloGET http://localhost:5173/admin HTTP/1.1\r\nHost: localhost:5173\r\nOrigin: null\r\n\r\n",
        "OVRA-LOCAL 5173\r\nGET / HTTP/1.1\r\nHost: localhost:5173\r\n\r\nGET /admin HTTP/1.1\r\nHost: evil.example\r\n\r\n",
    ] {
        let (head, mut stream) = p.ask(request).await;
        assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
        let mut rest = Vec::new();
        let _ = stream.read_to_end(&mut rest).await;
    }
    insta::assert_debug_snapshot!(p.heads(2).await, @r#"
    [
        "POST /up HTTP/1.1\r\nContent-Length: 5\r\nHost: localhost:5173\r\nConnection: close\r\n\r\nhello",
        "GET / HTTP/1.1\r\nHost: localhost:5173\r\nConnection: close\r\n\r\n",
    ]
    "#);
}

#[tokio::test]
async fn a_websocket_upgrade_is_relayed_both_ways() {
    let p = granted();
    let (head, mut stream) = p
        .ask("OVRA-LOCAL 8000\r\nGET /hmr HTTP/1.1\r\nHost: localhost:8000\r\nOrigin: http://localhost:5173\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n")
        .await;
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    for message in ["frame one", "frame two"] {
        stream
            .write_all(message.as_bytes())
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let mut buf = vec![0u8; message.len()];
        stream
            .read_exact(&mut buf)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(buf, message.as_bytes());
    }
    insta::assert_debug_snapshot!(p.heads(1).await, @r#"
    [
        "GET /hmr HTTP/1.1\r\nOrigin: http://localhost:5173\r\nSec-WebSocket-Key: dGhlIHNhbXBsZQ==\r\nSec-WebSocket-Version: 13\r\nHost: localhost:8000\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n",
    ]
    "#);
}

#[tokio::test]
async fn an_upgrade_the_server_refuses_passes_nothing_more() {
    let p = proxy(Vec::new(), &[]);
    p.set(vec![p.grant(5173, 1, Pod::NoUpgrade)]);
    let (head, mut stream) = p
        .ask("GET http://localhost:5173/ws HTTP/1.1\r\nHost: localhost:5173\r\nConnection: upgrade\r\nUpgrade: websocket\r\n\r\nGET /admin HTTP/1.1\r\n\r\n")
        .await;
    assert!(head.starts_with("HTTP/1.1 400"), "{head}");
    let _ = stream.write_all(b"GET /more HTTP/1.1\r\n\r\n").await;
    let mut rest = Vec::new();
    let _ = stream.read_to_end(&mut rest).await;
    // The server keeps its head only once the proxy closed its side.
    let heads = p.heads(1).await;
    assert!(!heads[0].contains("<after>"), "{heads:?}");
    assert!(!heads[0].contains("/admin"), "{heads:?}");
}

#[tokio::test]
async fn every_request_outside_the_rule_is_refused_with_its_reason() {
    let mut p = granted();
    let mut seen = Vec::new();
    for request in [
        // Rule 5: no tunnel to loopback.
        "CONNECT localhost:5173 HTTP/1.1\r\nHost: localhost:5173\r\n\r\n",
        "CONNECT 127.0.0.1:5173 HTTP/1.1\r\n\r\n",
        "OVRA-LOCAL 5173\r\nCONNECT localhost:5173 HTTP/1.1\r\n\r\n",
        // Rule 2: not granted, like the daemon's own port.
        "GET http://127.0.0.1:7777/ HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n",
        "GET http://localhost/ HTTP/1.1\r\nHost: localhost\r\n\r\n",
        "OVRA-LOCAL 7777\r\nGET / HTTP/1.1\r\nHost: localhost:7777\r\n\r\n",
        // Rule 6: Host.
        "GET http://localhost:5173/ HTTP/1.1\r\n\r\n",
        "GET http://localhost:5173/ HTTP/1.1\r\nHost: evil.example\r\n\r\n",
        "GET http://localhost:5173/ HTTP/1.1\r\nHost: localhost:8000\r\n\r\n",
        "GET http://localhost:5173/ HTTP/1.1\r\nHost: [::1]:5173\r\n\r\n",
        "GET http://localhost:5173/ HTTP/1.1\r\nHost: localhost:5173\r\nHost: evil.example\r\n\r\n",
        "OVRA-LOCAL 5173\r\nGET / HTTP/1.1\r\nHost: rebind.example:5173\r\n\r\n",
        // Rule 6: Origin.
        "GET http://localhost:5173/ HTTP/1.1\r\nHost: localhost:5173\r\nOrigin: null\r\n\r\n",
        "GET http://localhost:5173/ HTTP/1.1\r\nHost: localhost:5173\r\nOrigin: http://evil.example\r\n\r\n",
        "GET http://localhost:5173/ HTTP/1.1\r\nHost: localhost:5173\r\nOrigin: http://localhost:7777\r\n\r\n",
        "GET http://localhost:5173/ HTTP/1.1\r\nHost: localhost:5173\r\nOrigin: https://localhost:5173\r\n\r\n",
        "GET http://localhost:5173/ HTTP/1.1\r\nHost: localhost:5173\r\nOrigin: http://localhost:5173/\r\n\r\n",
        "GET http://localhost:5173/ HTTP/1.1\r\nHost: localhost:5173\r\nOrigin: http://localhost:5173\r\nOrigin: null\r\n\r\n",
        "OVRA-LOCAL 8000\r\nGET / HTTP/1.1\r\nHost: localhost:8000\r\nOrigin: http://evil.example\r\nConnection: upgrade\r\nUpgrade: websocket\r\n\r\n",
        // Rule 1: every other spelling of this machine stays refused.
        "GET http://[::1]:5173/ HTTP/1.1\r\nHost: [::1]:5173\r\n\r\n",
        "GET http://0.0.0.0:5173/ HTTP/1.1\r\nHost: 0.0.0.0:5173\r\n\r\n",
        "GET http://foo.localhost:5173/ HTTP/1.1\r\nHost: foo.localhost:5173\r\n\r\n",
        "GET http://127.0.0.2:5173/ HTTP/1.1\r\nHost: 127.0.0.2:5173\r\n\r\n",
        "GET http://localhost.:5173/ HTTP/1.1\r\nHost: localhost.:5173\r\n\r\n",
        "CONNECT [::1]:5173 HTTP/1.1\r\n\r\n",
        // Not requests at all.
        "OVRA-LOCAL x\r\nGET / HTTP/1.1\r\nHost: localhost:5173\r\n\r\n",
        "OVRA-LOCAL 5173\r\nGET http://localhost:5173/ HTTP/1.1\r\nHost: localhost:5173\r\n\r\n",
    ] {
        let (head, _) = p.ask(request).await;
        let line = request.trim_end().replace("\r\n", " | ");
        let status = head.lines().next().unwrap_or_default().to_owned();
        let refused = p.refused().join("; ");
        seen.push(format!("{line} => {status} {refused}"));
    }
    insta::assert_snapshot!(seen.join("\n"));
    assert_eq!(p.dials.load(Ordering::SeqCst), 0, "no pod was dialled");
    assert!(lock(&p.host.connected).is_empty(), "nor a host port");
}

#[tokio::test]
async fn a_grant_revoked_or_replaced_while_the_dial_waits_is_stale() {
    let mut p = proxy(Vec::new(), &[]);
    let request = "GET http://localhost:5173/ HTTP/1.1\r\nHost: localhost:5173\r\n\r\n";
    for (n, next) in [(1, None), (2, Some(2))] {
        p.set(vec![p.grant(5173, 1, Pod::Hang)]);
        let asked = p.ask(request);
        let change = async {
            p.dialled(n).await;
            // Revoked, then a restart with a new generation.
            p.set(
                next.map(|g| p.grant(5173, g, Pod::Http))
                    .into_iter()
                    .collect(),
            );
        };
        let ((head, _), ()) = tokio::join!(asked, change);
        assert!(head.starts_with("HTTP/1.1 403"), "{head}");
    }
    assert_eq!(
        p.refused(),
        ["localhost:5173: stale grant", "localhost:5173: stale grant"]
    );
}

#[tokio::test]
async fn revoking_a_grant_ends_its_open_websocket() {
    let p = granted();
    let (head, mut stream) = p
        .ask("GET http://localhost:8000/hmr HTTP/1.1\r\nHost: localhost:8000\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n")
        .await;
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    // Another server's grant changing leaves it open.
    p.set(vec![
        p.grant(5173, 2, Pod::Http),
        p.grant(8000, 1, Pod::WebSocket),
    ]);
    let _ = stream.write_all(b"ping").await;
    let mut buf = [0u8; 4];
    let _ = stream.read_exact(&mut buf).await;
    assert_eq!(&buf, b"ping");
    p.set(vec![p.grant(5173, 2, Pod::Http)]);
    let mut rest = Vec::new();
    let _ = stream.read_to_end(&mut rest).await;
    assert!(rest.is_empty(), "{rest:?}");
}

#[tokio::test]
async fn a_configured_gateway_on_localhost_keeps_its_own_path() {
    let gateway = Rule::configured(
        crate::url::Authority::parse("http://localhost:4000").unwrap_or_else(|| panic!("url")),
    );
    let mut p = proxy(vec![gateway], &[]);
    p.set(vec![p.grant(4000, 1, Pod::Http)]);
    let (head, _) = p.ask("CONNECT localhost:4000 HTTP/1.1\r\n\r\n").await;
    // Today's path: the host's own port, refused by the fake host here.
    assert!(head.starts_with("HTTP/1.1 502"), "{head}");
    assert_eq!(
        lock(&p.host.connected).first().map(ToString::to_string),
        Some("127.0.0.1:4000".to_owned())
    );
    // The forwarder's connections are always the rule's.
    let (head, _) = p
        .ask("OVRA-LOCAL 4000\r\nGET / HTTP/1.1\r\nHost: localhost:4000\r\n\r\n")
        .await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(p.refused().is_empty());
}

#[test]
fn only_exact_loopback_authorities_count() {
    let ports: Vec<Option<u16>> = [
        "localhost:5173",
        "LocalHost:5173",
        "127.0.0.1:80",
        "localhost",
        "localhost:",
        "localhost:+1",
        "localhost:0",
        "localhost:65536",
        "localhost:5173:1",
        "localhost.:5173",
        "127.1:5173",
        "[::1]:5173",
    ]
    .iter()
    .map(|a| super::authority(a))
    .collect();
    assert_eq!(
        ports,
        [
            Some(5173),
            Some(5173),
            Some(80),
            Some(80),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None
        ]
    );
}

#[tokio::test]
async fn a_secret_bound_to_localhost_is_never_added() {
    use ovra_core::protocol::secrets::{SecretInfo, SecretValue};
    use ovra_core::services::Secrets;

    let mut p = granted();
    let project = ProjectId("p1".into());
    let store = Arc::new(ovra_core::fakes::MemorySecrets::default());
    for host in ["localhost:5173", "localhost:443"] {
        let info = SecretInfo {
            name: host.into(),
            host: host.into(),
            header: "Authorization".into(),
            prefix: String::new(),
        };
        store
            .set(&project, info, SecretValue::new("dev-value"))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let bound = crate::inject::Bound {
        ca: crate::ca::SessionCa::new(&["localhost".to_owned()]).unwrap_or_else(|e| panic!("{e}")),
        secrets: store.list(&project).unwrap_or_default(),
        project,
        store,
        client: crate::tls::Roots::of(Vec::new())
            .client()
            .unwrap_or_else(|e| panic!("{e}")),
    };
    p.net = Arc::new(SessionNet {
        rules: RwLock::new(Rules {
            base: Arc::new([]),
            project: Vec::new(),
        }),
        reporter: p.net.reporter.clone(),
        bound: Some(bound),
        grants: watch::Sender::new(Arc::default()),
    });
    p.set(vec![
        p.grant(5173, 1, Pod::Http),
        p.grant(443, 1, Pod::Http),
    ]);
    for port in [5173, 443] {
        let (head, _) = p
            .ask(&format!("CONNECT localhost:{port} HTTP/1.1\r\n\r\n"))
            .await;
        assert!(head.starts_with("HTTP/1.1 403"), "{head}");
    }
    let (head, _) = p
        .ask("GET http://localhost:5173/ HTTP/1.1\r\nHost: localhost:5173\r\n\r\n")
        .await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    let heads = p.heads(1).await;
    assert!(!heads[0].contains("dev-value"), "{heads:?}");
    assert_eq!(
        p.refused(),
        [
            "localhost:5173: connect to loopback",
            "localhost:443: connect to loopback"
        ]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn grants_reach_a_running_proxy_through_its_socket() {
    use ovra_core::protocol::netproxy::Allowlist;
    use ovra_core::services::NetProxy;
    use tokio::net::UnixStream;

    let proxies = crate::NetProxies::new(
        &|_| None,
        Arc::new(HostNet::default()),
        Arc::new(ovra_core::fakes::MemorySecrets::default()),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let p = proxy(Vec::new(), &[]);
    let session = SessionId("s1".into());
    let reporter = NetReporter::new(ProjectId("p1".into()), session.clone(), p._core.sink());
    let address = proxies
        .start(session.clone(), Allowlist::default(), reporter, Vec::new())
        .await
        .unwrap_or_else(|e| panic!("{e}"))
        .address;
    let ask = || async {
        let mut stream = UnixStream::connect(&address)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let request = "OVRA-LOCAL 5173\r\nGET / HTTP/1.1\r\nHost: localhost:5173\r\n\r\n";
        let _ = stream.write_all(request.as_bytes()).await;
        head_of(&mut stream).await
    };
    // Another session's grant does nothing here.
    proxies.grant(&SessionId("s2".into()), vec![p.grant(5173, 1, Pod::Http)]);
    assert!(ask().await.starts_with("HTTP/1.1 403"));
    proxies.grant(&session, vec![p.grant(5173, 1, Pod::Http)]);
    assert!(ask().await.starts_with("HTTP/1.1 200"));
    proxies.grant(&session, Vec::new());
    assert!(ask().await.starts_with("HTTP/1.1 403"));
    assert_eq!(p.dials.load(Ordering::SeqCst), 1);
}
