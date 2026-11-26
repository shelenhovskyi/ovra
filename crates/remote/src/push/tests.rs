//! Pushes with a recording sender and a paused clock: who gets one, what
//! it holds, how often, and what a gone subscription does.

use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};

use ovra_core::protocol::agents::{AgentStatus, AgentsEvent, SessionId};
use ovra_core::protocol::permissions::PermissionsEvent;
use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::review::{ReviewEvent, ReviewId};
use ring::agreement::{ECDH_P256, EphemeralPrivateKey, UnparsedPublicKey};
use ring::rand::SystemRandom;
use serde_json::{Value, json};

use super::crypto::tests::{b64, open};
use super::*;
use crate::devices::Stored;
use crate::{Asset, Assets, RemoteServer};

/// Records every push and answers `status` (201 when unset).
#[derive(Debug, Default)]
pub(crate) struct Recorder {
    sent: Mutex<Vec<Push>>,
    status: AtomicU16,
}

impl PushSend for Recorder {
    fn send(&self, push: Push) -> BoxFuture<'static, Result<u16, String>> {
        self.sent.lock().expect("lock").push(push);
        let status = match self.status.load(Ordering::SeqCst) {
            0 => 201,
            s => s,
        };
        Box::pin(async move { Ok(status) })
    }
}

impl Recorder {
    fn sent(&self) -> Vec<Push> {
        self.sent.lock().expect("lock").clone()
    }

    async fn wait_for(&self, n: usize) -> Vec<Push> {
        while self.sent.lock().expect("lock").len() < n {
            tokio::task::yield_now().await;
        }
        self.sent()
    }
}

/// A phone's push key pair and secret.
struct Phone {
    private: EphemeralPrivateKey,
    public: Vec<u8>,
    auth: Vec<u8>,
}

impl Phone {
    fn new() -> Self {
        let private = EphemeralPrivateKey::generate(&ECDH_P256, &SystemRandom::new()).expect("key");
        let public = private
            .compute_public_key()
            .expect("public")
            .as_ref()
            .to_vec();
        Self {
            private,
            public,
            auth: b"sixteen byte sec".to_vec(),
        }
    }

    fn subscription(&self, endpoint: &str) -> Subscription {
        Subscription {
            endpoint: endpoint.into(),
            keys: Keys {
                p256dh: URL_SAFE_NO_PAD.encode(&self.public),
                auth: URL_SAFE_NO_PAD.encode(&self.auth),
            },
        }
    }

    fn read(self, body: &[u8]) -> Value {
        let as_public = &body[21..86];
        let plain = ring::agreement::agree_ephemeral(
            self.private,
            &UnparsedPublicKey::new(&ECDH_P256, as_public),
            |secret| open(secret, &self.public, &self.auth, body),
        )
        .expect("agree");
        serde_json::from_slice(&plain).expect("json")
    }
}

struct Setup {
    _dir: tempfile::TempDir,
    shared: Arc<Shared>,
    core: Core,
    recorder: Arc<Recorder>,
    stop: CancellationToken,
}

/// Pushes running for phones `away` (not connected) and `here`
/// (connected), both subscribed.
fn setup(away: &Phone, here: &Phone) -> Setup {
    let dir = tempfile::tempdir().expect("dir");
    let assets: Assets = Arc::new(|_: &str| None::<Asset>);
    let recorder = Arc::new(Recorder::default());
    let server = RemoteServer::with_push(dir.path(), assets, recorder.clone());
    let shared = server.shared.clone();
    for (id, phone) in [("away", away), ("here", here)] {
        let endpoint = format!("https://fcm.googleapis.com/fcm/send/{id}");
        let device = Stored {
            id: id.into(),
            name: id.into(),
            key: "k".into(),
            paired_at_ms: crate::now_ms(),
            seen_at_ms: None,
            push: Some(phone.subscription(&endpoint)),
        };
        shared.lock().devices.add(device).expect("add");
    }
    shared.lock().connected.push("here".into());
    let core = Core::new("test");
    let vapid = Arc::new(Vapid::load_or_create(dir.path()).expect("vapid"));
    let stop = CancellationToken::new();
    tokio::spawn(run(shared.clone(), core.subscribe(), vapid, stop.clone()));
    Setup {
        _dir: dir,
        shared,
        core,
        recorder,
        stop,
    }
}

fn permission_requested() -> Event {
    let request = serde_json::from_value(json!({
        "id": "p1", "session": "s1", "project": "pr1", "tool": "Bash",
        "input": {"command": "rm -rf secret-project"}, "title": "Delete the secret project",
    }))
    .expect("request");
    Event::Permissions(PermissionsEvent::Requested(Box::new(request)))
}

fn failed() -> Event {
    Event::Agents(AgentsEvent::Status {
        session: SessionId("s1".into()),
        project: ProjectId("pr1".into()),
        status: AgentStatus::Failed {
            message: "secret failure".into(),
        },
    })
}

#[tokio::test(start_paused = true)]
async fn a_request_pushes_only_a_kind_and_id_to_a_phone_away() {
    let (away, here) = (Phone::new(), Phone::new());
    let s = setup(&away, &here);
    s.core.emit(permission_requested());
    let sent = s.recorder.wait_for(1).await;
    // A second kind gets through too, so the first push was the only one.
    s.core.emit(failed());
    let sent_after = s.recorder.wait_for(2).await;
    assert_eq!(sent_after.len(), 2);
    assert!(
        sent_after.iter().all(|p| p.endpoint.ends_with("/away")),
        "not to a connected phone"
    );

    let push = &sent[0];
    assert_eq!(push.host, "fcm.googleapis.com");
    let header = |name: &str| {
        push.headers
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.clone())
            .expect("header")
    };
    assert_eq!(header("TTL"), "3600");
    assert_eq!(header("Urgency"), "high");
    assert_eq!(header("Content-Encoding"), "aes128gcm");
    assert_eq!(header("Content-Type"), "application/octet-stream");
    assert!(header("Authorization").starts_with("vapid t="));
    let body = String::from_utf8_lossy(&push.body);
    assert!(!body.contains("secret"), "the body is encrypted");

    let message = away.read(&push.body);
    let keys: Vec<_> = message
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect();
    assert_eq!(keys, ["id", "kind"], "nothing from the event");
    assert_eq!(message["kind"], "approval");
    assert_eq!(b64(message["id"].as_str().expect("id")).len(), 32);
    s.stop.cancel();
}

#[tokio::test(start_paused = true)]
async fn a_phone_hears_one_push_per_kind_in_30_seconds() {
    let (away, here) = (Phone::new(), Phone::new());
    let s = setup(&away, &here);
    s.core.emit(permission_requested());
    s.core.emit(permission_requested());
    s.core.emit(failed());
    let sent = s.recorder.wait_for(2).await;
    tokio::time::advance(Duration::from_secs(31)).await;
    s.core.emit(permission_requested());
    s.recorder.wait_for(3).await;
    assert_eq!(sent.len(), 2);
    assert_eq!(s.recorder.sent().len(), 3);
    s.stop.cancel();
}

#[tokio::test(start_paused = true)]
async fn a_gone_subscription_is_dropped() {
    let (away, here) = (Phone::new(), Phone::new());
    let s = setup(&away, &here);
    s.recorder.status.store(410, Ordering::SeqCst);
    s.core.emit(failed());
    s.recorder.wait_for(1).await;
    let gone = || {
        s.shared
            .lock()
            .devices
            .get("away")
            .is_some_and(|d| d.push.is_none())
    };
    while !gone() {
        tokio::task::yield_now().await;
    }
    assert!(
        s.shared
            .lock()
            .devices
            .get("here")
            .and_then(|d| d.push.clone())
            .is_some()
    );
    s.stop.cancel();
}

#[test]
fn throttle_allows_30_pushes_an_hour() {
    let mut throttle = Throttle::default();
    let start = Instant::now();
    let at = |s: u64| start + Duration::from_secs(s);
    assert!(throttle.take("d", Kind::Approval, at(0)));
    assert!(!throttle.take("d", Kind::Approval, at(29)));
    assert!(
        throttle.take("d", Kind::Failed, at(29)),
        "kinds count apart"
    );
    assert!(
        throttle.take("e", Kind::Approval, at(29)),
        "phones count apart"
    );
    let mut sent = 2;
    let mut t = 30;
    while throttle.take("d", Kind::Approval, at(t)) {
        sent += 1;
        t += 30;
    }
    assert_eq!(sent, PER_HOUR);
    assert!(t < 3600);
    assert!(
        throttle.take("d", Kind::Approval, at(3600)),
        "the hour moved on"
    );
}

#[test]
fn review_changes_do_not_push() {
    // Changed comes for every file change, Keep and Discard: not a signal
    // that work waits for the person.
    let changed = Event::Review(ReviewEvent::Changed {
        project: ProjectId("p".into()),
        id: ReviewId("r1".into()),
    });
    assert_eq!(kind_of(&changed), None);
    let working = Event::Agents(AgentsEvent::Status {
        session: SessionId("s".into()),
        project: ProjectId("p".into()),
        status: AgentStatus::Working,
    });
    assert_eq!(kind_of(&working), None);
}

#[test]
fn only_push_services_are_subscribed() {
    let phone = Phone::new();
    let ok = |endpoint: &str| phone.subscription(endpoint).check().is_ok();
    assert!(ok("https://fcm.googleapis.com/fcm/send/abc"));
    assert!(ok("https://web.push.apple.com/QGuQ"));
    assert!(ok("https://updates.push.services.mozilla.com/wpush/v2/x"));
    assert!(ok("https://wns2-db5p.notify.windows.com/w/?token=x"));
    for bad in [
        "http://fcm.googleapis.com/fcm/send/abc",
        "https://user@fcm.googleapis.com/x",
        "https://fcm.googleapis.com:443/x",
        "https://fcm.googleapis.com:8443/x",
        "https://fcm.googleapis.com.evil.example/x",
        "https://push.apple.com/x",
        "https://evilpush.apple.com/x",
        "https://192.168.1.1/x",
        "https://localhost/x",
        "fcm.googleapis.com/x",
        "",
    ] {
        assert!(!ok(bad), "{bad}");
    }
    let long = format!("https://fcm.googleapis.com/{}", "a".repeat(MAX_ENDPOINT));
    assert!(!ok(&long));

    let mut sub = phone.subscription("https://fcm.googleapis.com/x");
    sub.keys.auth = URL_SAFE_NO_PAD.encode([0u8; 15]);
    assert!(sub.check().is_err());
    let mut sub = phone.subscription("https://fcm.googleapis.com/x");
    sub.keys.p256dh = URL_SAFE_NO_PAD.encode([4u8; 33]);
    assert!(sub.check().is_err());
    sub.keys.p256dh = "not base64!".into();
    assert!(sub.check().is_err());
}
