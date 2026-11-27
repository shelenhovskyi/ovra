//! The link to one remote machine: `ssh` running `ovra-daemon --stdio`
//! there, its hello checked, and every frame it sends checked before it
//! goes on (`docs/specs/ssh-remotes.md`, "Errors and noise" and "Bridge").

use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ovra_transport::stdio::{BUSY, Hello, HelloLine, LINE_MAX, Line, Lines, PING, PROTOCOL};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::tools::Group;

/// How long `ssh` may take to bring the hello: the remote core waits up to
/// 10 s for an older one to let go.
const HELLO_WAIT: Duration = Duration::from_secs(60);
/// Lines before the hello that are not frames (a remote `.bashrc`).
const NOISE_MAX: usize = 64 * 1024;
/// What is kept of `ssh`'s stderr: the sshd banner and the remote daemon's
/// logs, both remote-controlled.
const STDERR_MAX: usize = 64 * 1024;
/// The remote core gives up after 60 s without a line.
const PING_EVERY: Duration = Duration::from_secs(15);
/// How long a line to the remote core may take to write.
const WRITE_WAIT: Duration = Duration::from_secs(60);
/// Frames waiting for the window; past this, events are dropped and
/// `info.resync` says how many.
const QUEUE: usize = 256;

/// Why a link did not open or ended, as `ssh.status` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    HostKeyChanged,
    NoPin,
    AuthFailed,
    NoDaemon,
    VersionMismatch,
    Busy,
    NoSsh,
    UnsafeConfig,
    Exited,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HostKeyChanged => "host_key_changed",
            Self::NoPin => "no_pin",
            Self::AuthFailed => "auth_failed",
            Self::NoDaemon => "no_daemon",
            Self::VersionMismatch => "version_mismatch",
            Self::Busy => "busy",
            Self::NoSsh => "no_ssh",
            Self::UnsafeConfig => "unsafe_config",
            Self::Exited => "exited",
        }
    }
}

/// A reason and what Ovra or `ssh` said about it. `details` may hold the
/// remote machine's own text: show it only as plain text, folded away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub reason: Reason,
    pub details: String,
}

impl Failure {
    pub(crate) fn new(reason: Reason, details: impl Into<String>) -> Self {
        Self {
            reason,
            details: details.into(),
        }
    }
}

/// An open link. Dropping it ends `ssh` and its process group.
///
/// For whoever bridges it to a window:
/// - Drain [`Link::recv`] in a task of its own, apart from
///   [`Link::send`]: a remote machine can fill the queue with replies,
///   and one task that does both in turn would wait on itself.
/// - Replies wait for room in the queue, and no ping goes out meanwhile:
///   a window that takes nothing for 60 s loses the link.
/// - A reply's `id` is the remote machine's word: ignore ids the window
///   never sent.
#[derive(Debug)]
pub struct Link {
    hello: Hello,
    to: mpsc::Sender<String>,
    from: mpsc::Receiver<String>,
    end: watch::Receiver<Option<Failure>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Link {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Link {
    /// What the remote core said it is, `no_sandbox` included.
    pub fn hello(&self) -> &Hello {
        &self.hello
    }

    /// Sends one client frame; see [`Sender::send`].
    pub async fn send(&self, frame: &str) -> Result<(), Failure> {
        self.sender().send(frame).await
    }

    /// A handle that sends while another task waits in [`Link::recv`].
    pub fn sender(&self) -> Sender {
        Sender {
            to: self.to.clone(),
            end: self.end.clone(),
        }
    }

    /// The next checked frame from the remote core, or why the link ended.
    pub async fn recv(&mut self) -> Result<String, Failure> {
        match self.from.recv().await {
            Some(frame) => Ok(frame),
            None => Err(self.ended()),
        }
    }

    fn ended(&self) -> Failure {
        ended(&self.end)
    }
}

/// Sends frames on a [`Link`]; it stops working when the link ends.
#[derive(Debug, Clone)]
pub struct Sender {
    to: mpsc::Sender<String>,
    end: watch::Receiver<Option<Failure>>,
}

impl Sender {
    /// Sends one client frame. A frame that would not be one line is
    /// refused rather than split.
    pub async fn send(&self, frame: &str) -> Result<(), Failure> {
        if frame.len() > LINE_MAX || frame.contains(['\n', '\r']) {
            return Err(Failure::new(
                Reason::Exited,
                "a frame for the remote machine was not one line",
            ));
        }
        match self.to.send(frame.to_owned()).await {
            Ok(()) => Ok(()),
            Err(_) => Err(ended(&self.end)),
        }
    }
}

fn ended(end: &watch::Receiver<Option<Failure>>) -> Failure {
    end.borrow()
        .clone()
        .unwrap_or_else(|| Failure::new(Reason::Exited, "the link was closed"))
}

/// A link to whatever `command` runs, with the hello and frame checks but
/// none of the SSH ones: e2e's `--fake-ssh` runs a local
/// `ovra-daemon --stdio` through it.
#[cfg(feature = "fake")]
pub async fn fake(mut command: tokio::process::Command) -> Result<Link, Failure> {
    crate::tools::own_group(&mut command);
    pipes(&mut command);
    let program = command
        .as_std()
        .get_program()
        .to_string_lossy()
        .into_owned();
    let child = command
        .spawn()
        .map_err(|e| Failure::new(Reason::NoSsh, format!("could not run {program}: {e}")))?;
    open(Group(child)).await
}

/// Waits for the hello of a started `ssh`, then hands its pipes to the
/// pump.
pub(crate) async fn open(mut child: Group) -> Result<Link, Failure> {
    let missing = || Failure::new(Reason::NoSsh, "could not talk to ssh");
    let stdin = child.0.stdin.take().ok_or_else(missing)?;
    let stdout = child.0.stdout.take().ok_or_else(missing)?;
    let mut stderr = Stderr::start(child.0.stderr.take());
    let mut lines = Lines::new(BufReader::new(stdout));
    let hello = match tokio::time::timeout(HELLO_WAIT, read_hello(&mut lines)).await {
        Err(_) => {
            return Err(Failure::new(
                Reason::Exited,
                "the remote machine did not answer within a minute",
            ));
        }
        Ok(Ok(hello)) => hello,
        Ok(Err(Some(failure))) => return Err(failure),
        Ok(Err(None)) => return Err(exited_early(&mut child, &mut stderr).await),
    };
    let (to, to_rx) = mpsc::channel(16);
    let (from_tx, from) = mpsc::channel(QUEUE);
    let (end_tx, end) = watch::channel(None);
    let task = tokio::spawn(async move {
        let failure = pump(&mut child, stdin, lines, to_rx, from_tx, &mut stderr).await;
        let _ = end_tx.send(Some(failure));
    });
    Ok(Link {
        hello,
        to,
        from,
        end,
        task,
    })
}

/// The hello, skipping what is not a frame; `Err(None)` when `ssh` closed
/// its output first.
async fn read_hello<R: tokio::io::AsyncBufRead + Unpin>(
    lines: &mut Lines<R>,
) -> Result<Hello, Option<Failure>> {
    let mut noise = 0;
    loop {
        let text = match lines.next().await {
            Line::Text(text) => text,
            Line::End => return Err(None),
            Line::Bad => return Err(Some(no_daemon())),
        };
        if text.trim() == BUSY {
            return Err(Some(Failure::new(
                Reason::Busy,
                "the last connection is still closing; try again in a minute",
            )));
        }
        if let Ok(line) = serde_json::from_str::<HelloLine>(&text) {
            let hello = line.hello;
            let ours = env!("CARGO_PKG_VERSION");
            if hello.version != ours || hello.protocol != PROTOCOL {
                return Err(Some(Failure::new(
                    Reason::VersionMismatch,
                    format!(
                        "this computer runs Ovra {ours}; the remote machine runs {}",
                        plain(&hello.version, 40)
                    ),
                )));
            }
            return Ok(Hello {
                os: plain(&hello.os, 40),
                no_sandbox: hello.no_sandbox.map(|reason| plain(&reason, 200)),
                ..hello
            });
        }
        noise += text.len() + 1;
        if noise > NOISE_MAX {
            return Err(Some(no_daemon()));
        }
    }
}

fn no_daemon() -> Failure {
    Failure::new(
        Reason::NoDaemon,
        "Ovra did not answer on the remote machine; check the path of Ovra there",
    )
}

/// Why `ssh` ended before the hello: its exit status, and for 255 (its
/// own failures) what it said.
async fn exited_early(child: &mut Group, stderr: &mut Stderr) -> Failure {
    let status = tokio::time::timeout(Duration::from_secs(5), child.0.wait()).await;
    let code = status.ok().and_then(Result::ok).and_then(|s| s.code());
    let said = stderr.said().await;
    let reason = match code {
        Some(255)
            if said.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
                || said.contains("Host key verification failed") =>
        {
            Reason::HostKeyChanged
        }
        Some(255) if said.contains("Permission denied") => Reason::AuthFailed,
        Some(127) => Reason::NoDaemon,
        _ => Reason::Exited,
    };
    Failure::new(reason, said)
}

/// Moves frames both ways until something ends the link, and says what.
async fn pump(
    child: &mut Group,
    mut stdin: ChildStdin,
    mut lines: Lines<BufReader<ChildStdout>>,
    mut to: mpsc::Receiver<String>,
    from: mpsc::Sender<String>,
    stderr: &mut Stderr,
) -> Failure {
    let mut ping = tokio::time::interval_at(Instant::now() + PING_EVERY, PING_EVERY);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut skipped: u64 = 0;
    loop {
        let out = tokio::select! {
            biased;
            frame = to.recv() => match frame {
                Some(frame) => frame,
                None => return Failure::new(Reason::Exited, "the link was closed"),
            },
            _ = ping.tick() => PING.to_owned(),
            line = lines.next() => {
                let text = match line {
                    Line::Text(text) => text,
                    Line::Bad => return bad_frame(),
                    Line::End => {
                        let _ = tokio::time::timeout(Duration::from_secs(5), child.0.wait()).await;
                        return Failure::new(Reason::Exited, stderr.said().await);
                    }
                };
                match kind(&text) {
                    None => return bad_frame(),
                    Some(Kind::Dropped) => {}
                    Some(Kind::Reply) => {
                        if from.send(text).await.is_err() {
                            return Failure::new(Reason::Exited, "the link was closed");
                        }
                    }
                    Some(Kind::Event) => match from.try_send(text) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => skipped += 1,
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            return Failure::new(Reason::Exited, "the link was closed");
                        }
                    },
                }
                if skipped > 0 {
                    let resync = format!(r#"{{"event":{{"type":"info.resync","skipped":{skipped}}}}}"#);
                    if from.try_send(resync).is_ok() {
                        skipped = 0;
                    }
                }
                continue;
            }
        };
        let write = async {
            stdin.write_all(out.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await
        };
        if !matches!(tokio::time::timeout(WRITE_WAIT, write).await, Ok(Ok(()))) {
            return Failure::new(Reason::Exited, stderr.said().await);
        }
    }
}

fn bad_frame() -> Failure {
    Failure::new(
        Reason::Exited,
        "the remote machine sent a frame Ovra does not accept",
    )
}

enum Kind {
    Reply,
    Event,
    /// An `ssh.*` event: a remote machine never speaks for the laptop's
    /// own list of machines.
    Dropped,
}

/// `{"id", "result"}`, `{"id", "error"}` or `{"event": {"type", ...}}`;
/// anything else is no frame.
fn kind(text: &str) -> Option<Kind> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let object = value.as_object()?;
    if let Some(event) = object.get("event") {
        let name = event.get("type")?.as_str()?;
        if object.len() != 1 {
            return None;
        }
        return Some(if name.starts_with("ssh.") {
            Kind::Dropped
        } else {
            Kind::Event
        });
    }
    let id = object.get("id")?;
    let answer = object.contains_key("result") != object.contains_key("error");
    let id_ok = id.is_u64() || (id.is_null() && object.contains_key("error"));
    (object.len() == 2 && answer && id_ok).then_some(Kind::Reply)
}

/// `ssh`'s stderr, read to its end in the background, keeping the first
/// [`STDERR_MAX`] bytes.
struct Stderr {
    kept: Arc<Mutex<Vec<u8>>>,
    done: watch::Receiver<bool>,
}

impl Stderr {
    fn start(pipe: Option<impl AsyncRead + Unpin + Send + 'static>) -> Self {
        let kept = Arc::new(Mutex::new(Vec::new()));
        let (done_tx, done) = watch::channel(false);
        let into = kept.clone();
        tokio::spawn(async move {
            if let Some(mut pipe) = pipe {
                let mut chunk = [0; 4096];
                while let Ok(n @ 1..) = pipe.read(&mut chunk).await {
                    let mut kept = into
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let room = STDERR_MAX.saturating_sub(kept.len());
                    kept.extend_from_slice(&chunk[..n.min(room)]);
                }
            }
            let _ = done_tx.send(true);
        });
        Self { kept, done }
    }

    /// What `ssh` said, once it closed stderr (at most a second after it
    /// is asked: a process it left behind may hold the pipe).
    async fn said(&mut self) -> String {
        let _ = tokio::time::timeout(Duration::from_secs(1), self.done.wait_for(|d| *d)).await;
        let kept = self
            .kept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        plain(&String::from_utf8_lossy(&kept), STDERR_MAX)
    }
}

/// `text` without control characters other than line breaks and tabs, so
/// a remote machine cannot draw with escape sequences, cut to `max` bytes.
fn plain(text: &str, max: usize) -> String {
    let mut out = String::new();
    for c in text.trim().chars() {
        if out.len() + c.len_utf8() > max {
            break;
        }
        if c == '\n' || c == '\t' || !ovra_core::ssh::hidden(c) {
            out.push(c);
        }
    }
    out
}

/// Pipes for a link: all three, none inherited.
pub(crate) fn pipes(command: &mut tokio::process::Command) {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
}

#[cfg(all(test, unix))]
mod tests;
