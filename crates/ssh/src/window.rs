//! What the app needs to keep a remote machine's window apart from the
//! laptop's (`docs/specs/ssh-remotes.md`, "Bridge and remote windows"):
//! its label, which frames go where, which links it may open, and its own
//! webview storage. Here rather than in the app so that it is tested.

use std::net::{Ipv4Addr, Ipv6Addr};

use ovra_core::ssh::HostId;

const LABEL_PREFIX: &str = "ssh-";

/// The window label of machine `id`: `ssh-<id>`.
pub fn label(id: &HostId) -> String {
    format!("{LABEL_PREFIX}{}", id.as_str())
}

/// The machine a window label names, if it is a remote window's.
pub fn machine(label: &str) -> Option<HostId> {
    HostId::parse(label.strip_prefix(LABEL_PREFIX)?).ok()
}

/// Where a checked frame from the remote core goes.
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// To the call waiting for this id, if the window sent it.
    Reply(u64),
    /// To the machine's window only.
    Event,
    /// Nowhere.
    Drop,
}

pub fn route(frame: &str) -> Route {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(frame) else {
        return Route::Drop;
    };
    if value.get("event").is_some() {
        return Route::Event;
    }
    match value.get("id").and_then(serde_json::Value::as_u64) {
        Some(id) => Route::Reply(id),
        None => Route::Drop,
    }
}

/// The id of a frame the window sends.
pub fn request_id(frame: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(frame)
        .ok()?
        .get("id")?
        .as_u64()
}

/// The answer to call `id` when the machine cannot give one.
pub fn unavailable(id: u64, message: &str) -> String {
    serde_json::json!({
        "id": id,
        "error": {"code": "unavailable", "message": message},
    })
    .to_string()
}

/// `url` if a remote window may open it in the laptop's browser: `http` or
/// `https`, with a host that is not this computer or its network. Those
/// would point at the laptop's own services, never the remote machine's.
/// A public name that resolves to such an address is not caught.
pub fn may_open(url: &str) -> Option<url::Url> {
    let parsed = url::Url::parse(url).ok()?;
    let userinfo = !parsed.username().is_empty() || parsed.password().is_some();
    if !matches!(parsed.scheme(), "http" | "https") || userinfo {
        return None;
    }
    let local = match parsed.host()? {
        url::Host::Domain(name) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost") || !name.contains('.')
        }
        url::Host::Ipv4(ip) => local_v4(ip),
        url::Host::Ipv6(ip) => local_v6(ip),
    };
    (!local).then_some(parsed)
}

fn local_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        // "This network", which reaches this computer on Linux.
        || a == 0
        // Shared address space (carrier NAT, Tailscale).
        || (a == 100 && (64..128).contains(&b))
        // Benchmarking networks.
        || (a == 198 && (18..20).contains(&b))
}

fn local_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    ip.is_loopback()
        || ip.is_unspecified()
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
        || (first & 0xff00) == 0xff00
        // `::ffff:a.b.c.d` and the old `::a.b.c.d`.
        || ip.to_ipv4().is_some_and(local_v4)
        // NAT64 of a local address.
        || (ip.segments()[..6] == [0x64, 0xff9b, 0, 0, 0, 0] && local_v4(nat64(ip)))
}

fn nat64(ip: Ipv6Addr) -> Ipv4Addr {
    let [.., a, b, c, d] = ip.octets();
    Ipv4Addr::new(a, b, c, d)
}

/// A webview data store id of the machine's own (macOS), so its drafts and
/// preferences never mix with the laptop's or another machine's.
pub fn data_store(id: &HostId) -> [u8; 16] {
    // FNV-1a, 128 bits: stable across runs and builds.
    let mut hash: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    for byte in b"ovra-ssh-window\0".iter().chain(id.as_str().as_bytes()) {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
    }
    hash.to_be_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> HostId {
        HostId::parse(text).unwrap_or_else(|e| panic!("{e:?}"))
    }

    #[test]
    fn labels_name_only_remote_windows() {
        assert_eq!(label(&id("abc123")), "ssh-abc123");
        assert_eq!(
            machine("ssh-abc123").map(|h| h.as_str().to_owned()),
            Some("abc123".into())
        );
        for other in ["main", "ssh-", "ssh-ABC", "ssh-a/b", "sshabc"] {
            assert!(machine(other).is_none(), "{other}");
        }
    }

    #[test]
    fn frames_go_to_their_own_window() {
        assert_eq!(route(r#"{"id":4,"result":{}}"#), Route::Reply(4));
        assert_eq!(route(r#"{"event":{"type":"info.ready"}}"#), Route::Event);
        assert_eq!(route(r#"{"id":null,"error":{}}"#), Route::Drop);
        assert_eq!(request_id(r#"{"id":9,"method":"info.get"}"#), Some(9));
        assert_eq!(request_id("{}"), None);
        let reply = unavailable(9, "gone");
        assert_eq!(route(&reply), Route::Reply(9));
    }

    #[test]
    fn only_links_away_from_the_laptop_open() {
        for url in [
            "https://example.org/docs",
            "http://203.0.113.9:8080/",
            "https://[2001:db8::1]/",
        ] {
            assert!(may_open(url).is_some(), "{url}");
        }
        for url in [
            "http://localhost:3000/",
            "http://LOCALHOST./",
            "http://app.localhost/",
            "http://router/",
            "http://127.0.0.1/",
            "http://10.0.0.2/",
            "http://192.168.1.1/",
            "http://172.16.0.1/",
            "http://169.254.169.254/latest/meta-data",
            "http://100.100.100.100/",
            "http://0.0.0.0/",
            "http://[::1]/",
            "http://[fd00::1]/",
            "http://[fe80::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://2130706433/",
            "http://0.1.2.3/",
            "http://224.0.0.1/",
            "http://198.18.0.1/",
            "http://[ff02::1]/",
            "http://[64:ff9b::7f00:1]/",
            "https://:pw@example.org/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://user@example.org/",
        ] {
            assert!(may_open(url).is_none(), "{url}");
        }
    }

    /// A remote window may call only these: no files, folders, drops,
    /// notifications or events of this computer.
    #[test]
    fn the_remote_window_capability_holds_only_its_commands() {
        let file = include_str!("../../../app/capabilities/remote.json");
        let capability: serde_json::Value =
            serde_json::from_str(file).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(capability["windows"], serde_json::json!(["ssh-*"]));
        assert_eq!(
            capability["permissions"],
            serde_json::json!([
                "allow-ovra-call",
                "allow-ovra-subscribe",
                "allow-ovra-remote-reconnect",
                "allow-ovra-remote-disconnect",
                "allow-ovra-remote-stop",
                "allow-ovra-open-url",
                "core:window:allow-start-dragging"
            ])
        );
    }

    /// The main window's list: restart and install of a remote machine
    /// are here, never in the remote window's.
    #[test]
    fn the_main_window_capability_holds_only_its_commands() {
        let file = include_str!("../../../app/capabilities/main.json");
        let capability: serde_json::Value =
            serde_json::from_str(file).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(capability["windows"], serde_json::json!(["main"]));
        assert_eq!(
            capability["permissions"],
            serde_json::json!([
                "allow-ovra-call",
                "allow-ovra-subscribe",
                "allow-ovra-pick-folder",
                "allow-ovra-import-drop",
                "allow-ovra-remote-show",
                "allow-ovra-remote-restart",
                "allow-ovra-remote-install",
                "core:window:allow-start-dragging",
                "notification:allow-is-permission-granted",
                "notification:allow-request-permission",
                "notification:allow-notify"
            ])
        );
    }

    #[test]
    fn each_machine_has_its_own_store() {
        assert_ne!(data_store(&id("abc")), data_store(&id("abd")));
        assert_eq!(data_store(&id("abc")), data_store(&id("abc")));
    }
}
