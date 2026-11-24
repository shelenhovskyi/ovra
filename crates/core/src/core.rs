use tokio::sync::broadcast;

use crate::protocol::{CoreInfo, Event, Request, Response};

const EVENT_CAPACITY: usize = 256;

/// The Ovra core. Cheap to clone; clones share state.
#[derive(Debug, Clone)]
pub struct Core {
    mode: String,
    events: broadcast::Sender<Event>,
}

impl Core {
    /// Creates a core for the given front-end mode (`tauri`, `headless`, ...).
    pub fn new(mode: impl Into<String>) -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Self {
            mode: mode.into(),
            events,
        }
    }

    /// Handles one request.
    pub fn handle(&self, request: Request) -> Response {
        match request {
            Request::GetInfo => Response::Info(CoreInfo {
                name: "ovra".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                mode: self.mode.clone(),
            }),
        }
    }

    /// Subscribes to events pushed by the core.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    /// Pushes an event to every subscriber. Having no subscribers is fine.
    pub fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_info_reports_mode_and_version() {
        let core = Core::new("test");
        let Response::Info(info) = core.handle(Request::GetInfo);
        assert_eq!(info.name, "ovra");
        assert_eq!(info.mode, "test");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn emitted_events_reach_subscribers() {
        let core = Core::new("test");
        let mut rx = core.subscribe();
        core.emit(Event::Ready);
        assert_eq!(rx.recv().await.ok(), Some(Event::Ready));
    }

    #[test]
    fn wire_format_is_stable() {
        insta::assert_json_snapshot!(serde_json::json!({
            "request": Request::GetInfo,
            "event": Event::Ready,
        }));
    }
}
