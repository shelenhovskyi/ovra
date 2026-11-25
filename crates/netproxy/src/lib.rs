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
mod ca;
mod dial;
mod inject;
mod rules;
mod serve;
mod tls;
mod upstream;
mod url;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, RwLock};

use ovra_core::protocol::agents::SessionId;
use ovra_core::protocol::netproxy::Allowlist;
use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::secrets::SecretInfo;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, NetProxy, NetReporter, NetStarted, Result, Secrets};
use tokio::sync::Semaphore;
use tokio::task::{JoinHandle, JoinSet};

use crate::ca::SessionCa;
use crate::dial::Dial;
use crate::inject::Bound;
use crate::rules::{Rule, Rules};
use crate::serve::{SessionNet, Shared};
use crate::tls::Roots;
use crate::upstream::Upstream;

/// Connections all proxies serve at once, and one session's proxy; more
/// wait. Keeps agents from using up the daemon's file descriptors.
const MAX_CONNECTIONS: usize = 256;
const MAX_SESSION_CONNECTIONS: usize = 64;

/// The netproxies of every session.
#[derive(Debug)]
pub struct NetProxies {
    base: Arc<[Rule]>,
    shared: Arc<Shared>,
    /// A private folder for the sockets (Unix).
    dir: PathBuf,
    /// Connections all sessions' proxies serve at once.
    connections: Arc<Semaphore>,
    running: Mutex<BTreeMap<SessionId, Running>>,
    /// Where secrets' values are read when the proxy adds them.
    secrets: Arc<dyn Secrets>,
    /// The roots trusted upstream, read when a session first has secrets.
    roots: OnceLock<Roots>,
}

#[derive(Debug)]
struct Running {
    net: Arc<SessionNet>,
    /// The accept loop; it owns the connections, so aborting it ends them.
    task: JoinHandle<()>,
    /// The socket's own folder (Unix).
    socket: Option<PathBuf>,
}

impl Running {
    fn end(self) {
        self.task.abort();
        if let Some(folder) = self.socket {
            let _ = std::fs::remove_dir_all(folder);
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
    /// `secrets` gives the values the proxies add
    /// (`docs/specs/secrets.md`).
    pub fn from_env(secrets: Arc<dyn Secrets>) -> Result<Self> {
        let var = |name: &str| std::env::var(name).ok();
        Self::new(&var, Arc::new(dial::Net), secrets)
    }

    fn new(
        var: &dyn Fn(&str) -> Option<String>,
        dial: Arc<dyn Dial>,
        secrets: Arc<dyn Secrets>,
    ) -> Result<Self> {
        Ok(Self {
            base: base::rules(var).into(),
            shared: Arc::new(Shared {
                dial,
                upstream: Upstream::from_env(var),
            }),
            dir: private_dir()?,
            connections: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            running: Mutex::default(),
            secrets,
            roots: OnceLock::new(),
        })
    }

    /// What the session's proxy needs to add `secrets`, and the CA bundle
    /// it writes into `folder`; `None` if no secret is left to add.
    /// Secrets for `claude`'s own hosts (the base list) are never added.
    async fn bind_secrets(
        &self,
        project: ProjectId,
        secrets: Vec<SecretInfo>,
        folder: &Path,
    ) -> Result<Option<(Bound, PathBuf)>> {
        let secrets: Vec<SecretInfo> = secrets
            .into_iter()
            .filter(|s| {
                !self
                    .base
                    .iter()
                    .any(|r| r.host == inject::binding(&s.host).0)
            })
            .collect();
        if secrets.is_empty() {
            return Ok(None);
        }
        let mut hosts: Vec<String> = secrets.iter().map(|s| inject::binding(&s.host).0).collect();
        hosts.sort();
        hosts.dedup();
        let roots = match self.roots.get() {
            Some(roots) => roots.clone(),
            None => {
                let read = tokio::task::spawn_blocking(|| {
                    Roots::from_env(&|name: &str| std::env::var(name).ok())
                })
                .await
                .map_err(|e| internal(format!("reading the system's roots: {e}")))?;
                self.roots.get_or_init(|| read).clone()
            }
        };
        let ca = SessionCa::new(&hosts).map_err(|e| internal(format!("the session CA: {e}")))?;
        let path = folder.join("ca.pem");
        write_private(&path, roots.bundle(ca.pem()).as_bytes())
            .map_err(|e| internal(format!("writing {}: {e}", path.display())))?;
        let client = roots.client().map_err(internal)?;
        let bound = Bound {
            ca,
            secrets,
            project,
            store: self.secrets.clone(),
            client,
        };
        Ok(Some((bound, path)))
    }
}

/// A new file only this user can read.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// A new folder in `dir` only this user can open.
fn new_folder(dir: &Path) -> Result<PathBuf> {
    let folder = dir.join(random_name());
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(&folder)
        .map_err(|e| internal(format!("creating {}: {e}", folder.display())))?;
    Ok(folder)
}

/// A new folder only this user can open, with a short path: Unix socket
/// paths are limited to about 100 bytes.
fn private_dir() -> Result<PathBuf> {
    let tmp = std::env::temp_dir();
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    for _ in 0..100u32 {
        let dir = tmp.join(format!("ovra-net-{}", random_name()));
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(internal(format!("creating {}: {e}", dir.display()))),
        }
    }
    Err(internal("no free folder for the netproxy's sockets".into()))
}

/// 64 random bits as hex: a folder name nobody can guess or take first.
fn random_name() -> String {
    format!("{:016x}", rand::random::<u64>())
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
        secrets: Vec<SecretInfo>,
    ) -> BoxFuture<'_, Result<NetStarted>> {
        Box::pin(async move {
            if lock(&self.running).contains_key(&session) {
                return Err(ProtocolError::new(
                    ErrorCode::Conflict,
                    format!("session {} already has a netproxy", session.0),
                ));
            }
            let (listener, address, mut socket) = bind(&self.dir).await?;
            let mut ca_bundle = None;
            let mut bound = None;
            if !secrets.is_empty() {
                let folder = match &socket {
                    Some(folder) => folder.clone(),
                    None => new_folder(&self.dir)?,
                };
                socket = Some(folder.clone());
                let made = self
                    .bind_secrets(refusals.project().clone(), secrets, &folder)
                    .await;
                match made {
                    Ok(Some((made, path))) => {
                        bound = Some(made);
                        ca_bundle = Some(path);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = std::fs::remove_dir_all(folder);
                        return Err(e);
                    }
                }
            }
            let net = Arc::new(SessionNet {
                rules: RwLock::new(Rules {
                    base: self.base.clone(),
                    project: allow.hosts,
                }),
                reporter: refusals,
                bound,
            });
            let task = tokio::spawn(accept(
                listener,
                self.shared.clone(),
                net.clone(),
                self.connections.clone(),
            ));
            let running = Running { net, task, socket };
            let mut all = lock(&self.running);
            if all.contains_key(&session) {
                drop(all);
                running.end();
                return Err(ProtocolError::new(
                    ErrorCode::Conflict,
                    format!("session {} already has a netproxy", session.0),
                ));
            }
            all.insert(session, running);
            Ok(NetStarted { address, ca_bundle })
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

/// A Unix socket in its own new folder in `dir`, with a name nobody can
/// guess, so that a sandbox can open exactly one session's socket: the
/// address is its path.
#[cfg(unix)]
async fn bind(dir: &std::path::Path) -> Result<(Listener, String, Option<PathBuf>)> {
    let folder = dir.join(random_name());
    let mut builder = std::fs::DirBuilder::new();
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(&folder)
        .map_err(|e| internal(format!("creating {}: {e}", folder.display())))?;
    let path = folder.join("p.sock");
    let listener = Listener::bind(&path)
        .map_err(|e| internal(format!("netproxy socket {}: {e}", path.display())))?;
    Ok((listener, path.to_string_lossy().into_owned(), Some(folder)))
}

/// A 127.0.0.1 port: the address is `127.0.0.1:<port>`.
#[cfg(not(unix))]
async fn bind(_dir: &std::path::Path) -> Result<(Listener, String, Option<PathBuf>)> {
    let listener = Listener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| internal(format!("netproxy port: {e}")))?;
    let address = listener
        .local_addr()
        .map_err(|e| internal(format!("netproxy port: {e}")))?
        .to_string();
    Ok((listener, address, None))
}

async fn accept(
    listener: Listener,
    shared: Arc<Shared>,
    net: Arc<SessionNet>,
    all: Arc<Semaphore>,
) {
    let limit = Arc::new(Semaphore::new(MAX_SESSION_CONNECTIONS));
    let mut connections = JoinSet::new();
    loop {
        let (Ok(permit), Ok(global)) = (
            limit.clone().acquire_owned().await,
            all.clone().acquire_owned().await,
        ) else {
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
            drop((permit, global));
        });
    }
}

#[cfg(test)]
mod tests;
