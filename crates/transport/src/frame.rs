use ovra_core::Core;
#[cfg(test)]
use ovra_core::protocol::info::InfoEvent;
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
pub async fn dispatch(core: &Core, text: &str) -> String {
    let frame = match serde_json::from_str::<ClientFrame>(text) {
        Ok(frame) => match core.handle(frame.request).await {
            Ok(result) => ServerFrame::Result {
                id: frame.id,
                result,
            },
            Err(error) => ServerFrame::Error {
                id: Some(frame.id),
                error,
            },
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

    #[tokio::test]
    async fn get_info_round_trip() {
        let core = Core::new("test");
        let reply = dispatch(&core, r#"{"id":7,"method":"info.get"}"#).await;
        let value: serde_json::Value = serde_json::from_str(&reply).unwrap_or_default();
        insta::assert_json_snapshot!(value, {".result.data.version" => "[version]"});
    }

    #[tokio::test]
    async fn unknown_method_is_bad_request_with_id() {
        let core = Core::new("test");
        let reply = dispatch(&core, r#"{"id":3,"method":"nope"}"#).await;
        let value: serde_json::Value = serde_json::from_str(&reply).unwrap_or_default();
        assert_eq!(value["id"], 3);
        assert_eq!(value["error"]["code"], "bad_request");
    }

    #[tokio::test]
    async fn garbage_is_bad_request_without_id() {
        let core = Core::new("test");
        let reply = dispatch(&core, "not json").await;
        let value: serde_json::Value = serde_json::from_str(&reply).unwrap_or_default();
        assert!(value["id"].is_null());
        assert_eq!(value["error"]["code"], "bad_request");
    }

    #[tokio::test]
    async fn service_errors_keep_the_request_id() {
        let core = Core::new("test");
        let reply = dispatch(&core, r#"{"id":4,"method":"projects.list"}"#).await;
        let value: serde_json::Value = serde_json::from_str(&reply).unwrap_or_default();
        assert_eq!(value["id"], 4);
        assert_eq!(value["error"]["code"], "unavailable");
    }

    #[test]
    fn event_frame_shape() {
        assert_eq!(
            event_frame(Event::Info(InfoEvent::Ready)),
            r#"{"event":{"type":"info.ready"}}"#
        );
    }
}
