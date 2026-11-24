use ovra_core::Core;
use ovra_core::protocol::{ErrorCode, Event, ProtocolError, Request, Response};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct ClientFrame {
    id: u64,
    #[serde(flatten)]
    request: Request,
}

/// A frame sent from the core to a front end.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ServerFrame {
    Result {
        id: u64,
        result: Response,
    },
    Error {
        id: Option<u64>,
        error: ProtocolError,
    },
    Event {
        event: Event,
    },
}

impl ServerFrame {
    /// Serializes the frame. Serializing these types cannot fail.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| String::from("{}"))
    }
}

/// Parses one client frame, runs it on the core and returns the reply frame.
pub fn dispatch(core: &Core, text: &str) -> String {
    let frame = match serde_json::from_str::<ClientFrame>(text) {
        Ok(frame) => ServerFrame::Result {
            id: frame.id,
            result: core.handle(frame.request),
        },
        Err(err) => ServerFrame::Error {
            id: serde_json::from_str::<serde_json::Value>(text)
                .ok()
                .and_then(|v| v.get("id")?.as_u64()),
            error: ProtocolError {
                code: ErrorCode::BadRequest,
                message: err.to_string(),
            },
        },
    };
    frame.to_json()
}

/// Wraps an event in a frame.
pub fn event_frame(event: Event) -> String {
    ServerFrame::Event { event }.to_json()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_info_round_trip() {
        let core = Core::new("test");
        let reply = dispatch(&core, r#"{"id":7,"method":"get_info"}"#);
        let value: serde_json::Value = serde_json::from_str(&reply).unwrap_or_default();
        insta::assert_json_snapshot!(value, {".result.data.version" => "[version]"});
    }

    #[test]
    fn unknown_method_is_bad_request_with_id() {
        let core = Core::new("test");
        let reply = dispatch(&core, r#"{"id":3,"method":"nope"}"#);
        let value: serde_json::Value = serde_json::from_str(&reply).unwrap_or_default();
        assert_eq!(value["id"], 3);
        assert_eq!(value["error"]["code"], "bad_request");
    }

    #[test]
    fn garbage_is_bad_request_without_id() {
        let core = Core::new("test");
        let reply = dispatch(&core, "not json");
        let value: serde_json::Value = serde_json::from_str(&reply).unwrap_or_default();
        assert!(value["id"].is_null());
        assert_eq!(value["error"]["code"], "bad_request");
    }

    #[test]
    fn event_frame_shape() {
        assert_eq!(event_frame(Event::Ready), r#"{"event":{"type":"ready"}}"#);
    }
}
