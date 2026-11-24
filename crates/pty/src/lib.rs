//! Shells in pseudo-terminals, one per terminal tab.
//! Contract: `ovra_core::services::TerminalService`; fake:
//! `ovra_core::fakes::EchoTerminals`.
//!
//! Stub: every call answers `unavailable` until task 2.A lands.

use std::path::PathBuf;

use ovra_core::protocol::terminals::{TermSize, TerminalId};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{EventSink, Result, TerminalService};

/// The real terminal service.
#[derive(Debug, Default)]
pub struct Pty {}

impl Pty {
    pub fn new() -> Self {
        Self {}
    }
}

fn not_yet<T>() -> Result<T> {
    Err(ProtocolError::new(
        ErrorCode::Unavailable,
        "terminals are not implemented yet (task 2.A)",
    ))
}

impl TerminalService for Pty {
    fn open(&self, _: PathBuf, _: TermSize, _: EventSink) -> Result<TerminalId> {
        not_yet()
    }
    fn input(&self, _: &TerminalId, _: &str) -> Result<()> {
        not_yet()
    }
    fn resize(&self, _: &TerminalId, _: TermSize) -> Result<()> {
        not_yet()
    }
    fn close(&self, _: &TerminalId) -> Result<()> {
        not_yet()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ovra_core::Core;
    use ovra_core::protocol::Request;

    use super::*;

    #[tokio::test]
    async fn answers_unavailable_through_the_core() {
        let core = Core::builder("test")
            .projects(Arc::new(ovra_core::fakes::MemoryProjects::default()))
            .terminals(Arc::new(Pty::new()))
            .build();
        let add = serde_json::json!({"method": "projects.add", "params": {"root": "/w"}});
        let open = serde_json::json!({"method": "terminals.open",
            "params": {"project": "p1", "size": {"cols": 80, "rows": 24}}});
        let mut results = Vec::new();
        for json in [add, open] {
            let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
            results.push(core.handle(request).await.err().map(|e| e.code));
        }
        assert_eq!(results, [None, Some(ErrorCode::Unavailable)]);
    }
}
