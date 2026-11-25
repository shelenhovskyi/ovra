//! Dev servers in sandbox pods, one pod per work dir
//! (`docs/specs/servers-and-browser.md`, ADR 0008).
//! Contract: `ovra_core::services::DevServers`.
//!
//! Pod mode ([`pod_init_if_asked`]) is built; until `PodServers` drives
//! pods, every server answers `unavailable`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ovra_core::protocol::servers::ServerId;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{
    BoxFuture, DevServerSpec, DevServers, PodConn, PodSpec, Result, SandboxStatus, ServerSink,
};

#[cfg(unix)]
mod control;
#[cfg(unix)]
mod init;

const NOT_YET: &str = "managed servers are not built yet";

/// If this process was started as a server pod, runs it and never
/// returns. Otherwise returns at once. Call it first in `main`, before any
/// threads start, like `ovra_sandbox::helper_if_asked`.
pub fn pod_init_if_asked() {
    #[cfg(unix)]
    init::pod_init_if_asked();
}

/// The real dev servers.
#[derive(Debug, Default)]
pub struct PodServers {}

impl PodServers {
    pub fn new() -> Self {
        Self {}
    }
}

fn unavailable<T>() -> Result<T> {
    Err(ProtocolError::new(ErrorCode::Unavailable, NOT_YET))
}

impl DevServers for PodServers {
    fn status(&self) -> SandboxStatus {
        SandboxStatus::Unavailable {
            reason: NOT_YET.into(),
        }
    }

    fn start(&self, _: &PodSpec, _: DevServerSpec, _: Arc<dyn ServerSink>) -> Result<()> {
        unavailable()
    }

    fn stop(&self, _: &ServerId, _: Duration) {}

    fn connect(&self, _: PathBuf, _: u16) -> BoxFuture<'_, Result<PodConn>> {
        Box::pin(async { unavailable() })
    }
}

#[cfg(test)]
mod tests {
    use ovra_core::protocol::projects::ProjectId;
    use ovra_core::protocol::servers::ServerStatus;
    use ovra_core::services::{CommandLine, ProfileNetwork, SandboxProfile};

    use super::*;

    #[derive(Debug)]
    struct Ignore;

    impl ServerSink for Ignore {
        fn output(&self, _: &ServerId, _: u64, _: Vec<String>) {}
        fn status(&self, _: &ServerId, _: u64, _: ServerStatus) {}
    }

    #[test]
    fn every_start_answers_unavailable() {
        let servers = PodServers::new();
        assert!(matches!(
            servers.status(),
            SandboxStatus::Unavailable { .. }
        ));
        let pod = PodSpec {
            project: ProjectId("p1".into()),
            work_dir: "/w".into(),
            profile: SandboxProfile {
                work_dir: "/w".into(),
                read: Vec::new(),
                write: Vec::new(),
                expose: vec![5173],
                forward: Vec::new(),
                network: ProfileNetwork::Blocked,
            },
        };
        let server = DevServerSpec {
            id: ServerId("srv1".into()),
            generation: 1,
            line: CommandLine {
                program: "pnpm".into(),
                args: vec!["dev".into()],
                env: Vec::new(),
                cwd: "/w".into(),
            },
            port: 5173,
            ready: None,
        };
        let started = servers.start(&pod, server, Arc::new(Ignore));
        assert_eq!(started.err().map(|e| e.code), Some(ErrorCode::Unavailable));
    }
}
