//! Dev servers in sandbox pods, one pod per work dir
//! (`docs/specs/servers-and-browser.md`, ADR 0008).
//! Contract: `ovra_core::services::DevServers`.
//!
//! A pod is this program itself in pod mode ([`pod_init_if_asked`]),
//! started through the `Sandbox` with the pod's profile. Ovra drives it
//! over `<pod dir>/control.sock` (`control`); each server port is reached
//! through `<pod dir>/<port>.sock`, never through a host port. Without a
//! sandbox (or a Unix system) nothing runs.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ovra_core::protocol::servers::ServerId;
use ovra_core::services::{
    BoxFuture, DevServerSpec, DevServers, PodConn, PodSpec, Result, Sandbox, SandboxStatus,
    ServerSink,
};

#[cfg(target_os = "linux")]
mod confine;
#[cfg(unix)]
mod control;
#[cfg(unix)]
mod init;
#[cfg(unix)]
mod pods;
#[cfg(unix)]
mod ready;
#[cfg(unix)]
mod sockets;

/// If this process was started as a server pod, runs it and never
/// returns. Otherwise returns at once. Call it first in `main`, before any
/// threads start, like `ovra_sandbox::helper_if_asked`.
pub fn pod_init_if_asked() {
    #[cfg(unix)]
    init::pod_init_if_asked();
}

/// The real dev servers.
#[derive(Debug)]
pub struct PodServers {
    #[cfg(unix)]
    pods: pods::Pods,
}

impl PodServers {
    /// Servers in pods made by `sandbox`, with each pod's folder in `dir`
    /// (keep it short: Unix socket paths are limited to about 100 bytes).
    pub fn new(sandbox: Arc<dyn Sandbox>, dir: PathBuf) -> Self {
        #[cfg(not(unix))]
        let _ = (sandbox, dir);
        Self {
            #[cfg(unix)]
            pods: pods::Pods::new(sandbox, dir),
        }
    }

    /// The program that calls [`pod_init_if_asked`] first thing; by
    /// default this process's own executable.
    #[cfg_attr(not(unix), allow(unused_mut))]
    pub fn program(mut self, program: PathBuf) -> Self {
        #[cfg(unix)]
        {
            self.pods.program = program;
        }
        #[cfg(not(unix))]
        let _ = program;
        self
    }
}

#[cfg(unix)]
impl DevServers for PodServers {
    fn status(&self) -> SandboxStatus {
        self.pods.status()
    }

    fn start(&self, pod: &PodSpec, server: DevServerSpec, sink: Arc<dyn ServerSink>) -> Result<()> {
        self.pods.start(pod, server, sink)
    }

    fn stop(&self, id: &ServerId, grace: Duration) {
        self.pods.stop(id, grace);
    }

    fn connect(&self, work_dir: PathBuf, port: u16) -> BoxFuture<'_, Result<PodConn>> {
        Box::pin(self.pods.connect(work_dir, port))
    }
}

#[cfg(not(unix))]
const NO_PODS: &str = "managed servers need a sandbox, and this system has none yet";

#[cfg(not(unix))]
impl DevServers for PodServers {
    fn status(&self) -> SandboxStatus {
        SandboxStatus::Unavailable {
            reason: NO_PODS.into(),
        }
    }

    fn start(&self, _: &PodSpec, _: DevServerSpec, _: Arc<dyn ServerSink>) -> Result<()> {
        Err(unavailable())
    }

    fn stop(&self, _: &ServerId, _: Duration) {}

    fn connect(&self, _: PathBuf, _: u16) -> BoxFuture<'_, Result<PodConn>> {
        Box::pin(async { Err(unavailable()) })
    }
}

#[cfg(not(unix))]
fn unavailable() -> ovra_core::protocol::ProtocolError {
    use ovra_core::protocol::{ErrorCode, ProtocolError};
    ProtocolError::new(ErrorCode::Unavailable, NO_PODS)
}
