//! One client over a byte stream: `ovra-daemon --stdio` on a remote
//! machine, reached through the person's own `ssh` (ADR 0016,
//! `docs/specs/ssh-remotes.md`). Frames are JSON lines. The first line out
//! is [`Hello`]; a line over [`LINE_MAX`] or one that is not UTF-8 ends the
//! connection, and so do [`IDLE`] without any line from the client.

use std::time::Duration;

use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use ovra_core::Core;
use ovra_core::protocol::info::InfoEvent;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::Instant;

use crate::{dispatch, event_frame};

/// The longest line either side sends.
pub const LINE_MAX: usize = 16 * 1024 * 1024;
/// How long the core waits for any line before it gives up on the client.
pub const IDLE: Duration = Duration::from_secs(60);
/// The line a client sends to say it is still there; it gets no answer.
pub const PING: &str = r#"{"ping":{}}"#;
/// The only line a core writes, instead of [`Hello`], when another
/// `--stdio` core still uses its data directory.
pub const BUSY: &str = r#"{"busy":{}}"#;
/// The line a client sends to end a `--core` (Stop Ovra on <name>); it gets
/// no answer. Only the exact line counts.
pub const QUIT: &str = r#"{"quit":{}}"#;
/// The last line a `--core` writes to a client another one replaced.
pub const REPLACED: &str = r#"{"replaced":{}}"#;
/// The most requests that run at once; more wait unread.
const IN_FLIGHT_MAX: usize = 64;
/// Raised when the frames change in a way an older side cannot read.
pub const PROTOCOL: u32 = 1;

/// The first line the remote core writes; `ovra-ssh` reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// The crate version; the laptop refuses any other than its own.
    pub version: String,
    pub protocol: u32,
    pub os: String,
    /// Why there is no sandbox on this machine; `None` when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_sandbox: Option<String>,
    /// When a `--core` started, in milliseconds since the Unix epoch; the
    /// laptop tells from it that a core it left was ended meanwhile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_ms: Option<u64>,
}

#[derive(Serialize)]
struct HelloFrame<'a> {
    hello: &'a Hello,
}

/// The hello line as the laptop reads it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelloLine {
    pub hello: Hello,
}

/// Why [`serve`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// The client closed its side.
    Closed,
    /// No line arrived for [`IDLE`].
    Idle,
    /// A line was too long or not UTF-8.
    BadLine,
    /// Writing failed: the client is gone.
    Gone,
    /// The client sent [`QUIT`].
    Quit,
    /// Another client took over; this one was told [`REPLACED`].
    Replaced,
}

/// What a `--core` adds to serving one client ([`serve_client`]).
#[derive(Debug, Default)]
pub struct Attach {
    /// Told once, when the client sends its first line: only then may it
    /// replace the current client.
    pub first_line: Option<tokio::sync::oneshot::Sender<()>>,
    /// Ends the client with [`REPLACED`] when it fires or is dropped.
    pub replaced: Option<tokio::sync::oneshot::Receiver<()>>,
    /// Sends an `info.resync` right after the hello: events were lost
    /// while no client was attached.
    pub resync: bool,
}

/// Serves one client of `core` on `input` and `output` until it ends.
pub async fn serve<R, W>(core: Core, input: R, output: W, hello: &Hello) -> Ended
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    serve_client(core, input, output, hello, Attach::default()).await
}

/// [`serve`] for one client of a `--core`, with what [`Attach`] adds.
pub async fn serve_client<R, W>(
    core: Core,
    input: R,
    mut output: W,
    hello: &Hello,
    attach: Attach,
) -> Ended
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let Attach {
        mut first_line,
        replaced,
        resync,
    } = attach;
    let replaced = async move {
        match replaced {
            Some(replaced) => {
                let _ = replaced.await;
            }
            None => std::future::pending().await,
        }
    };
    tokio::pin!(replaced);
    let mut events = core.subscribe();
    let client = core.connect();
    let hello = serde_json::to_string(&HelloFrame { hello }).unwrap_or_default();
    if write_line(&mut output, &hello).await.is_err()
        || write_line(&mut output, &event_frame(InfoEvent::Ready.into()))
            .await
            .is_err()
    {
        return Ended::Gone;
    }
    if resync
        && write_line(
            &mut output,
            &event_frame(InfoEvent::Resync { skipped: 0 }.into()),
        )
        .await
        .is_err()
    {
        return Ended::Gone;
    }
    let client = &client;
    let mut input = Lines::new(input);
    let mut running = FuturesUnordered::new();
    let mut deadline = Instant::now() + IDLE;
    loop {
        // Lines first, the deadline last: after a slow write, pings that
        // arrived meanwhile count before the deadline does.
        tokio::select! {
            biased;
            line = input.next(), if running.len() < IN_FLIGHT_MAX => match line {
                Line::End => return Ended::Closed,
                Line::Bad => return Ended::BadLine,
                Line::Text(text) => {
                    deadline = Instant::now() + IDLE;
                    if let Some(first) = first_line.take() {
                        let _ = first.send(());
                    }
                    match text.trim() {
                        PING => {}
                        QUIT => return Ended::Quit,
                        _ => running.push(async move { dispatch(client, &text).await }),
                    }
                }
            },
            () = &mut replaced => {
                let _ = write_line(&mut output, REPLACED).await;
                return Ended::Replaced;
            },
            Some(reply) = running.next(), if !running.is_empty() => {
                if write_line(&mut output, &reply).await.is_err() {
                    return Ended::Gone;
                }
            },
            event = Core::next_event(&mut events) => match event {
                Some(event) => {
                    if write_line(&mut output, &event_frame(event)).await.is_err() {
                        return Ended::Gone;
                    }
                }
                None => return Ended::Closed,
            },
            // Not while requests fill every slot: pings then wait unread.
            () = tokio::time::sleep_until(deadline), if running.len() < IN_FLIGHT_MAX => {
                return Ended::Idle;
            }
        }
    }
}

/// Fails when the client takes longer than [`IDLE`] to take the line: a
/// laptop that went to sleep leaves the connection open but reads nothing.
async fn write_line<W: AsyncWrite + Unpin>(output: &mut W, line: &str) -> std::io::Result<()> {
    let write = async {
        output.write_all(line.as_bytes()).await?;
        output.write_all(b"\n").await?;
        output.flush().await
    };
    tokio::time::timeout(IDLE, write)
        .await
        .map_err(|_| std::io::ErrorKind::TimedOut)?
}

/// One read of [`Lines`].
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    Text(String),
    /// The other side closed, or reading failed.
    End,
    /// Longer than the limit, or not UTF-8.
    Bad,
}

/// Reads lines without ever holding more than its limit
/// ([`LINE_MAX`] for frames).
#[derive(Debug)]
pub struct Lines<R> {
    input: R,
    buf: Vec<u8>,
    max: usize,
}

impl<R: AsyncBufRead + Unpin> Lines<R> {
    pub fn new(input: R) -> Self {
        Self::with_limit(input, LINE_MAX)
    }

    /// Lines of at most `max` bytes.
    pub fn with_limit(input: R, max: usize) -> Self {
        Self {
            input,
            buf: Vec::new(),
            max,
        }
    }

    /// Cancel-safe: a read cut short keeps what it had in `buf`.
    pub async fn next(&mut self) -> Line {
        loop {
            let chunk = match self.input.fill_buf().await {
                Ok(chunk) => chunk,
                Err(_) => return Line::End,
            };
            if chunk.is_empty() {
                return Line::End;
            }
            let (take, done) = match chunk.iter().position(|&b| b == b'\n') {
                Some(at) => (at + 1, true),
                None => (chunk.len(), false),
            };
            if self.buf.len() + take > self.max + 1 {
                return Line::Bad;
            }
            self.buf.extend_from_slice(&chunk[..take]);
            self.input.consume(take);
            if done {
                let mut line = std::mem::take(&mut self.buf);
                line.pop();
                return match String::from_utf8(line) {
                    Ok(text) => Line::Text(text),
                    Err(_) => Line::Bad,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests;
