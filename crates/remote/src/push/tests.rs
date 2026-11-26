//! Which subscriptions a phone may keep.

use super::*;

fn subscription(endpoint: &str) -> Subscription {
    let mut p256dh = vec![4u8];
    p256dh.extend([7u8; 64]);
    Subscription {
        endpoint: endpoint.into(),
        keys: Keys {
            p256dh: URL_SAFE_NO_PAD.encode(p256dh),
            auth: URL_SAFE_NO_PAD.encode([1u8; 16]),
        },
    }
}

#[test]
fn only_push_services_are_subscribed() {
    let ok = |endpoint: &str| subscription(endpoint).check().is_ok();
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

    let mut sub = subscription("https://fcm.googleapis.com/x");
    sub.keys.auth = URL_SAFE_NO_PAD.encode([0u8; 15]);
    assert!(sub.check().is_err());
    let mut sub = subscription("https://fcm.googleapis.com/x");
    sub.keys.p256dh = URL_SAFE_NO_PAD.encode([4u8; 33]);
    assert!(sub.check().is_err());
    sub.keys.p256dh = "not base64!".into();
    assert!(sub.check().is_err());
}
