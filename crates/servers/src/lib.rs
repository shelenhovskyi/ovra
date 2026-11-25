//! Dev servers in sandbox pods, one pod per work dir
//! (`docs/specs/servers-and-browser.md`, ADR 0008).
//! Contract: `ovra_core::services::DevServers`.
//!
//! A pod is this program itself in pod mode ([`pod_init_if_asked`]),
//! started through the `Sandbox` with the pod's profile. Ovra drives it
//! over `<pod dir>/control.sock` (`control`); each server port is reached
//! through `<pod dir>/<port>.sock`, never through a host port. Without a
//! sandbox (or a Unix system) nothing runs.

use std::path::{Path, PathBuf};
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

/// The longest socket path a pod may have: Unix socket paths end at about
/// 100 bytes (108 on Linux, 104 on macOS, with the final zero).
const MAX_SOCKET_PATH: usize = 99;

/// The longest name of a pod's folder and a socket in it: `/<16 hex>/control.sock`.
const POD_SOCKET_TAIL: usize = 1 + 16 + 1 + "control.sock".len();

/// Makes a new folder only this user can open, for the pods' folders, in a
/// place servers never need to write: `$XDG_RUNTIME_DIR` when it is in
/// `/run` and short enough, else `/var/tmp`. The Linux sandbox hides both
/// (`/run` and `/var/tmp` are empty there). Not the temporary folder:
/// servers could then make nothing right in it, since they may not make
/// entries in any folder above their pod's (`confine`). Fails, naming the
/// limit, if a pod's socket path there would not fit.
pub fn servers_dir() -> std::io::Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute() && dir.starts_with("/run") && dir.is_dir());
    let name = format!("ovra-srv-{:016x}", rand::random::<u64>());
    let dir = runtime
        .map(|base| base.join(&name))
        .filter(|dir| socket_paths_fit(dir))
        .unwrap_or_else(|| Path::new("/var/tmp").join(&name));
    if !socket_paths_fit(&dir) {
        return Err(too_long(&dir));
    }
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&dir)?;
    Ok(dir)
}

/// Whether every socket of a pod in `dir` has a path short enough.
fn socket_paths_fit(dir: &Path) -> bool {
    dir.as_os_str().len() + POD_SOCKET_TAIL <= MAX_SOCKET_PATH
}

fn too_long(dir: &Path) -> std::io::Error {
    std::io::Error::other(format!(
        "{} is too long for the pods' sockets: their paths must stay under {} bytes",
        dir.display(),
        MAX_SOCKET_PATH + 1
    ))
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
