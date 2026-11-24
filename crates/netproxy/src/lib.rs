//! Per-session HTTP proxies with a host allowlist: the only way out of an
//! agent sandbox (`docs/specs/security.md`).
//!
//! Each session gets its own proxy on a Unix socket (a 127.0.0.1 port on
//! Windows), which the platform's sandbox helper bridges to a local port
//! inside the sandbox. A proxy takes CONNECT (any TLS) and plain `http://`
//! requests, and lets through only hosts in the base list (what `claude`
//! needs) or the project's allowlist, on 443 and 80 unless an entry names
//! a port. It resolves names itself and refuses loopback, private and
//! link-local addresses whatever the name. Every refusal goes to the core.
//! Without TLS inspection it cannot see inside a tunnel.
//!
//! Contract: `ovra_core::services::NetProxy`; fake:
//! `ovra_core::fakes::FakeNetProxy`.

mod base;
mod dial;
mod rules;
mod serve;
mod upstream;
mod url;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use ovra_core::protocol::agents::SessionId;
use ovra_core::protocol::netproxy::Allowlist;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, NetProxy, NetReporter, Result};
use tokio::sync::Semaphore;
use tokio::task::{JoinHandle, JoinSet};

use crate::dial::Dial;
use crate::rules::{Rule, Rules};
use crate::serve::{SessionNet, Shared};
use crate::upstream::Upstream;

/// Connections one session's proxy serves at once; more wait.
const MAX_CONNECTIONS: usize = 128;

/// The netproxies of every session.
#[derive(Debug)]
pub struct NetProxies {
    base: Arc<[Rule]>,
    shared: Arc<Shared>,
    /// A private folder for the sockets (Unix).
    dir: PathBuf,
    next: AtomicU64,
    running: Mutex<BTreeMap<SessionId, Running>>,
}

#[derive(Debug)]
struct Running {
    net: Arc<SessionNet>,
    /// The accept loop; it owns the connections, so aborting it ends them.
    task: JoinHandle<()>,
    socket: Option<PathBuf>,
}

impl Running {
    fn end(self) {
        self.task.abort();
        if let Some(socket) = self.socket {
            let _ = std::fs::remove_file(socket);
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn internal(message: String) -> ProtocolError {
    ProtocolError::new(ErrorCode::Internal, message)
}

impl NetProxies {
    /// Proxies for the environment Ovra runs in: its base list follows the
    /// user's Bedrock, Vertex, Foundry or gateway settings, and it chains
    /// to the user's own proxy. Sockets go in a new private folder under
    /// the system's temporary folder.
    pub fn from_env() -> Result<Self> {
        let var = |name: &str| std::env::var(name).ok();
        Self::new(&var, Arc::new(dial::Net))
    }

    fn new(var: &dyn Fn(&str) -> Option<String>, dial: Arc<dyn Dial>) -> Result<Self> {
        Ok(Self {
            base: base::rules(var).into(),
            shared: Arc::new(Shared {
                dial,
                upstream: Upstream::from_env(var),
            }),
            dir: private_dir()?,
            next: AtomicU64::new(1),
            running: Mutex::default(),
        })
    }
}

/// A new folder only this user can open, with a short path: Unix socket
/// paths are limited to about 100 bytes.
fn private_dir() -> Result<PathBuf> {
    let tmp = std::env::temp_dir();
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    for attempt in 0..100u32 {
        let dir = tmp.join(format!("ovra-net-{}-{attempt}", std::process::id()));
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(internal(format!("creating {}: {e}", dir.display()))),
        }
    }
    Err(internal("no free folder for the netproxy's sockets".into()))
}

impl Drop for NetProxies {
    fn drop(&mut self) {
        for (_, running) in std::mem::take(&mut *lock(&self.running)) {
            running.end();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl NetProxy for NetProxies {
    fn base(&self) -> Allowlist {
        rules::as_allowlist(&self.base)
    }

    fn start(
        &self,
        session: SessionId,
        allow: Allowlist,
        refusals: NetReporter,
    ) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            let net = Arc::new(SessionNet {
                rules: RwLock::new(Rules {
                    base: self.base.clone(),
                    project: allow.hosts,
                }),
                reporter: refusals,
            });
            let n = self.next.fetch_add(1, Ordering::Relaxed);
            let (listener, address, socket) = bind(&self.dir, n).await?;
            let task = tokio::spawn(accept(listener, self.shared.clone(), net.clone()));
            let old = lock(&self.running).insert(session, Running { net, task, socket });
            if let Some(old) = old {
                old.end();
            }
            Ok(address)
        })
    }

    fn update(&self, session: &SessionId, allow: Allowlist) {
        if let Some(running) = lock(&self.running).get(session) {
            running
                .net
                .rules
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .project = allow.hosts;
        }
    }

    fn stop(&self, session: &SessionId) {
        let running = lock(&self.running).remove(session);
        if let Some(running) = running {
            running.end();
        }
    }
}

#[cfg(unix)]
type Listener = tokio::net::UnixListener;
#[cfg(not(unix))]
type Listener = tokio::net::TcpListener;

/// A Unix socket in `dir`: the address is its path.
#[cfg(unix)]
async fn bind(dir: &std::path::Path, n: u64) -> Result<(Listener, String, Option<PathBuf>)> {
    let path = dir.join(format!("{n}.sock"));
    let listener = Listener::bind(&path)
        .map_err(|e| internal(format!("netproxy socket {}: {e}", path.display())))?;
    Ok((listener, path.to_string_lossy().into_owned(), Some(path)))
}

/// A 127.0.0.1 port: the address is `127.0.0.1:<port>`.
#[cfg(not(unix))]
async fn bind(_dir: &std::path::Path, _n: u64) -> Result<(Listener, String, Option<PathBuf>)> {
    let listener = Listener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| internal(format!("netproxy port: {e}")))?;
    let address = listener
        .local_addr()
        .map_err(|e| internal(format!("netproxy port: {e}")))?
        .to_string();
    Ok((listener, address, None))
}

async fn accept(listener: Listener, shared: Arc<Shared>, net: Arc<SessionNet>) {
    let limit = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut connections = JoinSet::new();
    loop {
        let Ok(permit) = limit.clone().acquire_owned().await else {
            return;
        };
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            Some(_) = connections.join_next(), if !connections.is_empty() => continue,
        };
        let Ok((stream, _)) = accepted else {
            // Out of file descriptors, say: let connections finish.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            continue;
        };
        let (shared, net) = (shared.clone(), net.clone());
        connections.spawn(async move {
            serve::serve(stream, shared, net).await;
            drop(permit);
        });
    }
}

#[cfg(test)]
mod tests;
