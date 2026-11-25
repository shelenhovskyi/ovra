//! Ovra's way into a pod's sockets. The pod's folder is writable inside
//! the pod, so a name in it may be a link (to the host's Docker socket,
//! say) or a socket some server made. Each socket is opened without
//! following links, must be a socket, and must be pod-init's: the peer's
//! pid is the one learned from `control.sock`, which pod-init made before
//! anything else ran in the pod.

use std::io;
use std::os::fd::OwnedFd;
use std::path::Path;
#[cfg(not(target_os = "linux"))]
use std::path::PathBuf;
use std::sync::OnceLock;

use rustix::fs::{FileType, Mode, OFlags};
use tokio::net::UnixStream;

use crate::control::{CONTROL, port_socket};

/// How far up from a peer its parents are looked at for the process Ovra
/// started: the sandbox puts its own processes in between.
const MAX_DEPTH: usize = 8;

/// One pod's folder, held open from its making on.
#[derive(Debug)]
pub(crate) struct Sockets {
    /// Where there is no `/proc/self/fd`, sockets are reached by name.
    #[cfg(not(target_os = "linux"))]
    path: PathBuf,
    folder: OwnedFd,
    /// Pod-init's pid, once its control socket answered.
    init: OnceLock<u32>,
}

impl Sockets {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let flags = OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::RDONLY;
        Ok(Self {
            #[cfg(not(target_os = "linux"))]
            path: path.to_owned(),
            folder: rustix::fs::open(path, flags, Mode::empty())?,
            init: OnceLock::new(),
        })
    }

    /// The control connection; its listener must be ours and, where the
    /// system shows it, `launched` or a process below it. That listener is
    /// pod-init from then on.
    pub(crate) async fn control(&self, launched: Option<u32>) -> io::Result<UnixStream> {
        let conn = self.connect(CONTROL).await?;
        let pid = peer(&conn)?;
        if let Some(launched) = launched
            && !below(pid, launched)
        {
            return Err(refused("its control socket is not the pod's own"));
        }
        let init = *self.init.get_or_init(|| pid);
        if init != pid {
            return Err(refused("its control socket changed hands"));
        }
        Ok(conn)
    }

    /// A connection to the relay of `port`, which pod-init listens on.
    pub(crate) async fn port(&self, port: u16) -> io::Result<UnixStream> {
        let Some(&init) = self.init.get() else {
            return Err(refused("the pod has not started yet"));
        };
        let conn = self.connect(&port_socket(port)).await?;
        if peer(&conn)? != init {
            return Err(refused("its socket is not pod-init's"));
        }
        Ok(conn)
    }

    /// Connects to the socket `name` in the folder, never through a link.
    async fn connect(&self, name: &str) -> io::Result<UnixStream> {
        #[cfg(target_os = "linux")]
        {
            // The socket itself, held while connecting: `/proc/self/fd/N`
            // leads to it whatever the name points to by then.
            let flags = OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC;
            let socket = rustix::fs::openat(&self.folder, name, flags, Mode::empty())?;
            is_socket(rustix::fs::fstat(&socket)?)?;
            let path = format!("/proc/self/fd/{}", std::os::fd::AsRawFd::as_raw_fd(&socket));
            UnixStream::connect(path).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            // The name may change after this check; the peer check holds.
            let at = rustix::fs::AtFlags::SYMLINK_NOFOLLOW;
            is_socket(rustix::fs::statat(&self.folder, name, at)?)?;
            UnixStream::connect(self.path.join(name)).await
        }
    }
}

fn is_socket(stat: rustix::fs::Stat) -> io::Result<()> {
    if FileType::from_raw_mode(stat.st_mode) == FileType::Socket {
        Ok(())
    } else {
        Err(refused("not a socket"))
    }
}

/// The pid of the process that listens on `conn`'s other end; it must run
/// as this user.
fn peer(conn: &UnixStream) -> io::Result<u32> {
    let cred = conn.peer_cred()?;
    if cred.uid() != rustix::process::geteuid().as_raw() {
        return Err(refused("its listener is another user's"));
    }
    cred.pid()
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid > 0)
        .ok_or_else(|| refused("its listener is unknown"))
}

/// Whether `pid` is `launched` or below it. Only Linux shows parents
/// (`/proc`); elsewhere the user check is all.
fn below(pid: u32, launched: u32) -> bool {
    if !cfg!(target_os = "linux") {
        return true;
    }
    let mut at = pid;
    for _ in 0..MAX_DEPTH {
        if at == launched {
            return true;
        }
        match parent(at) {
            Some(up) if up > 1 => at = up,
            _ => return false,
        }
    }
    false
}

/// The parent of `pid`: the field after the state in `/proc/<pid>/stat`,
/// whose name field may hold spaces and parentheses.
fn parent(pid: u32) -> Option<u32> {
    parent_in(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

fn parent_in(stat: &str) -> Option<u32> {
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_whitespace().nth(1)?.parse().ok()
}

fn refused(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, why.to_owned())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use tokio::net::UnixListener;

    fn pod() -> (tempfile::TempDir, Sockets) {
        let dir = tempfile::tempdir().unwrap();
        let sockets = Sockets::open(dir.path()).unwrap();
        (dir, sockets)
    }

    fn me() -> Option<u32> {
        Some(std::process::id())
    }

    #[tokio::test]
    async fn pod_init_s_own_sockets_connect() {
        let (dir, sockets) = pod();
        let _control = UnixListener::bind(dir.path().join(CONTROL)).unwrap();
        let _port = UnixListener::bind(dir.path().join("5173.sock")).unwrap();
        assert!(
            sockets.port(5173).await.is_err(),
            "before pod-init is known"
        );
        sockets.control(me()).await.unwrap();
        sockets.port(5173).await.unwrap();
    }

    #[tokio::test]
    async fn a_link_is_never_followed() {
        let (dir, sockets) = pod();
        let elsewhere = tempfile::tempdir().unwrap();
        let target = elsewhere.path().join("docker.sock");
        let _target = UnixListener::bind(&target).unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join(CONTROL)).unwrap();
        assert!(sockets.control(me()).await.is_err());
        std::fs::remove_file(dir.path().join(CONTROL)).unwrap();
        let _control = UnixListener::bind(dir.path().join(CONTROL)).unwrap();
        sockets.control(me()).await.unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("5173.sock")).unwrap();
        assert!(sockets.port(5173).await.is_err());
    }

    #[tokio::test]
    async fn a_file_that_is_not_a_socket_is_refused() {
        let (dir, sockets) = pod();
        std::fs::write(dir.path().join(CONTROL), "").unwrap();
        assert!(sockets.control(me()).await.is_err());
        std::fs::create_dir(dir.path().join("5173.sock")).unwrap();
        assert!(sockets.port(5173).await.is_err());
    }

    #[tokio::test]
    async fn a_socket_of_another_process_is_refused() {
        let (dir, sockets) = pod();
        let _control = UnixListener::bind(dir.path().join(CONTROL)).unwrap();
        // Not below the process that was started.
        #[cfg(target_os = "linux")]
        assert!(sockets.control(Some(1)).await.is_err());
        let _ = sockets.init.set(1);
        let _port = UnixListener::bind(dir.path().join("5173.sock")).unwrap();
        assert!(sockets.port(5173).await.is_err());
    }

    #[test]
    fn a_parent_is_read_past_a_name_with_parentheses() {
        assert_eq!(parent_in("41 (a) b (c)) S 7 41 41 0"), Some(7));
        assert_eq!(parent_in("41 (a"), None);
        #[cfg(target_os = "linux")]
        {
            let me = std::process::id();
            assert!(below(me, me));
            let up = rustix::process::Pid::as_raw(rustix::process::getppid());
            assert_eq!(parent(me), u32::try_from(up).ok());
        }
    }
}
