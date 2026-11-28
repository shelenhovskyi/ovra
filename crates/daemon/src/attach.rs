//! `--core` and `--attach` (`docs/specs/ssh-remotes.md`, section 6): a
//! remote machine's core that outlives the SSH connection. `--core` serves
//! one client at a time on a Unix socket in the private data folder;
//! `--attach`, which `ssh` runs, starts it when none runs and relays bytes.

use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use ovra_core::Core;
use ovra_transport::stdio::{self, Attach, Ended, Hello};
use rustix::fs::{FileType, Mode, OFlags};
use rustix::io::Errno;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};

/// How long a core stays with no client while nothing runs.
pub(crate) const IDLE_EXIT: Duration = Duration::from_secs(15 * 60);
/// How often a core without a client looks whether it may end.
const IDLE_CHECK: Duration = Duration::from_secs(30);
/// How long `--attach` waits for a core to answer.
const START_WAIT: Duration = Duration::from_secs(10);
/// How long `--attach` still tries after its core found the data folder taken.
const BUSY_GRACE: Duration = Duration::from_secs(1);
/// How long `--attach --restart` waits for the old core to let go.
const RESTART_WAIT: Duration = Duration::from_secs(30);
/// The exit code of a `--core` that found the data folder in use.
pub(crate) const EXIT_BUSY: i32 = 3;
/// `core.log` is cut back to this much...
const LOG_KEEP: u64 = 1024 * 1024;
/// ...when it grows past this.
const LOG_MAX: u64 = 2 * 1024 * 1024;
const SOCKET: &str = "core.sock";
/// Variables of the SSH session the detached core must not keep.
const DROPPED_ENV: &[&str] = &["DISPLAY", "XAUTHORITY", "OVRA_UNUSED"];

/// A folder of this user alone, open: made 0700 on the descriptor.
#[derive(Debug)]
pub(crate) struct Private(OwnedFd);

impl Private {
    /// Opens `path` (an absolute folder) without following a symlink at
    /// its last part, and checks it.
    pub(crate) fn open(path: &Path) -> anyhow::Result<Self> {
        Self::at(rustix::fs::CWD, path, false)
    }

    /// `name` in this folder, made when missing.
    fn child(&self, name: &str) -> anyhow::Result<Self> {
        Self::at(&self.0, Path::new(name), true)
    }

    /// `run/`, where the core's socket is.
    pub(crate) fn child_run(&self) -> anyhow::Result<Self> {
        self.child("run")
    }

    fn at(dir: impl AsFd, name: &Path, create: bool) -> anyhow::Result<Self> {
        if create {
            match rustix::fs::mkdirat(&dir, name, Mode::RWXU) {
                Ok(()) | Err(Errno::EXIST) => {}
                Err(e) => return Err(e).with_context(|| format!("making {}", name.display())),
            }
        }
        let fd = rustix::fs::openat(
            &dir,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .with_context(|| format!("{} is not a folder Ovra may use", name.display()))?;
        let stat = rustix::fs::fstat(&fd)?;
        anyhow::ensure!(
            stat.st_uid == rustix::process::geteuid().as_raw(),
            "{} belongs to another user",
            name.display()
        );
        rustix::fs::fchmod(&fd, Mode::RWXU).context("making the folder private")?;
        Ok(Self(fd))
    }

    /// A path to `name` in this folder through its descriptor: no race on
    /// the folders above it, and no limit on how long they are.
    fn path(&self, name: &str) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}/{name}", self.0.as_raw_fd()))
    }
}

/// Opens `core.log` for the core's stderr: no symlink, this user only,
/// cut to its last [`LOG_KEEP`] bytes.
pub(crate) fn open_log(data: &Private) -> anyhow::Result<std::fs::File> {
    let fd = rustix::fs::openat(
        &data.0,
        "core.log",
        OFlags::RDWR | OFlags::CREATE | OFlags::APPEND | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .context("opening core.log")?;
    let stat = rustix::fs::fstat(&fd)?;
    anyhow::ensure!(
        FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile
            && stat.st_uid == rustix::process::geteuid().as_raw(),
        "core.log is not a file of this user"
    );
    rustix::fs::fchmod(&fd, Mode::RUSR | Mode::WUSR)?;
    let file = std::fs::File::from(fd);
    cut_log(&file)?;
    Ok(file)
}

/// Keeps only the last [`LOG_KEEP`] bytes of `file`.
fn cut_log(file: &std::fs::File) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() <= LOG_KEEP {
        return Ok(());
    }
    let mut file = file;
    file.seek(SeekFrom::Start(meta.len() - LOG_KEEP))?;
    let mut tail = Vec::new();
    file.take(LOG_KEEP).read_to_end(&mut tail)?;
    file.set_len(0)?;
    // Ignored with O_APPEND, which writes at the end anyway.
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&tail)
}

/// Cuts the core's own stderr when it is `core.log` and too long.
fn cut_own_log() {
    let stderr = std::io::stderr();
    let Ok(fd) = stderr.as_fd().try_clone_to_owned() else {
        return;
    };
    let file = std::fs::File::from(fd);
    if file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() > LOG_MAX)
    {
        let _ = cut_log(&file);
    }
}

/// The core's socket, bound while the data folder's lock is held: a
/// leftover one is removed, anything else in its place is refused.
pub(crate) fn bind(run: &Private) -> anyhow::Result<UnixListener> {
    match rustix::fs::statat(&run.0, SOCKET, rustix::fs::AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => {
            anyhow::ensure!(
                FileType::from_raw_mode(stat.st_mode) == FileType::Socket,
                "run/{SOCKET} is not a socket"
            );
            rustix::fs::unlinkat(&run.0, SOCKET, rustix::fs::AtFlags::empty())?;
        }
        Err(Errno::NOENT) => {}
        Err(e) => return Err(e).context("checking the socket"),
    }
    let old = rustix::process::umask(Mode::RWXG | Mode::RWXO);
    let bound = std::os::unix::net::UnixListener::bind(run.path(SOCKET));
    rustix::process::umask(old);
    let listener = bound.context("binding the core's socket")?;
    rustix::fs::chmodat(
        &run.0,
        SOCKET,
        Mode::RUSR | Mode::WUSR,
        rustix::fs::AtFlags::empty(),
    )?;
    listener.set_nonblocking(true)?;
    Ok(UnixListener::from_std(listener)?)
}

/// Whether `stream` comes from a process of this user.
fn same_user(stream: &UnixStream) -> bool {
    stream
        .peer_cred()
        .is_ok_and(|cred| same_uid(cred.uid(), rustix::process::geteuid().as_raw()))
}

/// The core's check on a peer: its uid is the core's own, root's included
/// only for root.
pub(crate) fn same_uid(peer: u32, me: u32) -> bool {
    peer == me
}

/// What one client's task tells the core.
#[derive(Debug)]
enum Said {
    /// Client `n` sent its first line: it takes over.
    First(u64),
    Ended(u64, Ended),
}

/// Whether a core with no client since `since` may end at `now`.
pub(crate) fn may_end(
    since: Option<tokio::time::Instant>,
    now: tokio::time::Instant,
    runs: bool,
) -> bool {
    since.is_some_and(|since| !runs && now.duration_since(since) >= IDLE_EXIT)
}

/// Serves clients of `core` on `listener` until a client sends
/// `{"quit":{}}` or the core has been alone with nothing running for
/// [`IDLE_EXIT`].
pub(crate) async fn serve_core(core: Core, listener: UnixListener, hello: Hello) {
    let (said_tx, mut said) = mpsc::channel::<Said>(16);
    let mut made = 0u64;
    // The client that took over, and what tells it it was replaced.
    let mut current: Option<(u64, oneshot::Sender<()>)> = None;
    // Clients that have not sent a line yet, with their replace senders.
    let mut waiting: Vec<(u64, oneshot::Sender<()>)> = Vec::new();
    let mut alone_since = Some(tokio::time::Instant::now());
    core.set_attended(false);
    let mut check = tokio::time::interval(IDLE_CHECK);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else {
                    // Out of descriptors, say: let some close.
                    tracing::warn!("could not accept a client");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                };
                if !same_user(&stream) {
                    tracing::warn!("closed a connection from another user");
                    continue;
                }
                made += 1;
                let n = made;
                let (replace_tx, replace_rx) = oneshot::channel();
                let (first_tx, first_rx) = oneshot::channel();
                waiting.push((n, replace_tx));
                let attach = Attach { first_line: Some(first_tx), replaced: Some(replace_rx), resync: n > 1 };
                let (core, hello, said) = (core.clone(), hello.clone(), said_tx.clone());
                tokio::spawn(async move {
                    let (read, write) = stream.into_split();
                    let told = said.clone();
                    tokio::spawn(async move {
                        if first_rx.await.is_ok() {
                            let _ = told.send(Said::First(n)).await;
                        }
                    });
                    let ended = stdio::serve_client(core, tokio::io::BufReader::new(read), write, &hello, attach).await;
                    let _ = said.send(Said::Ended(n, ended)).await;
                });
            }
            Some(said) = said.recv() => match said {
                Said::First(n) => {
                    if let Some(at) = waiting.iter().position(|(w, _)| *w == n) {
                        // Dropping the old sender ends the old client with `replaced`.
                        current = Some(waiting.swap_remove(at));
                        alone_since = None;
                        core.set_attended(true);
                    }
                }
                Said::Ended(n, ended) => {
                    waiting.retain(|(w, _)| *w != n);
                    if ended == Ended::Quit {
                        tracing::info!("asked to quit");
                        return;
                    }
                    if current.as_ref().is_some_and(|(c, _)| *c == n) {
                        tracing::info!(?ended, "the client left; the core stays");
                        current = None;
                        alone_since = Some(tokio::time::Instant::now());
                        core.set_attended(false);
                    }
                }
            },
            _ = check.tick() => {
                cut_own_log();
                if may_end(alone_since, tokio::time::Instant::now(), core.runs_anything()) {
                    tracing::info!("alone with nothing running; stopping");
                    return;
                }
            }
        }
    }
}

/// Relays this process's stdin and stdout to the core of `data`, starting
/// it when none answers. With `restart`, the running core is asked to quit
/// first.
pub(crate) async fn attach(data_dir: &Path, restart: bool) -> anyhow::Result<()> {
    let data = Private::open(data_dir)?;
    let run = data.child_run()?;
    let socket = run.path(SOCKET);
    if restart {
        if let Ok(old) = UnixStream::connect(&socket).await {
            // Its lock below says whether it ended; this only asks.
            let _ = tokio::time::timeout(RESTART_WAIT, ask_to_quit(old)).await;
        }
        let freed = crate::lock_data_dir(data_dir, (RESTART_WAIT.as_millis() / 250) as u32).await?;
        anyhow::ensure!(freed.is_some(), "the running Ovra did not stop within 30 s");
    }
    let mut stream = match UnixStream::connect(&socket).await {
        Ok(stream) => stream,
        Err(e) if no_core(&e) => match start(data_dir, &data, &socket).await? {
            Some(stream) => stream,
            None => {
                // A `--stdio` holds the data folder.
                println!("{}", stdio::BUSY);
                return Ok(());
            }
        },
        Err(e) => return Err(e).context("connecting to the core"),
    };
    let mut local = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
    let _ = tokio::io::copy_bidirectional(&mut local, &mut stream).await;
    Ok(())
}

/// Asks the core on `stream` to quit, as a client: after its hello (which
/// it writes first, so a client that closed already never gets read), and
/// then until it closes.
async fn ask_to_quit(stream: UnixStream) -> std::io::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};
    let (read, mut write) = stream.into_split();
    let mut read = tokio::io::BufReader::new(read.take(stdio::LINE_MAX as u64 + 1));
    let mut hello = Vec::new();
    read.read_until(b'\n', &mut hello).await?;
    write
        .write_all(format!("{}\n", stdio::QUIT).as_bytes())
        .await?;
    // What it still says goes nowhere; its end is what counts.
    tokio::io::copy(&mut read.into_inner().into_inner(), &mut tokio::io::sink()).await?;
    Ok(())
}

fn no_core(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
    )
}

/// Starts a `--core` on `data_dir`, detached from this SSH session, and
/// connects once it answers; `None` when another process holds the data
/// folder and no core answers.
async fn start(
    data_dir: &Path,
    data: &Private,
    socket: &Path,
) -> anyhow::Result<Option<UnixStream>> {
    let exe = std::env::current_exe().context("finding ovra-daemon")?;
    let log = open_log(data)?;
    let mut command = tokio::process::Command::new(exe);
    command
        .arg("--core")
        .arg("--data-dir")
        .arg(data_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(log)
        // Its own process group: without a terminal nothing hangs it up
        // when the SSH session ends.
        .process_group(0);
    if let Some(home) = std::env::var_os("HOME") {
        command.current_dir(home);
    }
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy().into_owned();
        if name.starts_with("SSH_") || DROPPED_ENV.contains(&name.as_str()) {
            command.env_remove(name);
        }
    }
    let mut child = command.spawn().context("starting the core")?;
    let mut until = tokio::time::Instant::now() + START_WAIT;
    let mut busy = false;
    while tokio::time::Instant::now() < until {
        match UnixStream::connect(socket).await {
            Ok(stream) => return Ok(Some(stream)),
            Err(e) if no_core(&e) => {}
            Err(e) => return Err(e).context("connecting to the core"),
        }
        if let Ok(Some(status)) = child.try_wait() {
            // Another core may still be starting: keep trying until the wait ends.
            if status.code() != Some(EXIT_BUSY) {
                anyhow::bail!(
                    "the core stopped: {status}; see {}",
                    data_dir.join("core.log").display()
                );
            }
            if !busy {
                // A core that holds the lock binds its socket right after
                // taking it; give it a moment, then the holder is a `--stdio`.
                busy = true;
                until = until.min(tokio::time::Instant::now() + BUSY_GRACE);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    anyhow::ensure!(busy, "the core did not answer within 10 s");
    Ok(None)
}

#[cfg(test)]
mod tests;
