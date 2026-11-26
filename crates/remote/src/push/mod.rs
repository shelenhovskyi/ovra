//! Web Push without content (spec section 6): while the listener runs, a
//! paired phone that asked for notifications and is not connected hears
//! that something needs it. The message, encrypted per RFC 8291, holds only
//! a kind and a random id; the phone opens the app to see what it is.

mod crypto;
mod http;

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use axum::http::Uri;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures_util::future::BoxFuture;
use ovra_core::Core;
use ovra_core::protocol::Event;
use ovra_core::protocol::agents::{AgentStatus, AgentsEvent};
use ovra_core::protocol::permissions::PermissionsEvent;
use ovra_core::protocol::review::{ReviewEvent, ReviewId};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

pub(crate) use crypto::Vapid;
pub(crate) use http::HttpPush;

use crate::Shared;

/// One push per device and kind in this long.
const PER_KIND: Duration = Duration::from_secs(30);
const PER_HOUR: usize = 30;
const HOUR: Duration = Duration::from_secs(60 * 60);
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
const TTL_SECONDS: u64 = 60 * 60;
const TOKEN_LIFE_SECONDS: u64 = 12 * 60 * 60;
const MAX_ENDPOINT: usize = 1024;
/// Push services a subscription may name. The daemon runs outside the
/// sandbox, so an endpoint elsewhere would let a phone make it call any
/// address.
const PUSH_HOST: &str = "fcm.googleapis.com";
const PUSH_DOMAINS: [&str; 3] = [
    ".push.apple.com",
    ".push.services.mozilla.com",
    ".notify.windows.com",
];
/// Review ids remembered, so each new one pushes once.
const MAX_SEEN: usize = 4096;

/// A browser's `PushSubscription`, as its `toJSON()` gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Subscription {
    pub(crate) endpoint: String,
    pub(crate) keys: Keys,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Keys {
    pub(crate) p256dh: String,
    pub(crate) auth: String,
}

fn decode(s: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(s.trim_end_matches('=')).ok()
}

impl Subscription {
    /// Checks what a phone sent; the error says what is wrong.
    pub(crate) fn check(&self) -> Result<(), &'static str> {
        self.host()?;
        if !decode(&self.keys.p256dh).is_some_and(|k| k.len() == 65 && k[0] == 4) {
            return Err("keys.p256dh must be an uncompressed P-256 point, base64url");
        }
        if !decode(&self.keys.auth).is_some_and(|a| a.len() == 16) {
            return Err("keys.auth must be 16 bytes, base64url");
        }
        Ok(())
    }

    /// The endpoint's host, if it is an `https://` URL of a known push
    /// service with nothing but a host for its authority.
    fn host(&self) -> Result<String, &'static str> {
        const BAD: &str = "the endpoint is not a known push service";
        if self.endpoint.len() > MAX_ENDPOINT {
            return Err("the endpoint is too long");
        }
        let uri: Uri = self.endpoint.parse().map_err(|_| BAD)?;
        let (Some(authority), Some(host)) = (uri.authority(), uri.host()) else {
            return Err(BAD);
        };
        // No user info and no port, not even 443.
        if uri.scheme_str() != Some("https") || authority.as_str() != host {
            return Err(BAD);
        }
        let host = host.to_ascii_lowercase();
        let known = host == PUSH_HOST || PUSH_DOMAINS.iter().any(|d| host.ends_with(d));
        known.then_some(host).ok_or(BAD)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Approval,
    Review,
    Failed,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Approval => "approval",
            Self::Review => "review",
            Self::Failed => "failed",
        }
    }
}

/// What an event tells a phone, if anything: a permission request, an
/// agent session that failed, or a review item seen for the first time.
fn kind_of(event: &Event, reviews: &mut HashSet<ReviewId>) -> Option<Kind> {
    match event {
        Event::Permissions(PermissionsEvent::Requested(_)) => Some(Kind::Approval),
        Event::Agents(AgentsEvent::Status {
            status: AgentStatus::Failed { .. },
            ..
        }) => Some(Kind::Failed),
        Event::Review(ReviewEvent::Changed { id, .. }) => {
            if reviews.len() >= MAX_SEEN {
                reviews.clear();
            }
            reviews.insert(id.clone()).then_some(Kind::Review)
        }
        _ => None,
    }
}

/// How often one phone hears from us.
#[derive(Debug, Default)]
struct Throttle {
    last: HashMap<(String, Kind), Instant>,
    sent: HashMap<String, VecDeque<Instant>>,
}

impl Throttle {
    /// Counts one push; `false` when `device` had this kind in the last
    /// [`PER_KIND`] or [`PER_HOUR`] pushes in the last hour.
    fn take(&mut self, device: &str, kind: Kind, now: Instant) -> bool {
        let key = (device.to_owned(), kind);
        if self.last.get(&key).is_some_and(|t| now - *t < PER_KIND) {
            return false;
        }
        let sent = self.sent.entry(device.to_owned()).or_default();
        while sent.front().is_some_and(|t| now - *t >= HOUR) {
            sent.pop_front();
        }
        if sent.len() >= PER_HOUR {
            return false;
        }
        sent.push_back(now);
        self.last.insert(key, now);
        true
    }
}

/// One request to a push service.
#[derive(Debug, Clone)]
pub(crate) struct Push {
    pub(crate) endpoint: String,
    pub(crate) host: String,
    pub(crate) headers: Vec<(&'static str, String)>,
    pub(crate) body: Vec<u8>,
}

/// Delivers a push and answers the push service's HTTP status.
pub(crate) trait PushSend: Send + Sync {
    fn send(&self, push: Push) -> BoxFuture<'static, Result<u16, String>>;
}

fn message(vapid: &Vapid, sub: &Subscription, kind: Kind) -> Result<Push, String> {
    let host = sub.host().map_err(str::to_owned)?;
    let payload = serde_json::json!({"kind": kind.as_str(), "id": crate::auth::random_token()});
    let (Some(ua_public), Some(auth)) = (decode(&sub.keys.p256dh), decode(&sub.keys.auth)) else {
        return Err("bad subscription keys".into());
    };
    let body = crypto::encrypt(&ua_public, &auth, payload.to_string().as_bytes())
        .map_err(|_| "encrypting failed")?;
    let exp = crate::now_ms() / 1000 + TOKEN_LIFE_SECONDS;
    let authorization = vapid
        .authorization(&format!("https://{host}"), exp)
        .map_err(|_| "signing failed")?;
    Ok(Push {
        endpoint: sub.endpoint.clone(),
        host,
        headers: vec![
            ("TTL", TTL_SECONDS.to_string()),
            ("Urgency", "high".into()),
            ("Content-Encoding", "aes128gcm".into()),
            ("Content-Type", "application/octet-stream".into()),
            ("Authorization", authorization),
        ],
        body,
    })
}

/// Pushes to phones until `stop`.
pub(crate) async fn run(
    shared: Arc<Shared>,
    mut events: broadcast::Receiver<Event>,
    vapid: Arc<Vapid>,
    stop: CancellationToken,
) {
    let mut throttle = Throttle::default();
    let mut reviews = HashSet::new();
    loop {
        let event = tokio::select! {
            () = stop.cancelled() => return,
            event = Core::next_event(&mut events) => match event {
                Some(event) => event,
                None => return,
            },
        };
        let Some(kind) = kind_of(&event, &mut reviews) else {
            continue;
        };
        let now = Instant::now();
        let due: Vec<(String, Subscription)> = {
            let state = shared.lock();
            state
                .devices
                .list()
                .iter()
                .filter(|d| !state.connected.contains(&d.id))
                .filter_map(|d| Some((d.id.clone(), d.push.clone()?)))
                .filter(|(id, _)| throttle.take(id, kind, now))
                .collect()
        };
        for (device, sub) in due {
            tokio::spawn(deliver(shared.clone(), vapid.clone(), device, sub, kind));
        }
    }
}

async fn deliver(
    shared: Arc<Shared>,
    vapid: Arc<Vapid>,
    device: String,
    sub: Subscription,
    kind: Kind,
) {
    let push = match message(&vapid, &sub, kind) {
        Ok(push) => push,
        Err(e) => {
            tracing::warn!(device, error = %e, "push not sent");
            return;
        }
    };
    // The endpoint is a capability URL: only its host goes to the log.
    let host = push.host.clone();
    match tokio::time::timeout(SEND_TIMEOUT, shared.push.send(push)).await {
        Ok(Ok(404 | 410)) => {
            let mut state = shared.lock();
            let same = state
                .devices
                .get(&device)
                .and_then(|d| d.push.as_ref())
                .is_some_and(|p| p.endpoint == sub.endpoint);
            if same {
                let _ = state.devices.set_push(&device, None);
            }
            tracing::info!(device, host, "push subscription gone");
        }
        Ok(Ok(status)) if (200..300).contains(&status) => {
            tracing::debug!(device, host, kind = kind.as_str(), "pushed");
        }
        Ok(Ok(status)) => tracing::warn!(device, host, status, "push refused"),
        Ok(Err(e)) => tracing::warn!(device, host, error = %e, "push failed"),
        Err(_) => tracing::warn!(device, host, "push timed out"),
    }
}

#[cfg(test)]
pub(crate) mod tests;
