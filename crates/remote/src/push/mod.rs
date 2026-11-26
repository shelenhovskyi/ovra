//! Web Push without content (spec section 6): the VAPID key that names
//! this install to push services, and the subscriptions phones ask for.

mod crypto;

use axum::http::Uri;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

pub(crate) use crypto::Vapid;

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

#[cfg(test)]
mod tests;
