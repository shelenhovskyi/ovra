//! Runs agent sessions. Claude sessions are `claude -p` processes from the
//! user's own Claude Code installation, driven over stream-json on stdio
//! (ADR 0006, `docs/specs/claude-cli-protocol.md`).
//! Contract: `ovra_core::services::AgentRuntime`; fake:
//! `ovra_core::fakes::ScriptedAgent`.
//!
//! Stub: starting a session answers `unavailable` until task 2.C lands.

use std::path::PathBuf;

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{AgentHost, AgentRuntime, AgentSpec, Result, RunningAgent};

/// Starts Claude sessions with the user's `claude` executable.
#[derive(Debug)]
pub struct ClaudeCli {
    /// `claude` (looked up on `PATH`) unless the user set a path.
    executable: PathBuf,
}

impl ClaudeCli {
    pub fn new(executable: Option<PathBuf>) -> Self {
        Self {
            executable: executable.unwrap_or_else(|| PathBuf::from("claude")),
        }
    }
}

impl AgentRuntime for ClaudeCli {
    fn start(&self, _: AgentSpec, _: AgentHost) -> Result<Box<dyn RunningAgent>> {
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            format!(
                "running {} is not implemented yet (task 2.C)",
                self.executable.display()
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ovra_core::Core;
    use ovra_core::protocol::Request;

    use super::*;

    #[tokio::test]
    async fn a_failed_start_is_reported_and_frees_the_checkout() {
        let core = Core::builder("test")
            .projects(Arc::new(ovra_core::fakes::MemoryProjects::default()))
            .agents(Arc::new(ClaudeCli::new(None)))
            .build();
        let call = |json: serde_json::Value| {
            let core = core.clone();
            async move {
                let request: Request =
                    serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
                core.handle(request).await
            }
        };
        let add = serde_json::json!({"method": "projects.add", "params": {"root": "/w"}});
        assert!(call(add).await.is_ok());
        let start = serde_json::json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "hi"}});
        for _ in 0..2 {
            let err = call(start.clone()).await.err().map(|e| e.code);
            assert_eq!(err, Some(ErrorCode::Unavailable));
        }
    }
}
