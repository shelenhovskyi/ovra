//! `phone.*`: what a phone asks of the listener itself, answered here and
//! never passed to the core. Each acts on the signed-in device only, never
//! on an id the phone sends.
//!
//! - `phone.forget`: forgets this phone; the session closes after the reply.
//! - `phone.push_key`: `{"key"}`, the VAPID public key for `subscribe()`.
//! - `phone.push_subscribe {subscription}`: keeps the browser's
//!   `PushSubscription` (or with `null` drops it).

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_transport::ServerFrame;
use serde_json::{Value, json};

use crate::Shared;
use crate::push::Subscription;

/// A reply to send, and whether the session ends after it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Reply {
    pub(crate) text: String,
    pub(crate) close: bool,
}

fn error(id: Option<u64>, code: ErrorCode, message: impl Into<String>) -> Reply {
    Reply {
        text: ServerFrame::Error {
            id,
            error: ProtocolError::new(code, message),
        }
        .to_json(),
        close: false,
    }
}

fn result(id: u64, result: Value) -> Reply {
    Reply {
        text: json!({"id": id, "result": result}).to_string(),
        close: false,
    }
}

/// Answers `text` if it is a `phone.*` request; `None` for anything else,
/// which goes to the core.
pub(crate) fn handle(shared: &Shared, device: &str, text: &str) -> Option<Reply> {
    let frame: Value = serde_json::from_str(text).ok()?;
    let method = frame.get("method")?.as_str()?;
    if !method.starts_with("phone.") {
        return None;
    }
    let Some(id) = frame.get("id").and_then(Value::as_u64) else {
        return Some(error(
            None,
            ErrorCode::BadRequest,
            "a request needs a numeric id",
        ));
    };
    Some(match method {
        "phone.forget" => match shared.forget(device) {
            Ok(()) => Reply {
                close: true,
                ..result(id, Value::Null)
            },
            Err(e) => error(Some(id), e.code, e.message),
        },
        "phone.push_key" => {
            let key = shared.lock().running.as_ref().map(|r| r.vapid.public());
            match key {
                Some(key) => result(id, json!({"key": key})),
                None => error(Some(id), ErrorCode::Unavailable, "phone access is off"),
            }
        }
        "phone.push_subscribe" => subscribe(shared, device, id, &frame),
        other => error(
            Some(id),
            ErrorCode::BadRequest,
            format!("unknown method {other}"),
        ),
    })
}

fn subscribe(shared: &Shared, device: &str, id: u64, frame: &Value) -> Reply {
    let Some(given) = frame.pointer("/params/subscription") else {
        return error(
            Some(id),
            ErrorCode::BadRequest,
            "params.subscription is required",
        );
    };
    let subscription = if given.is_null() {
        None
    } else {
        let parsed = serde_json::from_value::<Subscription>(given.clone())
            .map_err(|e| e.to_string())
            .and_then(|s| s.check().map(|()| s).map_err(str::to_owned));
        match parsed {
            Ok(s) => Some(s),
            Err(e) => return error(Some(id), ErrorCode::BadRequest, e),
        }
    };
    let on = subscription.is_some();
    match shared.lock().devices.set_push(device, subscription) {
        Ok(true) => {
            tracing::info!(device, on, "phone notifications changed");
            result(id, Value::Null)
        }
        Ok(false) => error(Some(id), ErrorCode::NotFound, "this phone is not paired"),
        Err(e) => error(Some(id), ErrorCode::Internal, e),
    }
}
