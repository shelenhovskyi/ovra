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
    let csp = csp.expect("csp");
    assert!(csp.contains("frame-ancestors 'none'"));
    let wss = s.origin.replacen("https://", "wss://", 1);
    assert!(csp.contains(&format!("connect-src 'self' {wss};")), "{csp}");
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

#[test]
fn app_files_stay_inside_their_folder() {
    assert!(crate::inside("").is_none());
    assert!(crate::inside("../x").is_none());
    assert!(crate::inside("C:x").is_none());
    assert!(crate::inside("a\\b").is_none());
    assert!(crate::inside("assets/app.js").is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_code_replaces_an_old_one_without_being_closed_by_it() {
    let s = setup();
    let old = secret_of(&s.server.start_pairing().expect("pairing").url);
    let mut events = s.core.subscribe();
    let body = json!({"secret": old, "key": TestKey::new().public(), "name": "old"}).to_string();
    let head = format!(
        "POST /api/pair HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json",
        s.host
    );
    let stream = tls(&s).await;
    let waiting = tokio::spawn(async move { http(stream, &head, &body, "x").await.0 });
    pairing_request(&mut events).await;

    // The person opens a new code while the old phone still waits.
    let new = secret_of(&s.server.start_pairing().expect("pairing").url);
    assert_eq!(waiting.await.expect("post"), 403);
    assert!(
        s.server.shared.lock().pairing.is_some(),
        "the new code still works"
    );
    let (status, _) = pair(&s, &new, &TestKey::new(), true).await;
    assert_eq!(status, 200);
}

/// A paired, signed-in phone past its `info.ready`.
async fn signed_in(s: &Setup) -> (Ws, String) {
    let pairing = s.server.start_pairing().expect("pairing");
    let key = TestKey::new();
    let (_, body) = pair(s, &secret_of(&pairing.url), &key, true).await;
    let paired: Value = serde_json::from_slice(&body).expect("json");
    let device = paired["device"].as_str().expect("device").to_owned();
    let (mut ws, _) = sign_in(s, &device, &key).await;
    next_json(&mut ws).await.expect("ready");
    (ws, device)
}

async fn ask(ws: &mut Ws, frame: Value) -> Value {
    ws.send(Message::Text(frame.to_string().into()))
        .await
        .expect("send");
    next_json(ws).await.expect("reply")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_phone_subscribes_to_pushes_for_itself_only() {
    let s = setup();
    let (mut ws, device) = signed_in(&s).await;
    let push = |s: &Setup| {
        s.server
            .shared
            .lock()
            .devices
            .get(&device)
            .and_then(|d| d.push.clone())
    };

    let reply = ask(&mut ws, json!({"id": 1, "method": "phone.push_key"})).await;
    let vapid = s
        .server
        .shared
        .lock()
        .running
        .as_ref()
        .expect("running")
        .vapid
        .public();
    assert_eq!(reply, json!({"id": 1, "result": {"key": vapid}}));

    let key = TestKey::new().public();
    let subscription = |endpoint: &str| {
        json!({"endpoint": endpoint, "expirationTime": null,
               "keys": {"p256dh": key, "auth": "AAAAAAAAAAAAAAAAAAAAAA"}})
    };
    let subscribe = |id: u64, sub: Value| json!({"id": id, "method": "phone.push_subscribe", "params": {"subscription": sub}});
    for (id, bad) in [
        (2, subscribe(2, subscription("https://evil.example/x"))),
        (3, subscribe(3, subscription("http://fcm.googleapis.com/x"))),
        (
            4,
            subscribe(4, json!({"endpoint": "https://fcm.googleapis.com/x"})),
        ),
        (5, json!({"id": 5, "method": "phone.push_subscribe"})),
        (6, json!({"id": 6, "method": "phone.nope"})),
    ] {
        let reply = ask(&mut ws, bad).await;
        assert_eq!(reply["id"], id);
        assert_eq!(reply["error"]["code"], "bad_request", "{reply}");
    }
    assert!(push(&s).is_none());

    let good = subscription("https://fcm.googleapis.com/fcm/send/abc");
    let reply = ask(&mut ws, subscribe(7, good)).await;
    assert_eq!(reply, json!({"id": 7, "result": null}));
    let kept = push(&s).expect("subscribed");
    assert_eq!(kept.endpoint, "https://fcm.googleapis.com/fcm/send/abc");

    let reply = ask(&mut ws, subscribe(8, Value::Null)).await;
    assert_eq!(reply, json!({"id": 8, "result": null}));
    assert!(push(&s).is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_phone_forgets_itself_and_its_session_closes() {
    let s = setup();
    let (mut ws, device) = signed_in(&s).await;
    let reply = ask(&mut ws, json!({"id": 1, "method": "phone.nope"})).await;
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["error"]["code"], "bad_request", "{reply}");
    let reply = ask(&mut ws, json!({"method": "phone.forget"})).await;
    assert_eq!(reply["error"]["code"], "bad_request", "no id, nothing done");
    assert_eq!(s.server.devices().expect("devices").len(), 1);
    let mut events = s.core.subscribe();
    let reply = ask(
        &mut ws,
        json!({"id": 1, "method": "phone.forget", "params": {"device": "someone-else"}}),
    )
    .await;
    assert_eq!(reply, json!({"id": 1, "result": null}));
    assert!(next_json(&mut ws).await.is_none(), "the session closed");
    assert!(s.server.devices().expect("devices").is_empty());
    assert!(
        !s.server
            .shared
            .lock()
            .devices
            .list()
            .iter()
            .any(|d| d.id == device)
    );
    loop {
        if let Some(Event::Remote(RemoteEvent::DevicesChanged)) =
            Core::next_event(&mut events).await
        {
            break;
        }
    }
}
