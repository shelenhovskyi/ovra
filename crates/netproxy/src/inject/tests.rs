use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, RwLock};

use ovra_core::Core;
use ovra_core::fakes::MemorySecrets;
use ovra_core::protocol::agents::SessionId;
use ovra_core::protocol::netproxy::AllowedHost;
use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::secrets::{SecretInfo, SecretValue};
use ovra_core::services::{BoxFuture, NetReporter, Secrets};
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{
    CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName,
};
use tokio_rustls::rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use super::{Body, Bound, check, copy_chunked};
use crate::ca::SessionCa;
use crate::dial::{Conn, Dial};
use crate::rules::Rules;
use crate::serve::{SessionNet, Shared, serve};
use crate::tls::{Roots, provider};
use crate::upstream::Upstream;

const HOST: &str = "api.example.com";

fn gh() -> SecretInfo {
    SecretInfo {
        name: "gh".into(),
        host: HOST.into(),
        header: "Authorization".into(),
        prefix: "Bearer ".into(),
    }
}

fn checked(raw: &str) -> Result<String, &'static str> {
    check(raw.as_bytes(), HOST, 443, &[gh()]).map(|c| String::from_utf8_lossy(&c.head).into_owned())
}

#[test]
fn a_request_is_rebuilt_without_what_the_secret_or_this_hop_owns() {
    let head = checked(
        "POST /repos?x=1 HTTP/1.1\r\nHost: API.example.com\r\nauthorization: token fake\r\n\
         Connection: keep-alive, X-Hop\r\nX-Hop: 1\r\nProxy-Authorization: p\r\n\
         Content-Length: 3\r\nAccept: */*\r\n\r\n",
    );
    assert_eq!(
        head.as_deref(),
        Ok(
            "POST /repos?x=1 HTTP/1.1\r\nHost: api.example.com\r\nAccept: */*\r\n\
            Content-Length: 3\r\nConnection: close\r\n"
        )
    );
    let chunked = check(
        b"PUT / HTTP/1.1\r\nHost: api.example.com:443\r\nTransfer-Encoding: Chunked\r\n\r\n",
        HOST,
        443,
        &[gh()],
    )
    .map(|c| c.body);
    assert_eq!(chunked, Ok(Body::Chunked));
}

#[test]
fn every_form_two_parsers_could_read_differently_is_refused() {
    let host = "Host: api.example.com\r\n";
    let refused = [
        format!("GET / HTTP/1.1\r\n{host}Content-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n"),
        format!("GET / HTTP/1.1\r\n{host}Content-Length: 1\r\nContent-Length: 1\r\n\r\n"),
        format!("GET / HTTP/1.1\r\n{host}Content-Length: +1\r\n\r\n"),
        format!("GET / HTTP/1.1\r\n{host}Transfer-Encoding: gzip, chunked\r\n\r\n"),
        format!(
            "GET / HTTP/1.1\r\n{host}Transfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n\r\n"
        ),
        format!("GET / HTTP/1.1\r\n{host}X-A: 1\r\n folded\r\n\r\n"),
        format!("GET / HTTP/1.1\n{host}\r\n"),
        format!("GET / HTTP/1.1\r\n{host}Upgrade: websocket\r\n\r\n"),
        format!("CONNECT api.example.com:443 HTTP/1.1\r\n{host}\r\n"),
        format!("GET https://evil.example/ HTTP/1.1\r\n{host}\r\n"),
        format!("GET / HTTP/1.0\r\n{host}\r\n"),
        "GET / HTTP/1.1\r\n\r\n".to_owned(),
        format!("GET / HTTP/1.1\r\n{host}{host}\r\n"),
        "GET / HTTP/1.1\r\nHost: evil.example\r\n\r\n".to_owned(),
        "GET / HTTP/1.1\r\nHost: api.example.com:8443\r\n\r\n".to_owned(),
    ];
    for raw in refused {
        assert!(checked(&raw).is_err(), "accepted: {raw:?}");
    }
}

async fn rechunk(body: &[u8]) -> io::Result<Vec<u8>> {
    let mut from = tokio::io::BufReader::new(body);
    let mut to = Vec::new();
    copy_chunked(&mut from, &mut to).await?;
    Ok(to)
}

#[tokio::test]
async fn chunked_bodies_are_decoded_strictly_and_written_again() {
    let ok = rechunk(b"3;ext=1\r\nabc\r\nA\r\n0123456789\r\n0\r\n\r\nGET /next").await;
    assert_eq!(
        ok.ok().as_deref(),
        Some(&b"3\r\nabc\r\na\r\n0123456789\r\n0\r\n\r\n"[..])
    );
    for bad in [
        &b"3\r\nabcd\r\n0\r\n\r\n"[..],
        b"3\nabc\r\n0\r\n\r\n",
        b"x\r\nabc\r\n0\r\n\r\n",
        b"\r\n",
        b"3\r\nabc\r\n0\r\nX-Trailer: 1\r\n\r\n",
        b"11111111111111111\r\n",
    ] {
        assert!(rechunk(bad).await.is_err(), "accepted: {bad:?}");
    }
}

/// A real-looking host behind a test CA: it keeps what it receives and
/// answers `ok`.
#[derive(Debug)]
struct Host {
    tls: Arc<ServerConfig>,
    received: Arc<Mutex<Vec<String>>>,
    connects: Arc<Mutex<u32>>,
    /// Plain HTTP: keeps the raw request head instead of speaking TLS.
    plain: bool,
}

impl Dial for Host {
    fn resolve(&self, _: String, port: u16) -> BoxFuture<'_, io::Result<Vec<SocketAddr>>> {
        Box::pin(async move { Ok(vec![SocketAddr::from(([93, 184, 216, 34], port))]) })
    }

    fn resolve_as_given(
        &self,
        host: String,
        port: u16,
    ) -> BoxFuture<'_, io::Result<Vec<SocketAddr>>> {
        self.resolve(host, port)
    }

    fn connect(&self, _: SocketAddr) -> BoxFuture<'_, io::Result<Conn>> {
        Box::pin(async move {
            *self.connects.lock().unwrap_or_else(|e| e.into_inner()) += 1;
            let (ours, theirs) = duplex(64 * 1024);
            let (tls, received) = (self.tls.clone(), self.received.clone());
            if self.plain {
                tokio::spawn(async move {
                    let mut theirs = theirs;
                    let mut got = Vec::new();
                    let mut byte = [0u8; 1];
                    while !got.ends_with(b"\r\n\r\n") {
                        if theirs.read(&mut byte).await.unwrap_or(0) == 0 {
                            break;
                        }
                        got.push(byte[0]);
                    }
                    received
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(String::from_utf8_lossy(&got).into_owned());
                    let _ = theirs
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                        .await;
                });
                return Ok(Box::new(ours) as Conn);
            }
            tokio::spawn(async move {
                let Ok(mut stream) = TlsAcceptor::from(tls).accept(theirs).await else {
                    return;
                };
                let mut got = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let text = String::from_utf8_lossy(&got).into_owned();
                    let done = match text.split_once("\r\n\r\n") {
                        Some((head, body)) if head.contains("Transfer-Encoding: chunked") => {
                            body.ends_with("0\r\n\r\n")
                        }
                        Some((head, body)) => {
                            let length = head
                                .lines()
                                .find_map(|l| l.strip_prefix("Content-Length: "))
                                .and_then(|l| l.parse::<usize>().ok())
                                .unwrap_or(0);
                            body.len() >= length
                        }
                        None => false,
                    };
                    if done {
                        break;
                    }
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => got.extend_from_slice(&buf[..n]),
                    }
                }
                received
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(String::from_utf8_lossy(&got).into_owned());
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    )
                    .await;
                let _ = stream.shutdown().await;
                // Anything more would be a second request.
                let mut after = Vec::new();
                let _ = stream.read_to_end(&mut after).await;
                if !after.is_empty() {
                    received
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(format!("AFTER: {}", String::from_utf8_lossy(&after)));
                }
            });
            Ok(Box::new(ours) as Conn)
        })
    }
}

/// A CA for the fake host, and its server config for `HOST`.
fn host_tls() -> (CertificateDer<'static>, Arc<ServerConfig>) {
    let ca_key = KeyPair::generate().unwrap_or_else(|e| panic!("{e}"));
    let mut ca = CertificateParams::default();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_cert = ca.self_signed(&ca_key).unwrap_or_else(|e| panic!("{e}"));
    let issuer = Issuer::new(ca, ca_key);
    let key = KeyPair::generate().unwrap_or_else(|e| panic!("{e}"));
    let leaf = CertificateParams::new(vec![HOST.into()])
        .and_then(|p| p.signed_by(&key, &issuer))
        .unwrap_or_else(|e| panic!("{e}"));
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
    let config = ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .and_then(|b| {
            b.with_no_client_auth()
                .with_single_cert(vec![leaf.der().clone()], key)
        })
        .unwrap_or_else(|e| panic!("{e}"));
    (ca_cert.der().clone(), Arc::new(config))
}

struct Setup {
    shared: Arc<Shared>,
    net: Arc<SessionNet>,
    host: Arc<Host>,
    agent_tls: Arc<ClientConfig>,
    _core: Core,
}

fn setup(secrets: Arc<MemorySecrets>) -> Setup {
    setup_with(secrets, true, false)
}

/// `trusted`: Ovra's roots hold the host's CA. `plain`: the host speaks
/// plain HTTP.
fn setup_with(secrets: Arc<MemorySecrets>, trusted: bool, plain: bool) -> Setup {
    let (host_ca, host_config) = host_tls();
    let host = Arc::new(Host {
        tls: host_config,
        received: Arc::default(),
        connects: Arc::default(),
        plain,
    });
    let ca = SessionCa::new(&[HOST.to_owned()]).unwrap_or_else(|e| panic!("{e}"));
    // The agent trusts only what the sandbox's bundle holds: the session CA.
    let mut roots = RootCertStore::empty();
    let session_ca =
        CertificateDer::from_pem_slice(ca.pem().as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    roots.add(session_ca).unwrap_or_else(|e| panic!("{e}"));
    let agent_tls = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map(|b| b.with_root_certificates(roots).with_no_client_auth())
        .unwrap_or_else(|e| panic!("{e}"));
    let core = Core::new("test");
    let bound = Bound {
        ca,
        secrets: vec![gh()],
        project: ProjectId("p1".into()),
        store: secrets,
        client: Roots::of(if trusted { vec![host_ca] } else { Vec::new() })
            .client()
            .unwrap_or_else(|e| panic!("{e}")),
    };
    let net = Arc::new(SessionNet {
        rules: RwLock::new(Rules {
            base: Arc::new([]),
            project: vec![AllowedHost {
                host: HOST.into(),
                port: None,
            }],
        }),
        reporter: NetReporter::new(ProjectId("p1".into()), SessionId("s1".into()), core.sink()),
        bound: Some(bound),
        grants: tokio::sync::watch::Sender::new(Arc::default()),
    });
    Setup {
        shared: Arc::new(Shared {
            dial: host.clone(),
            upstream: Upstream::default(),
        }),
        net,
        host,
        agent_tls: Arc::new(agent_tls),
        _core: core,
    }
}

/// Sends `request` as an agent would: CONNECT, then TLS that trusts the
/// session CA. Returns the answer, decrypted.
async fn ask(setup: &Setup, request: &[u8]) -> String {
    let (mut agent, proxy) = duplex(64 * 1024);
    tokio::spawn(serve(proxy, setup.shared.clone(), setup.net.clone()));
    agent
        .write_all(format!("CONNECT {HOST}:443 HTTP/1.1\r\nHost: {HOST}:443\r\n\r\n").as_bytes())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut answer = Vec::new();
    let mut byte = [0u8; 1];
    while !answer.ends_with(b"\r\n\r\n") {
        if agent.read(&mut byte).await.unwrap_or(0) == 0 {
            panic!("no CONNECT answer: {}", String::from_utf8_lossy(&answer));
        }
        answer.push(byte[0]);
    }
    assert!(
        answer.starts_with(b"HTTP/1.1 200"),
        "{}",
        String::from_utf8_lossy(&answer)
    );
    let name = ServerName::try_from(HOST).unwrap_or_else(|e| panic!("{e}"));
    let mut tls = TlsConnector::from(setup.agent_tls.clone())
        .connect(name, agent)
        .await
        .unwrap_or_else(|e| panic!("TLS to the proxy: {e}"));
    tls.write_all(request)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut out = Vec::new();
    let _ = tls.read_to_end(&mut out).await;
    String::from_utf8_lossy(&out).into_owned()
}

fn received(setup: &Setup) -> Vec<String> {
    setup
        .host
        .received
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[tokio::test]
async fn the_secret_reaches_the_bound_host_and_nothing_after_the_request_does() {
    let secrets = Arc::new(MemorySecrets::default());
    secrets
        .set(&ProjectId("p1".into()), gh(), SecretValue::new("ghp_value"))
        .unwrap_or_else(|e| panic!("{e}"));
    let setup = setup(secrets);
    let answer = ask(
        &setup,
        b"POST /gists HTTP/1.1\r\nHost: api.example.com\r\nAuthorization: Bearer agent-made\r\n\
          Transfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n\
          GET /smuggled HTTP/1.1\r\nHost: api.example.com\r\n\r\n",
    )
    .await;
    assert!(answer.ends_with("ok"), "{answer}");
    let got = received(&setup);
    assert_eq!(got.len(), 1, "one request, nothing after it: {got:?}");
    assert_eq!(
        got[0],
        "POST /gists HTTP/1.1\r\nHost: api.example.com\r\nTransfer-Encoding: chunked\r\n\
         Connection: close\r\nAuthorization: Bearer ghp_value\r\n\r\n5\r\nhello\r\n0\r\n\r\n"
    );
}

#[tokio::test]
async fn without_the_value_nothing_is_sent() {
    // No value stored: the binding exists but the keychain has nothing.
    let setup = setup(Arc::new(MemorySecrets::default()));
    let answer = ask(&setup, b"GET / HTTP/1.1\r\nHost: api.example.com\r\n\r\n").await;
    assert!(answer.starts_with("HTTP/1.1 502"), "{answer}");
    assert!(received(&setup).is_empty());
}

#[tokio::test]
async fn a_smuggling_head_is_answered_by_the_proxy_and_never_sent() {
    let secrets = Arc::new(MemorySecrets::default());
    secrets
        .set(&ProjectId("p1".into()), gh(), SecretValue::new("ghp_value"))
        .unwrap_or_else(|e| panic!("{e}"));
    let setup = setup(secrets);
    let answer = ask(
        &setup,
        b"POST / HTTP/1.1\r\nHost: api.example.com\r\nContent-Length: 4\r\n\
          Transfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
    )
    .await;
    assert!(answer.starts_with("HTTP/1.1 400"), "{answer}");
    assert!(!answer.contains("ghp_value"));
    assert!(received(&setup).is_empty());
}

fn stored() -> Arc<MemorySecrets> {
    let secrets = Arc::new(MemorySecrets::default());
    secrets
        .set(&ProjectId("p1".into()), gh(), SecretValue::new("ghp_value"))
        .unwrap_or_else(|e| panic!("{e}"));
    secrets
}

#[tokio::test]
async fn a_host_ovra_cannot_verify_gets_nothing_and_the_refusal_is_reported() {
    let setup = setup_with(stored(), false, false);
    let mut events = setup._core.subscribe();
    let answer = ask(&setup, b"GET / HTTP/1.1\r\nHost: api.example.com\r\n\r\n").await;
    assert!(answer.starts_with("HTTP/1.1 502"), "{answer}");
    assert!(!answer.contains("ghp_value"));
    assert!(received(&setup).is_empty());
    assert!(matches!(
        events.try_recv(),
        Ok(ovra_core::protocol::Event::Netproxy(
            ovra_core::protocol::netproxy::NetproxyEvent::Refused { .. }
        ))
    ));
}

#[tokio::test]
async fn the_session_ca_cannot_vouch_for_another_name() {
    let ca = SessionCa::new(&[HOST.to_owned()]).unwrap_or_else(|e| panic!("{e}"));
    let other = ca
        .server_for("evil.example")
        .unwrap_or_else(|e| panic!("{e}"));
    let mut roots = RootCertStore::empty();
    let session_ca =
        CertificateDer::from_pem_slice(ca.pem().as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    roots.add(session_ca).unwrap_or_else(|e| panic!("{e}"));
    let client = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map(|b| b.with_root_certificates(roots).with_no_client_auth())
        .unwrap_or_else(|e| panic!("{e}"));
    let (agent, server) = duplex(64 * 1024);
    tokio::spawn(async move {
        let _ = TlsAcceptor::from(other).accept(server).await;
    });
    let name = ServerName::try_from("evil.example").unwrap_or_else(|e| panic!("{e}"));
    let made = TlsConnector::from(Arc::new(client))
        .connect(name, agent)
        .await;
    let err = made.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(
        !err.is_empty(),
        "the name constraints let another name through"
    );
}

#[tokio::test]
async fn plain_http_and_other_ports_never_carry_the_secret() {
    let setup = setup_with(stored(), true, true);
    let bound = setup
        .net
        .bound
        .as_ref()
        .unwrap_or_else(|| panic!("no secrets"));
    assert_eq!(bound.for_host(HOST, 443).len(), 1);
    assert!(bound.for_host(HOST, 8443).is_empty());
    assert!(bound.for_host(HOST, 80).is_empty());

    let (mut agent, proxy) = duplex(64 * 1024);
    tokio::spawn(serve(proxy, setup.shared.clone(), setup.net.clone()));
    agent
        .write_all(b"GET http://api.example.com/x HTTP/1.1\r\nHost: api.example.com\r\n\r\n")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut answer = Vec::new();
    let _ = agent.read_to_end(&mut answer).await;
    let got = received(&setup);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].starts_with("GET /x HTTP/1.1"), "{got:?}");
    assert!(!got[0].contains("ghp_value") && !got[0].contains("Authorization"));
}

#[tokio::test(start_paused = true)]
async fn a_body_that_stops_arriving_ends_the_tunnel() {
    let setup = setup(stored());
    // Ten bytes promised, two sent, then nothing: the idle timeout closes it.
    let answer = ask(
        &setup,
        b"POST / HTTP/1.1\r\nHost: api.example.com\r\nContent-Length: 10\r\n\r\nhi",
    )
    .await;
    assert!(answer.is_empty(), "{answer}");
}
