use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, RwLock};

use ovra_core::Core;
use ovra_core::protocol::Event;
use ovra_core::protocol::agents::SessionId;
use ovra_core::protocol::netproxy::{AllowedHost, Allowlist, NetproxyEvent};
use ovra_core::protocol::projects::ProjectId;
use ovra_core::services::{BoxFuture, NetProxy, NetReporter};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex};
use tokio::sync::broadcast;

use crate::dial::{Conn, Dial};
use crate::rules::{Rule, Rules};
use crate::serve::{SessionNet, Shared, serve};
use crate::upstream::Upstream;
use crate::{NetProxies, lock};

/// What a fake server at an address does with a connection.
#[derive(Debug, Clone, Copy)]
enum Server {
    /// Sends back what it receives.
    Echo,
    /// Reads one request head, keeps it, answers `200 ok` and closes.
    Http,
    /// A user's proxy: keeps the CONNECT head, answers 200, then echoes.
    Upstream,
    /// A user's proxy that answers every request with 407.
    Refuse,
}

#[derive(Debug, Default)]
struct FakeNet {
    names: BTreeMap<String, Vec<IpAddr>>,
    servers: BTreeMap<SocketAddr, Server>,
    /// Every address connected to, in order.
    connected: Mutex<Vec<SocketAddr>>,
    /// Request heads the `Http` and `Upstream` servers received.
    heads: Arc<Mutex<Vec<String>>>,
}

impl FakeNet {
    fn name(mut self, host: &str, ips: &[&str]) -> Self {
        let ips = ips
            .iter()
            .map(|ip| ip.parse().unwrap_or_else(|e| panic!("{e}")));
        self.names.insert(host.into(), ips.collect());
        self
    }

    fn server(mut self, addr: &str, server: Server) -> Self {
        let addr = addr.parse().unwrap_or_else(|e| panic!("{e}"));
        self.servers.insert(addr, server);
        self
    }
}

async fn head_of(stream: &mut DuplexStream) -> String {
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

async fn echo(mut stream: DuplexStream) {
    let mut buf = [0u8; 1024];
    while let Ok(n) = stream.read(&mut buf).await {
        if n == 0 || stream.write_all(&buf[..n]).await.is_err() {
            return;
        }
    }
}

impl Dial for FakeNet {
    fn resolve(&self, host: String, port: u16) -> BoxFuture<'_, io::Result<Vec<SocketAddr>>> {
        Box::pin(async move {
            match self.names.get(&host) {
                Some(ips) => Ok(ips.iter().map(|ip| SocketAddr::new(*ip, port)).collect()),
                None => Err(io::Error::new(io::ErrorKind::NotFound, "no such host")),
            }
        })
    }

    fn connect(&self, addr: SocketAddr) -> BoxFuture<'_, io::Result<Conn>> {
        Box::pin(async move {
            lock(&self.connected).push(addr);
            let server = *self
                .servers
                .get(&addr)
                .ok_or(io::ErrorKind::ConnectionRefused)?;
            let (ours, mut theirs) = duplex(4096);
            let heads = self.heads.clone();
            tokio::spawn(async move {
                match server {
                    Server::Echo => echo(theirs).await,
                    Server::Http => {
                        let mut head = head_of(&mut theirs).await;
                        let length = head
                            .lines()
                            .find_map(|l| l.strip_prefix("Content-Length: "))
                            .and_then(|l| l.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        let mut body = vec![0u8; length];
                        let _ = theirs.read_exact(&mut body).await;
                        head.push_str(&String::from_utf8_lossy(&body));
                        let _ = theirs
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                            .await;
                        let _ = theirs.shutdown().await;
                        // Whatever else arrives would be a second request.
                        let mut after = Vec::new();
                        let _ = theirs.read_to_end(&mut after).await;
                        if !after.is_empty() {
                            head.push_str("<after>");
                            head.push_str(&String::from_utf8_lossy(&after));
                        }
                        lock(&heads).push(head);
                    }
                    Server::Refuse => {
                        let _ = head_of(&mut theirs).await;
                        let _ = theirs
                            .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                            .await;
                    }
                    Server::Upstream => {
                        let head = head_of(&mut theirs).await;
                        lock(&heads).push(head);
                        let _ = theirs.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await;
                        echo(theirs).await;
                    }
                }
            });
            Ok(Box::new(ours) as Conn)
        })
    }
}

struct Proxy {
    shared: Arc<Shared>,
    net: Arc<SessionNet>,
    dial: Arc<FakeNet>,
    events: broadcast::Receiver<Event>,
    _core: Core,
}

fn rules(project: &[(&str, Option<u16>)]) -> Rules {
    Rules {
        base: vec![Rule::named("api.anthropic.com")].into(),
        project: project
            .iter()
            .map(|(host, port)| AllowedHost {
                host: (*host).into(),
                port: *port,
            })
            .collect(),
    }
}

fn proxy(fake: FakeNet, rules: Rules, upstream: Upstream) -> Proxy {
    let core = Core::new("test");
    let events = core.subscribe();
    let dial = Arc::new(fake);
    Proxy {
        shared: Arc::new(Shared {
            dial: dial.clone(),
            upstream,
        }),
        net: Arc::new(SessionNet {
            rules: RwLock::new(rules),
            reporter: NetReporter::new(ProjectId("p1".into()), SessionId("s1".into()), core.sink()),
        }),
        dial,
        events,
        _core: core,
    }
}

impl Proxy {
    /// Sends `request` and returns the proxy's answer head and the stream.
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

    fn refused(&mut self) -> Vec<(String, String)> {
        let mut seen = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            if let Event::Netproxy(NetproxyEvent::Refused { host, reason, .. }) = event {
                seen.push((host, reason));
            }
        }
        seen
    }
}

async fn roundtrip(stream: &mut DuplexStream) -> String {
    stream
        .write_all(b"hello")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut buf = [0u8; 5];
    stream
        .read_exact(&mut buf)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    String::from_utf8_lossy(&buf).into_owned()
}

fn web() -> FakeNet {
    FakeNet::default()
        .name("crates.io", &["93.184.216.34"])
        .name("api.anthropic.com", &["160.79.104.10"])
        .name("rebound.example", &["8.8.8.8", "169.254.169.254"])
        .name("inside.example", &["10.0.0.5"])
        .server("93.184.216.34:443", Server::Echo)
        .server("93.184.216.34:8443", Server::Echo)
        .server("93.184.216.34:80", Server::Http)
        .server("160.79.104.10:443", Server::Echo)
        .name("proxy.corp", &["10.1.1.1"])
        .server("10.1.1.1:3128", Server::Upstream)
        .server("10.1.1.1:3129", Server::Http)
}

/// Waits until the fake servers kept `n` heads.
async fn heads(p: &Proxy, n: usize) -> Vec<String> {
    loop {
        let heads = lock(&p.dial.heads).clone();
        if heads.len() >= n {
            return heads;
        }
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn an_allowed_host_gets_a_tunnel() {
    let p = proxy(web(), rules(&[("crates.io", None)]), Upstream::default());
    let (head, mut stream) = p
        .ask("CONNECT crates.io:443 HTTP/1.1\r\nHost: crates.io:443\r\n\r\n")
        .await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(roundtrip(&mut stream).await, "hello");
    // The base list needs no project entry.
    let (head, _) = p
        .ask("CONNECT api.anthropic.com:443 HTTP/1.1\r\n\r\n")
        .await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
}

#[tokio::test]
async fn hosts_and_ports_outside_the_allowlist_are_refused_and_reported() {
    let mut p = proxy(
        web(),
        rules(&[("crates.io", None), ("static.crates.io", Some(8443))]),
        Upstream::default(),
    );
    for request in [
        "CONNECT evil.example:443 HTTP/1.1\r\n\r\n",
        "CONNECT crates.io:22 HTTP/1.1\r\n\r\n",
        "CONNECT static.crates.io:443 HTTP/1.1\r\n\r\n",
        "CONNECT 169.254.169.254:80 HTTP/1.1\r\n\r\n",
        "GET http://127.0.0.1:7777/ HTTP/1.1\r\n\r\n",
    ] {
        let (head, _) = p.ask(request).await;
        assert!(head.starts_with("HTTP/1.1 403"), "{request}: {head}");
    }
    assert!(lock(&p.dial.connected).is_empty());
    let refused = p.refused();
    let hosts: Vec<&str> = refused.iter().map(|(h, _)| h.as_str()).collect();
    assert_eq!(
        hosts,
        [
            "evil.example:443",
            "crates.io:22",
            "static.crates.io:443",
            "169.254.169.254:80",
            "127.0.0.1:7777"
        ]
    );
    assert_eq!(refused[1].1, "port 22 is not allowed for this host");
}

#[tokio::test]
async fn an_allowed_name_that_resolves_to_a_private_address_is_refused() {
    let mut p = proxy(
        web(),
        rules(&[("rebound.example", None), ("inside.example", None)]),
        Upstream::default(),
    );
    for request in [
        "CONNECT rebound.example:443 HTTP/1.1\r\n\r\n",
        "CONNECT inside.example:443 HTTP/1.1\r\n\r\n",
    ] {
        let (head, _) = p.ask(request).await;
        assert!(head.starts_with("HTTP/1.1 403"), "{head}");
    }
    assert!(lock(&p.dial.connected).is_empty());
    assert_eq!(p.refused().len(), 2);
}

#[tokio::test]
async fn an_endpoint_the_user_configured_may_be_local() {
    let fake = FakeNet::default()
        .name("gateway.lan", &["192.168.1.20"])
        .server("192.168.1.20:4000", Server::Echo);
    let var = |name: &str| {
        (name == "ANTHROPIC_BASE_URL").then(|| "http://Gateway.lan:4000/v1".to_owned())
    };
    let base = crate::base::rules(&var);
    let p = proxy(
        fake,
        Rules {
            base: base.into(),
            project: Vec::new(),
        },
        Upstream::default(),
    );
    let (head, _) = p.ask("CONNECT gateway.lan:4000 HTTP/1.1\r\n\r\n").await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    let (head, _) = p.ask("CONNECT gateway.lan:22 HTTP/1.1\r\n\r\n").await;
    assert!(head.starts_with("HTTP/1.1 403"), "{head}");
}

#[tokio::test]
async fn plain_http_goes_on_as_one_request_without_proxy_headers() {
    let p = proxy(web(), rules(&[("crates.io", None)]), Upstream::default());
    let (head, mut stream) = p
        .ask("GET http://crates.io/api?q=1 HTTP/1.1\r\nHost: evil.example\r\nProxy-Authorization: Basic eDp5\r\nAccept: */*\r\nConnection: keep-alive\r\n\r\n")
        .await;
    assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
    let mut body = String::new();
    let _ = stream.read_to_string(&mut body).await;
    assert_eq!(body, "ok");
    assert_eq!(
        heads(&p, 1).await,
        ["GET /api?q=1 HTTP/1.1\r\nAccept: */*\r\nHost: crates.io\r\nConnection: close\r\n\r\n"]
    );
}

#[tokio::test]
async fn requests_that_are_not_proxy_requests_are_bad() {
    let mut p = proxy(web(), rules(&[("crates.io", None)]), Upstream::default());
    for request in [
        "GET https://crates.io/ HTTP/1.1\r\n\r\n",
        "GET /index.html HTTP/1.1\r\nHost: crates.io\r\n\r\n",
        "CONNECT crates.io HTTP/1.1\r\n\r\n",
        "GET http://user@crates.io/ HTTP/1.1\r\n\r\n",
        "not http at all\r\n\r\n",
    ] {
        let (head, _) = p.ask(request).await;
        assert!(head.starts_with("HTTP/1.1 400"), "{request}: {head}");
    }
    let (mut client, theirs) = duplex(64 * 1024);
    let serving = tokio::spawn(serve(theirs, p.shared.clone(), p.net.clone()));
    let huge = format!(
        "GET http://crates.io/ HTTP/1.1\r\nX: {}\r\n\r\n",
        "a".repeat(20_000)
    );
    let _ = client.write_all(huge.as_bytes()).await;
    assert!(head_of(&mut client).await.starts_with("HTTP/1.1 400"));
    let _ = serving.await;
    assert!(p.refused().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_client_that_never_finishes_its_head_is_dropped() {
    let p = proxy(web(), rules(&[]), Upstream::default());
    let (mut client, theirs) = duplex(4096);
    let serving = tokio::spawn(serve(theirs, p.shared.clone(), p.net.clone()));
    let _ = client
        .write_all(b"CONNECT crates.io:443 HTTP/1.1\r\n")
        .await;
    // The paused clock runs ahead to the head timeout once all is idle.
    let _ = serving.await;
    let mut rest = Vec::new();
    let _ = client.read_to_end(&mut rest).await;
    assert!(rest.is_empty());
}

#[tokio::test]
async fn the_users_own_proxy_carries_tunnels_unless_no_proxy_says_otherwise() {
    let fake = web();
    let var = |name: &str| match name {
        "HTTPS_PROXY" => Some("http://me:p%40ss@proxy.corp:3128".to_owned()),
        "NO_PROXY" => Some("localhost, .anthropic.com".to_owned()),
        _ => None,
    };
    let p = proxy(
        fake,
        rules(&[("crates.io", None), ("private.example", None)]),
        Upstream::from_env(&var),
    );
    let (head, mut stream) = p.ask("CONNECT crates.io:443 HTTP/1.1\r\n\r\n").await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(roundtrip(&mut stream).await, "hello");
    assert_eq!(
        lock(&p.dial.heads).as_slice(),
        [
            "CONNECT crates.io:443 HTTP/1.1\r\nHost: crates.io:443\r\nProxy-Authorization: Basic bWU6cEBzcw==\r\n\r\n"
        ]
    );
    // A name only the user's proxy can resolve still goes through it.
    let (head, _) = p.ask("CONNECT private.example:443 HTTP/1.1\r\n\r\n").await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    // NO_PROXY: straight to the server.
    let (head, _) = p
        .ask("CONNECT api.anthropic.com:443 HTTP/1.1\r\n\r\n")
        .await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    let connected = lock(&p.dial.connected).clone();
    assert_eq!(
        connected.last().map(ToString::to_string).as_deref(),
        Some("160.79.104.10:443")
    );
}

#[test]
fn the_base_list_follows_the_users_provider() {
    let hosts = |vars: &[(&str, &str)]| {
        let vars: BTreeMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect();
        let var = move |name: &str| vars.get(name).cloned();
        crate::base::rules(&var)
            .into_iter()
            .map(|r| match r.port {
                Some(port) => format!("{}:{port}", r.host),
                None => r.host,
            })
            .collect::<Vec<_>>()
    };
    let anthropic = ["api.anthropic.com", "claude.ai", "platform.claude.com"];
    assert_eq!(hosts(&[]), anthropic);
    assert_eq!(
        hosts(&[
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
            ("AWS_REGION", "eu-west-1")
        ])[3..],
        ["bedrock-runtime.eu-west-1.amazonaws.com"]
    );
    assert_eq!(
        hosts(&[
            ("CLAUDE_CODE_USE_BEDROCK", "true"),
            ("AWS_REGION", "x.evil.com/")
        ])[3..],
        ["bedrock-runtime.us-east-1.amazonaws.com"]
    );
    assert_eq!(
        hosts(&[
            ("CLAUDE_CODE_USE_VERTEX", "1"),
            ("CLOUD_ML_REGION", "europe-west4")
        ])[3..],
        [
            "europe-west4-aiplatform.googleapis.com",
            "oauth2.googleapis.com"
        ]
    );
    assert_eq!(
        hosts(&[
            ("CLAUDE_CODE_USE_FOUNDRY", "1"),
            ("ANTHROPIC_FOUNDRY_RESOURCE", "Mine")
        ])[3..],
        ["mine.services.ai.azure.com"]
    );
    assert_eq!(
        hosts(&[("ANTHROPIC_BASE_URL", "https://llm.corp.example/anthropic")])[3..],
        ["llm.corp.example:443"]
    );
    assert_eq!(hosts(&[("CLAUDE_CODE_USE_BEDROCK", "0")]), anthropic);
}

#[cfg(unix)]
#[tokio::test]
async fn each_session_gets_a_socket_that_follows_updates_and_closes_on_stop() {
    use tokio::net::UnixStream;

    let fake = Arc::new(web());
    let proxies = NetProxies::new(&|_| None, fake.clone()).unwrap_or_else(|e| panic!("{e}"));
    let core = Core::new("test");
    let session = SessionId("s1".into());
    let reporter = NetReporter::new(ProjectId("p1".into()), session.clone(), core.sink());
    let allow = |hosts: &[&str]| Allowlist {
        hosts: hosts
            .iter()
            .map(|h| AllowedHost {
                host: (*h).into(),
                port: None,
            })
            .collect(),
    };
    let address = proxies
        .start(session.clone(), allow(&["crates.io"]), reporter, Vec::new())
        .await
        .unwrap_or_else(|e| panic!("{e}"))
        .address;
    let mode = std::fs::metadata(&proxies.dir)
        .map(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o777);
    assert_eq!(mode.ok(), Some(0o700));
    let ask = |request: &'static str| {
        let address = address.clone();
        async move {
            let mut stream = UnixStream::connect(&address)
                .await
                .unwrap_or_else(|e| panic!("{e}"));
            stream
                .write_all(request.as_bytes())
                .await
                .unwrap_or_else(|e| panic!("{e}"));
            let mut buf = vec![0u8; 12];
            stream
                .read_exact(&mut buf)
                .await
                .unwrap_or_else(|e| panic!("{e}"));
            (String::from_utf8_lossy(&buf).into_owned(), stream)
        }
    };
    let (status, mut tunnel) = ask("CONNECT crates.io:443 HTTP/1.1\r\n\r\n").await;
    assert_eq!(status, "HTTP/1.1 200");
    proxies.update(&session, allow(&[]));
    let (status, _) = ask("CONNECT crates.io:443 HTTP/1.1\r\n\r\n").await;
    assert_eq!(status, "HTTP/1.1 403");
    proxies.stop(&session);
    assert!(UnixStream::connect(&address).await.is_err());
    // The open tunnel ends with its proxy.
    let mut rest = Vec::new();
    let _ = tunnel.read_to_end(&mut rest).await;
    let dir = proxies.dir.clone();
    drop(proxies);
    assert!(!dir.exists());
}

fn corp_proxy() -> Upstream {
    Upstream::from_env(&|name: &str| match name {
        "HTTPS_PROXY" => Some("http://proxy.corp:3128".to_owned()),
        "HTTP_PROXY" => Some("http://me:pw@proxy.corp:3129".to_owned()),
        "NO_PROXY" => Some("10.0.0.0/8,*.internal.example".to_owned()),
        _ => None,
    })
}

#[tokio::test]
async fn plain_http_passes_one_body_and_never_a_pipelined_request() {
    let p = proxy(web(), rules(&[("crates.io", None)]), corp_proxy());
    let (head, mut stream) = p
        .ask("POST http://crates.io/up HTTP/1.1\r\nContent-Length: 5\r\nConnection: x-secret\r\nX-Secret: 1\r\nProxy-Foo: 1\r\nUpgrade: h2c\r\n\r\nhelloGET http://169.254.169.254/ HTTP/1.1\r\nHost: x\r\n\r\n")
        .await;
    assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
    let mut body = String::new();
    let _ = stream.read_to_string(&mut body).await;
    assert_eq!(
        heads(&p, 1).await,
        [
            "POST http://crates.io/up HTTP/1.1\r\nContent-Length: 5\r\nHost: crates.io\r\nConnection: close\r\nProxy-Authorization: Basic bWU6cHc=\r\n\r\nhello"
        ]
    );
}

#[tokio::test]
async fn bodies_that_cannot_be_framed_are_refused() {
    let p = proxy(web(), rules(&[("crates.io", None)]), Upstream::default());
    for request in [
        "POST http://crates.io/ HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
        "POST http://crates.io/ HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\nx",
        "POST http://crates.io/ HTTP/1.1\r\nContent-Length: +1\r\n\r\nx",
    ] {
        let (head, _) = p.ask(request).await;
        assert!(head.starts_with("HTTP/1.1 400"), "{request}: {head}");
    }
    assert!(lock(&p.dial.connected).is_empty());
}

#[tokio::test]
async fn an_upstream_that_refuses_the_tunnel_is_a_bad_gateway() {
    let fake = web().server("10.1.1.1:3130", Server::Refuse);
    let upstream = Upstream::from_env(&|name: &str| {
        (name == "HTTPS_PROXY").then(|| "proxy.corp:3130".to_owned())
    });
    let p = proxy(fake, rules(&[("crates.io", None)]), upstream);
    let (head, _) = p.ask("CONNECT crates.io:443 HTTP/1.1\r\n\r\n").await;
    assert!(head.starts_with("HTTP/1.1 502"), "{head}");
    // An upstream that cannot be reached is one too.
    let p = proxy(
        web(),
        rules(&[("crates.io", None)]),
        Upstream::from_env(&|name: &str| {
            (name == "HTTPS_PROXY").then(|| "proxy.corp:9".to_owned())
        }),
    );
    let (head, _) = p.ask("CONNECT crates.io:443 HTTP/1.1\r\n\r\n").await;
    assert!(head.starts_with("HTTP/1.1 502"), "{head}");
}

#[tokio::test]
async fn targets_are_normalized_before_the_rules_see_them() {
    let fake = web()
        .server("93.184.216.34:443", Server::Echo)
        .server("[2606:4700::1111]:443", Server::Echo);
    let mut p = proxy(
        fake,
        rules(&[("crates.io", None), ("2606:4700::1111", None)]),
        Upstream::default(),
    );
    for (request, status) in [
        ("CONNECT CRATES.IO.:443 HTTP/1.1\r\n\r\n", "200"),
        ("CONNECT [2606:4700:0::1111]:443 HTTP/1.1\r\n\r\n", "200"),
        ("CONNECT crates.io:+443 HTTP/1.1\r\n\r\n", "400"),
        ("CONNECT crates.io:443:443 HTTP/1.1\r\n\r\n", "400"),
        ("CONNECT crates.io:0 HTTP/1.1\r\n\r\n", "400"),
    ] {
        let (head, _) = p.ask(request).await;
        assert!(
            head.starts_with(&format!("HTTP/1.1 {status}")),
            "{request}: {head}"
        );
    }
    assert!(p.refused().is_empty());
}

#[tokio::test]
async fn a_configured_gateway_lends_its_private_pass_to_no_other_entry() {
    let fake = FakeNet::default()
        .name("gateway.lan", &["192.168.1.20"])
        .server("192.168.1.20:4000", Server::Echo)
        .server("192.168.1.20:443", Server::Echo);
    let var =
        |name: &str| (name == "ANTHROPIC_BASE_URL").then(|| "http://gateway.lan:4000".to_owned());
    let p = proxy(
        fake,
        Rules {
            base: crate::base::rules(&var).into(),
            project: vec![AllowedHost {
                host: "gateway.lan".into(),
                port: None,
            }],
        },
        Upstream::default(),
    );
    let (head, _) = p.ask("CONNECT gateway.lan:443 HTTP/1.1\r\n\r\n").await;
    assert!(head.starts_with("HTTP/1.1 403"), "{head}");
    let (head, _) = p.ask("CONNECT gateway.lan:4000 HTTP/1.1\r\n\r\n").await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
}

#[test]
fn no_proxy_takes_names_suffixes_and_networks() {
    let upstream = corp_proxy();
    assert!(upstream.server("crates.io", true).is_some());
    assert!(upstream.server("10.9.8.7", true).is_none());
    assert!(upstream.server("a.internal.example", true).is_none());
    assert!(upstream.server("internal.example", true).is_none());
    assert!(upstream.server("notinternal.example", true).is_some());
    assert!(upstream.server("11.0.0.1", false).is_some());
}
