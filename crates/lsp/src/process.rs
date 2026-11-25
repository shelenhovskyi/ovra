//! One server process: a writer task for its stdin, a reader for its stdout,
//! its stderr drained, and a supervisor that reports its end once.

use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{CommandLine, Result, RunningServer, ServerHost};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStderr, ChildStdin, Command};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::frame::{Decoder, encode};

/// How long a stopped server may take to exit before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(2);
/// How long output still pending when the process ends is read.
const DRAIN_GRACE: Duration = Duration::from_millis(500);
/// How often a held-back reader looks whether the clients caught up.
const BUSY_POLL: Duration = Duration::from_millis(10);
/// How much of stderr is kept for a failure reason.
const STDERR_TAIL: usize = 4096;
/// Messages waiting for a server that does not read its input.
const QUEUE: usize = 256;

/// The handle the core keeps. Dropping it stops the server.
#[derive(Debug)]
pub(crate) struct Process {
    queue: mpsc::Sender<Vec<u8>>,
    stop: Arc<watch::Sender<bool>>,
}

impl RunningServer for Process {
    fn send(&self, message: Value) -> Result<()> {
        self.queue.try_send(encode(&message)).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => ProtocolError::new(
                ErrorCode::Unavailable,
                "the language server is not reading its input",
            ),
            mpsc::error::TrySendError::Closed(_) => {
                ProtocolError::new(ErrorCode::NotFound, "the language server is gone")
            }
        })
    }

    fn stop(&self) {
        self.stop.send_replace(true);
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts `line`. Must run inside a tokio runtime.
pub(crate) fn spawn(line: &CommandLine, name: &str, host: ServerHost) -> Result<Process> {
    let mut command = Command::new(&line.program);
    command.args(&line.args).current_dir(&line.cwd);
    for (key, value) in &line.env {
        match value {
            Some(value) => command.env(key, value),
            None => command.env_remove(key),
        };
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            ProtocolError::new(
                ErrorCode::Unavailable,
                format!("could not start {name}: {e}"),
            )
        })?;
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return Err(ProtocolError::new(
            ErrorCode::Internal,
            format!("{name} started without pipes"),
        ));
    };
    let (queue, queued) = mpsc::channel(QUEUE);
    let stop = Arc::new(watch::channel(false).0);
    tokio::spawn(write(stdin, queued, stop.subscribe()));
    let reader = tokio::spawn(read(stdout, host.clone()));
    let ends = Ends {
        child,
        reader,
        stderr,
        host,
        name: name.to_owned(),
        stop: stop.clone(),
    };
    tokio::spawn(ends.supervise());
    Ok(Process { queue, stop })
}

/// Writes queued messages until the server is stopped, then what is still
/// queued (the core's polite `shutdown` and `exit`), then closes stdin.
async fn write(
    mut stdin: ChildStdin,
    mut queued: mpsc::Receiver<Vec<u8>>,
    mut stopped: watch::Receiver<bool>,
) {
    loop {
        let bytes = tokio::select! {
            biased;
            _ = stopped.wait_for(|s| *s) => break,
            bytes = queued.recv() => bytes,
        };
        let Some(bytes) = bytes else { return };
        if stdin.write_all(&bytes).await.is_err() || stdin.flush().await.is_err() {
            return;
        }
    }
    while let Ok(bytes) = queued.try_recv() {
        if stdin.write_all(&bytes).await.is_err() {
            return;
        }
    }
    let _ = stdin.flush().await;
}

/// Hands each message to the host in order, held back while it is busy.
/// `Ok` at the end of the output; an error ends the server.
pub(crate) async fn read(
    mut output: impl AsyncRead + Unpin,
    host: ServerHost,
) -> std::result::Result<(), String> {
    let mut decoder = Decoder::default();
    let mut buf = vec![0; 64 << 10];
    loop {
        let n = output
            .read(&mut buf)
            .await
            .map_err(|e| format!("could not read the server's output: {e}"))?;
        if n == 0 {
            return Ok(());
        }
        decoder.push(&buf[..n]);
        while let Some(message) = decoder.next()? {
            while host.busy() {
                tokio::time::sleep(BUSY_POLL).await;
            }
            host.message(message);
        }
    }
}

/// What the supervisor waits on first.
enum First {
    Exited(std::io::Result<ExitStatus>),
    Stopped,
    Read(std::result::Result<(), String>),
}

struct Ends {
    child: Child,
    reader: JoinHandle<std::result::Result<(), String>>,
    stderr: ChildStderr,
    host: ServerHost,
    name: String,
    stop: Arc<watch::Sender<bool>>,
}

impl Ends {
    async fn supervise(mut self) {
        let tail = Arc::new(Mutex::new(Vec::new()));
        let mut draining = tokio::spawn(drain(self.stderr, tail.clone()));
        let mut stopped = self.stop.subscribe();
        let first = tokio::select! {
            status = self.child.wait() => First::Exited(status),
            _ = stopped.wait_for(|s| *s) => First::Stopped,
            read = &mut self.reader => First::Read(joined(read)),
        };
        let mut read = None;
        let mut by_us = false;
        let status = match first {
            First::Exited(status) => status,
            First::Stopped => {
                by_us = true;
                end(&mut self.child).await
            }
            First::Read(Err(reason)) => {
                read = Some(Err(reason));
                let _ = self.child.start_kill();
                self.child.wait().await
            }
            First::Read(Ok(())) => {
                read = Some(Ok(()));
                let status = tokio::select! {
                    status = self.child.wait() => Some(status),
                    _ = stopped.wait_for(|s| *s) => None,
                };
                match status {
                    Some(status) => status,
                    None => {
                        by_us = true;
                        end(&mut self.child).await
                    }
                }
            }
        };
        let read = match read {
            Some(read) => read,
            // A child the server left behind may hold the pipe open.
            None => match timeout(DRAIN_GRACE, &mut self.reader).await {
                Ok(read) => joined(read),
                Err(_) => {
                    self.reader.abort();
                    let _ = (&mut self.reader).await;
                    Ok(())
                }
            },
        };
        if timeout(DRAIN_GRACE, &mut draining).await.is_err() {
            draining.abort();
        }
        let code = status.as_ref().ok().and_then(ExitStatus::code);
        let reason = match (read, status) {
            (Err(reason), _) => Some(reason),
            (Ok(()), _) if by_us => None,
            (Ok(()), Ok(status)) if status.success() => None,
            (Ok(()), Ok(status)) => {
                let message = format!("{} exited with {status}", self.name);
                Some(
                    match last_line(&tail.lock().unwrap_or_else(PoisonError::into_inner)) {
                        Some(line) => format!("{message}: {line}"),
                        None => message,
                    },
                )
            }
            (Ok(()), Err(e)) => Some(format!("could not wait for {}: {e}", self.name)),
        };
        // Ends the writer if the server exited by itself.
        self.stop.send_replace(true);
        self.host.exited(code, reason);
    }
}

fn joined(
    read: std::result::Result<std::result::Result<(), String>, tokio::task::JoinError>,
) -> std::result::Result<(), String> {
    read.unwrap_or_else(|e| Err(format!("the server's reader failed: {e}")))
}

/// Closing stdin (the writer does) ends most servers; the rest are killed.
async fn end(child: &mut Child) -> std::io::Result<ExitStatus> {
    match timeout(STOP_GRACE, child.wait()).await {
        Ok(status) => status,
        Err(_) => {
            let _ = child.start_kill();
            child.wait().await
        }
    }
}

/// Reads stderr so the server never blocks on it, keeping the end.
async fn drain(mut stderr: ChildStderr, tail: Arc<Mutex<Vec<u8>>>) {
    let mut buf = [0; 4096];
    while let Ok(n) = stderr.read(&mut buf).await {
        if n == 0 {
            return;
        }
        let mut tail = tail.lock().unwrap_or_else(PoisonError::into_inner);
        tail.extend_from_slice(&buf[..n]);
        let extra = tail.len().saturating_sub(STDERR_TAIL);
        tail.drain(..extra);
    }
}

fn last_line(tail: &[u8]) -> Option<String> {
    String::from_utf8_lossy(tail)
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use serde_json::json;
    use tokio::io::AsyncWriteExt;

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn holds_messages_back_while_the_host_is_busy() {
        let (host, record) = ServerHost::recording();
        record.busy.store(true, Ordering::SeqCst);
        let (mut server, output) = tokio::io::duplex(1024);
        let reading = tokio::spawn(read(output, host));
        let mut bytes = encode(&json!({"id": 1}));
        bytes.extend(encode(&json!({"id": 2})));
        server.write_all(&bytes).await.expect("write");
        tokio::time::sleep(BUSY_POLL * 10).await;
        assert!(record.messages().is_empty());
        record.busy.store(false, Ordering::SeqCst);
        record.wait(|r| r.messages().len() == 2).await;
        assert_eq!(record.messages(), [json!({"id": 1}), json!({"id": 2})]);
        drop(server);
        assert_eq!(reading.await.expect("join"), Ok(()));
    }

    #[tokio::test]
    async fn a_bad_frame_ends_reading_with_a_reason() {
        let (host, record) = ServerHost::recording();
        let (mut server, output) = tokio::io::duplex(1024);
        let mut bytes = encode(&json!({"id": 1}));
        bytes.extend(b"Content-Length: x\r\n\r\n");
        server.write_all(&bytes).await.expect("write");
        let read = read(output, host).await;
        assert_eq!(read, Err("the server wrote a bad header".into()));
        assert_eq!(record.messages(), [json!({"id": 1})]);
    }

    #[test]
    fn the_reason_uses_the_last_line_of_stderr() {
        assert_eq!(
            last_line(b"warn\nerror: no cargo\n\n"),
            Some("error: no cargo".into())
        );
        assert_eq!(last_line(b" \n"), None);
    }
}
