//! Language servers for trusted projects (ADR 0007, `docs/specs/lsp.md`):
//! finds the servers Ovra knows on `PATH`, starts them through the
//! sandbox, and frames their JSON-RPC messages. Trust, URIs and the
//! requests a server makes are the core's (`crates/core/src/lsp/`).
//! Contract: `ovra_core::services::LanguageServers`; fake:
//! `ovra_core::fakes::FakeLanguageServers`.

use std::sync::Arc;

use ovra_core::protocol::lsp::KnownServer;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{
    BoxFuture, LanguageServers, Result, Sandbox, SandboxStatus, ServerHost, ServerSpec,
    StartedServer,
};

/// The language servers of this machine. A stub until task L.A: it knows
/// no server yet.
#[derive(Debug)]
pub struct Servers {
    sandbox: Arc<dyn Sandbox>,
}

impl Servers {
    pub fn new(sandbox: Arc<dyn Sandbox>) -> Self {
        Self { sandbox }
    }
}

impl LanguageServers for Servers {
    fn known(&self) -> BoxFuture<'_, Vec<KnownServer>> {
        Box::pin(async { Vec::new() })
    }

    fn sandbox(&self) -> Option<String> {
        match self.sandbox.status() {
            SandboxStatus::Available { name } => Some(name),
            SandboxStatus::Unavailable { .. } => None,
        }
    }

    fn start(&self, spec: ServerSpec, _host: ServerHost) -> BoxFuture<'_, Result<StartedServer>> {
        Box::pin(async move {
            Err(ProtocolError::new(
                ErrorCode::NotFound,
                format!("no language server for {}", spec.language),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use ovra_core::fakes::FakeSandbox;
    use ovra_core::protocol::projects::ProjectId;

    use super::*;

    #[tokio::test]
    async fn names_the_sandbox_and_starts_nothing_yet() {
        let servers = Servers::new(Arc::new(FakeSandbox::available()));
        assert_eq!(servers.sandbox().as_deref(), Some("fake"));
        let spec = ServerSpec {
            project: ProjectId("p1".into()),
            root: ovra_core::test_path!("/work/a").into(),
            language: "rust".into(),
        };
        let (host, _) = ServerHost::recording();
        let code = servers.start(spec, host).await.err().map(|e| e.code);
        assert_eq!(code, Some(ErrorCode::NotFound));
    }
}
