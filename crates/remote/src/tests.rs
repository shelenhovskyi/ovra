//! The listener end to end on loopback: TLS against Ovra's CA, pairing
//! confirmed on the computer, signed sessions, and what is refused.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use ovra_core::Core;
use ovra_core::protocol::Event;
use ovra_core::protocol::remote::{PairingRequest, RemoteEvent};
use ovra_core::services::RemoteAccess;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::auth::testkey::TestKey;
use crate::{Asset, Assets, RemoteServer, auth};

struct Setup {
    _dir: tempfile::TempDir,
    server: Arc<RemoteServer>,
    core: Core,
    origin: String,
    host: String,
    ca: Vec<u8>,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().expect("dir");
    let assets: Assets = Arc::new(|path: &str| {
        (path == "/mobile.html").then(|| Asset::new(path, b"<p>phone</p>".to_vec()))
    });
    let server = Arc::new(RemoteServer::for_tests(dir.path(), assets));
    let core = Core::builder("test").remote(server.clone()).build();
    let status = server
        .enable(core.clone(), Some("127.0.0.1".into()))
        .expect("enable");
    let origin = status.url.expect("listening");
    let host = origin.trim_start_matches("https://").to_owned();
    let ca = server
        .shared
        .lock()
        .running
        .as_ref()
        .expect("running")
        .ca
        .der()
        .to_vec();
    Setup {
        _dir: dir,
        server,
        core,
        origin,
        host,
        ca,
    }
}

async fn tls(s: &Setup) -> TlsStream<TcpStream> {
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(s.ca.clone())).expect("root");
    let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("tls")
        .with_root_certificates(roots)
        .with_no_client_auth();
    let stream = TcpStream::connect(&s.host).await.expect("connect");
    let name = ServerName::try_from("127.0.0.1").expect("name").to_owned();
    TlsConnector::from(Arc::new(config))
        .connect(name, stream)
        .await
        .expect("handshake")
}

/// One HTTP/1.1 request; the status, a header's value and the body.
async fn http(
    mut io: impl AsyncRead + AsyncWrite + Unpin,
    head: &str,
    body: &str,
    header: &str,
) -> (u16, Option<String>, Vec<u8>) {
    let request = format!(
        "{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    io.write_all(request.as_bytes()).await.expect("write");
    let mut raw = Vec::new();
    // A peer that closes without close_notify still sent its answer.
    let _ = io.read_to_end(&mut raw).await;
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response head");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("status");
    let value = head.lines().find_map(|l| {
        let (name, value) = l.split_once(':')?;
        name.eq_ignore_ascii_case(header)
            .then(|| value.trim().to_owned())
    });
    (status, value, raw[split + 4..].to_vec())
}

async fn pairing_request(events: &mut broadcast::Receiver<Event>) -> PairingRequest {
    loop {
        if let Some(Event::Remote(RemoteEvent::PairingRequest(r))) = Core::next_event(events).await
        {
            return r;
        }
    }
}

fn secret_of(url: &str) -> String {
    url.split_once("#pair=").expect("secret").1.to_owned()
}

/// Posts a pairing and answers it on the computer with `allow`.
async fn pair(s: &Setup, secret: &str, key: &TestKey, allow: bool) -> (u16, Vec<u8>) {
    let mut events = s.core.subscribe();
    let body = json!({"secret": secret, "key": key.public(), "name": "Test\nphone"}).to_string();
    let head = format!(
        "POST /api/pair HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json",
        s.host
    );
    let stream = tls(s).await;
    let posted = tokio::spawn(async move {
        let (status, _, body) = http(stream, &head, &body, "x").await;
        (status, body)
    });
    let request = pairing_request(&mut events).await;
    assert_eq!(request.name, "Testphone");
    assert_eq!(request.code, auth::code(secret, &key.public()));
    s.server.answer_pairing(&request.id, allow).expect("answer");
    posted.await.expect("post")
}

type Ws = tokio_tungstenite::WebSocketStream<TlsStream<TcpStream>>;

async fn open_ws(s: &Setup, origin: &str) -> Result<Ws, u16> {
    let mut request = format!("wss://{}/ws", s.host)
        .into_client_request()
        .expect("request");
    request
        .headers_mut()
        .insert("Origin", origin.parse().expect("origin"));
    match tokio_tungstenite::client_async(request, tls(s).await).await {
        Ok((ws, _)) => Ok(ws),
        Err(tokio_tungstenite::tungstenite::Error::Http(r)) => Err(r.status().as_u16()),
        Err(e) => panic!("{e}"),
    }
}

async fn next_json(ws: &mut Ws) -> Option<Value> {
    loop {
        match ws.next().await? {
            Ok(Message::Text(t)) => return serde_json::from_str(t.as_str()).ok(),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

/// Opens a session and signs its challenge as `device` with `key`.
async fn sign_in(s: &Setup, device: &str, key: &TestKey) -> (Ws, Value) {
    let mut ws = open_ws(s, &s.origin).await.expect("upgrade");
    let hello = next_json(&mut ws).await.expect("challenge");
    let challenge = hello["challenge"].as_str().expect("challenge");
    let signature = key.sign(&auth::message(&s.origin, challenge));
    let proof = json!({"device": device, "signature": signature}).to_string();
    ws.send(Message::Text(proof.into())).await.expect("send");
    let answer = next_json(&mut ws).await.expect("answer");
    (ws, answer)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_paired_phone_signs_in_uses_its_allowlist_and_is_forgotten() {
    let s = setup();
    let pairing = s.server.start_pairing().expect("pairing");
    let key = TestKey::new();
    let (status, body) = pair(&s, &secret_of(&pairing.url), &key, true).await;
    assert_eq!(status, 200);
    let paired: Value = serde_json::from_slice(&body).expect("json");
    let device = paired["device"].as_str().expect("device").to_owned();
    assert_eq!(s.server.devices().expect("devices").len(), 1);

    let (mut ws, answer) = sign_in(&s, &device, &key).await;
    assert_eq!(answer["paired"], "Testphone");
    let ready = next_json(&mut ws).await.expect("ready");
    assert_eq!(ready["event"]["type"], "info.ready");

    let ask = |id: u64, method: &str| json!({"id": id, "method": method}).to_string();
    ws.send(Message::Text(ask(1, "permissions.pending").into()))
        .await
        .expect("send");
    let reply = next_json(&mut ws).await.expect("reply");
    assert_eq!(reply["id"], 1);
    assert!(reply.get("result").is_some(), "{reply}");
    ws.send(Message::Text(ask(2, "remote.status").into()))
        .await
        .expect("send");
    let reply = next_json(&mut ws).await.expect("reply");
    assert_eq!(reply["error"]["code"], "forbidden", "{reply}");

    s.server.forget(&device).expect("forget");
    while next_json(&mut ws).await.is_some() {}
    let (_, answer) = sign_in(&s, &device, &key).await;
    assert_eq!(answer["error"], "not_paired");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_or_used_secret_pairs_nobody() {
    let s = setup();
    let pairing = s.server.start_pairing().expect("pairing");
    let secret = secret_of(&pairing.url);
    let (status, _) = pair(&s, &secret, &TestKey::new(), false).await;
    assert_eq!(status, 403);
    assert!(s.server.devices().expect("devices").is_empty());

    // The window closed with the answer, and the secret was spent anyway.
    let body = json!({"secret": secret, "key": TestKey::new().public(), "name": "x"}).to_string();
    let head = format!(
        "POST /api/pair HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json",
        s.host
    );
    let (status, ..) = http(tls(&s).await, &head, &body, "x").await;
    assert_eq!(status, 403);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_key_or_a_strange_origin_or_host_gets_nothing() {
    let s = setup();
    let (_, answer) = sign_in(&s, "nobody", &TestKey::new()).await;
    assert_eq!(answer["error"], "not_paired");
    assert_eq!(open_ws(&s, "https://evil.example").await.err(), Some(403));

    let get = |host: &str| format!("GET / HTTP/1.1\r\nHost: {host}");
    let (status, ..) = http(tls(&s).await, &get("evil.example"), "", "x").await;
    assert_eq!(status, 421);
    let (status, csp, body) =
        http(tls(&s).await, &get(&s.host), "", "content-security-policy").await;
    assert_eq!(status, 200);
    assert!(csp.expect("csp").contains("frame-ancestors 'none'"));
    assert_eq!(body, b"<p>phone</p>");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plain_http_hands_out_the_certificate_only_while_pairing() {
    let s = setup();
    let get = format!("GET /ca.crt HTTP/1.1\r\nHost: {}", s.host);
    let plain = || async { TcpStream::connect(&s.host).await.expect("connect") };
    let (status, location, _) = http(plain().await, &get, "", "location").await;
    assert_eq!(status, 308);
    assert_eq!(location, Some(format!("{}/ca.crt", s.origin)));

    s.server.start_pairing().expect("pairing");
    let (status, _, body) = http(plain().await, &get, "", "x").await;
    assert_eq!(status, 200);
    assert_eq!(body, s.ca);

    let status = s.server.disable().expect("disable");
    assert!(!status.enabled && status.url.is_none());
}
