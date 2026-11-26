//! Paired phones, kept in `devices.json`: a name, the phone's P-256 public
//! key and when it was paired and last seen. At most [`MAX_DEVICES`];
//! a phone not seen for [`FORGET_AFTER_MS`] is forgotten.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub(crate) use crate::push::Subscription;

pub(crate) const MAX_DEVICES: usize = 5;
pub(crate) const FORGET_AFTER_MS: u64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Stored {
    pub(crate) id: String,
    pub(crate) name: String,
    /// The uncompressed SEC1 point (65 bytes), base64url.
    pub(crate) key: String,
    pub(crate) paired_at_ms: u64,
    #[serde(default)]
    pub(crate) seen_at_ms: Option<u64>,
    /// Where Web Push reaches this phone, if it asked for notifications.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) push: Option<Subscription>,
}

#[derive(Debug)]
pub(crate) struct Devices {
    path: PathBuf,
    list: Vec<Stored>,
}

impl Devices {
    /// Loads the list; a missing or unreadable file is an empty list.
    pub(crate) fn load(path: PathBuf, now_ms: u64) -> Self {
        let list = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut devices = Self { path, list };
        devices.expire(now_ms);
        devices
    }

    pub(crate) fn list(&self) -> &[Stored] {
        &self.list
    }

    pub(crate) fn get(&self, id: &str) -> Option<&Stored> {
        self.list.iter().find(|d| d.id == id)
    }

    /// Adds a phone; the oldest one goes when there are already
    /// [`MAX_DEVICES`]. Returns the ids of the phones that went.
    pub(crate) fn add(&mut self, device: Stored) -> Result<Vec<String>, String> {
        let mut gone = Vec::new();
        while self.list.len() >= MAX_DEVICES {
            let oldest = self
                .list
                .iter()
                .enumerate()
                .min_by_key(|(_, d)| d.seen_at_ms.unwrap_or(d.paired_at_ms))
                .map(|(i, _)| i)
                .unwrap_or(0);
            gone.push(self.list.remove(oldest).id);
        }
        self.list.push(device);
        self.save().map(|()| gone)
    }

    pub(crate) fn forget(&mut self, id: &str) -> Result<bool, String> {
        let before = self.list.len();
        self.list.retain(|d| d.id != id);
        if self.list.len() == before {
            return Ok(false);
        }
        self.save().map(|()| true)
    }

    /// Sets or clears `id`'s push subscription; `false` when there is no
    /// such phone.
    pub(crate) fn set_push(
        &mut self,
        id: &str,
        push: Option<Subscription>,
    ) -> Result<bool, String> {
        let Some(d) = self.list.iter_mut().find(|d| d.id == id) else {
            return Ok(false);
        };
        d.push = push;
        self.save().map(|()| true)
    }

    pub(crate) fn seen(&mut self, id: &str, now_ms: u64) {
        if let Some(d) = self.list.iter_mut().find(|d| d.id == id) {
            d.seen_at_ms = Some(now_ms);
            let _ = self.save();
        }
    }

    /// Forgets phones not seen (or paired) for [`FORGET_AFTER_MS`].
    pub(crate) fn expire(&mut self, now_ms: u64) {
        let before = self.list.len();
        self.list.retain(|d| {
            now_ms.saturating_sub(d.seen_at_ms.unwrap_or(d.paired_at_ms)) < FORGET_AFTER_MS
        });
        if self.list.len() != before {
            let _ = self.save();
        }
    }

    /// Only this user may read the file: it holds the push secrets.
    fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.list).map_err(|e| e.to_string())?;
        crate::ca::write_private(&self.path, &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phone(id: &str, at: u64) -> Stored {
        Stored {
            id: id.into(),
            name: format!("phone {id}"),
            key: "k".into(),
            paired_at_ms: at,
            seen_at_ms: None,
            push: None,
        }
    }

    #[test]
    fn phones_are_kept_capped_and_forgotten_when_unused() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("devices.json");
        let mut devices = Devices::load(path.clone(), 0);
        for n in 0..5 {
            assert!(
                devices
                    .add(phone(&format!("d{n}"), n))
                    .expect("add")
                    .is_empty()
            );
        }
        assert_eq!(devices.add(phone("d5", 5)).expect("add"), ["d0"]);
        assert_eq!(devices.list().len(), MAX_DEVICES);
        assert!(devices.get("d0").is_none(), "the oldest went");
        devices.seen("d1", FORGET_AFTER_MS);
        let sub: Subscription = serde_json::from_value(serde_json::json!({
            "endpoint": "https://fcm.googleapis.com/x", "keys": {"p256dh": "p", "auth": "a"},
        }))
        .expect("subscription");
        assert!(devices.set_push("d1", Some(sub.clone())).expect("push"));
        assert!(!devices.set_push("nobody", None).expect("push"));
        assert!(devices.forget("d2").expect("forget"));
        assert!(!devices.forget("d2").expect("forget"));

        let kept = Devices::load(path.clone(), FORGET_AFTER_MS + 10);
        let ids: Vec<_> = kept.list().iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["d1"], "only the phone seen recently is kept");
        assert_eq!(kept.list()[0].push, Some(sub), "its subscription is kept");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("file").permissions().mode();
            assert_eq!(mode & 0o077, 0, "the push secrets are readable by others");
        }
    }
}
