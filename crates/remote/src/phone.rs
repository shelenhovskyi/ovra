//! `phone.*`: what a phone asks of the listener itself, answered here and
//! never passed to the core. Each acts on the signed-in device only, never
//! on an id the phone sends.
//!
//! - `phone.forget`: forgets this phone; the session closes after the reply.

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_transport::ServerFrame;
use serde_json::{Value, json};

use crate::Shared;

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
        other => error(
            Some(id),
            ErrorCode::BadRequest,
            format!("unknown method {other}"),
        ),
    })
}
