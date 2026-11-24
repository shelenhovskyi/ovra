//! Moves a terminal's output from the pseudo-terminal to the core's events
//! without losing any.
//!
//! All front ends share one event channel, and a subscriber that falls more
//! than its capacity behind misses events. So the output side holds back
//! while the channel's backlog is high and merges what piles up meanwhile
//! into larger events. When its own buffer is full, the reader stops
//! reading, the pseudo-terminal fills up and the shell waits, as it would in
//! any slow terminal.

use std::io::Read;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ovra_core::protocol::terminals::{TerminalId, TerminalsEvent};
use ovra_core::services::EventSink;

use crate::lock;
use crate::scrollback::Scrollback;
use crate::utf8::Utf8Stream;

/// Output waits while more events than this are queued for some subscriber;
/// the channel holds 1024, and the rest is room for other services.
const MAX_BACKLOG: usize = 256;
/// Largest output event built from merged reads.
const MAX_EVENT_BYTES: usize = 64 * 1024;
const READ_BYTES: usize = 16 * 1024;
/// Reads buffered between the reader and the emitter (up to 1 MiB).
const BUFFERED_READS: usize = 64;
/// How long `terminals.exited` waits for more output after the shell ended.
/// Output only keeps coming when a background job still holds the terminal.
const DRAIN_GRACE: Duration = Duration::from_millis(500);
/// How often a held-back emitter looks at the backlog again.
const BACKLOG_POLL: Duration = Duration::from_millis(1);

#[derive(Debug)]
pub(crate) enum Chunk {
    Data(Vec<u8>),
    /// The shell ended, with this exit code.
    Exited(Option<i32>),
}

/// Starts reading `reader` and emitting its output. Send [`Chunk::Exited`]
/// through the returned sender when the shell ends; `terminals.exited`
/// follows the last output.
pub(crate) fn serve(
    mut reader: Box<dyn Read + Send>,
    id: TerminalId,
    sink: EventSink,
    output: Arc<Mutex<Scrollback>>,
) -> SyncSender<Chunk> {
    let (tx, rx) = sync_channel(BUFFERED_READS);
    let reads = tx.clone();
    thread::spawn(move || {
        let mut buf = vec![0u8; READ_BYTES];
        loop {
            match reader.read(&mut buf) {
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                // Unix reports EIO once the shell and its children are gone.
                Ok(0) | Err(_) => break,
                // Blocks while the emitter is behind; an error means it is done.
                Ok(n) => {
                    if reads.send(Chunk::Data(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });
    thread::spawn(move || emit_all(&rx, &id, &sink, &output));
    tx
}

fn emit_all(rx: &Receiver<Chunk>, id: &TerminalId, sink: &EventSink, output: &Mutex<Scrollback>) {
    let mut stream = Utf8Stream::default();
    let mut exit: Option<Option<i32>> = None;
    loop {
        let next = if exit.is_some() {
            rx.recv_timeout(DRAIN_GRACE)
                .map_err(|e| e == RecvTimeoutError::Timeout)
        } else {
            rx.recv().map_err(|_| false)
        };
        let mut data = match next {
            Ok(Chunk::Data(data)) => data,
            Ok(Chunk::Exited(code)) => {
                exit = Some(code);
                continue;
            }
            // Every sender is gone, or the grace after the exit is over.
            Err(_) => break,
        };
        while sink.backlog() >= MAX_BACKLOG {
            #[cfg(test)]
            {
                lock(output).held_back = true;
            }
            thread::sleep(BACKLOG_POLL);
        }
        while data.len() < MAX_EVENT_BYTES {
            match rx.try_recv() {
                Ok(Chunk::Data(more)) => data.extend_from_slice(&more),
                Ok(Chunk::Exited(code)) => exit = Some(code),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        lock(output).emit(sink, id, stream.push(&data));
    }
    lock(output).emit(sink, id, stream.finish());
    sink.emit(TerminalsEvent::Exited {
        id: id.clone(),
        code: exit.flatten(),
    });
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use ovra_core::Core;
    use ovra_core::protocol::Event;

    use super::*;

    /// Hands out `left` reads of `line` as fast as they are asked for.
    struct Flood {
        line: &'static [u8],
        left: usize,
        done: Arc<AtomicBool>,
    }

    impl Read for Flood {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.left == 0 {
                self.done.store(true, Ordering::SeqCst);
                return Ok(0);
            }
            self.left -= 1;
            buf[..self.line.len()].copy_from_slice(self.line);
            Ok(self.line.len())
        }
    }

    #[tokio::test]
    async fn a_reader_faster_than_the_subscriber_loses_nothing() {
        // Enough small reads to overflow the core's 1024 events.
        const READS: usize = 200_000;
        let core = Core::new("test");
        let mut rx = core.subscribe();
        let id = TerminalId("t1".into());
        let done = Arc::new(AtomicBool::new(false));
        let flood = Flood {
            line: "line é\r\n".as_bytes(),
            left: READS,
            done: Arc::clone(&done),
        };
        let output = Arc::new(Mutex::new(Scrollback::default()));
        // The sender is dropped right away, as the waiter thread does.
        serve(Box::new(flood), id, core.sink(), Arc::clone(&output))
            .send(Chunk::Exited(Some(0)))
            .expect("send");
        // A slow subscriber: read nothing until the output is held back
        // for it (or, without backpressure, until the flood is over).
        while !(done.load(Ordering::SeqCst) || lock(&output).held_back) {
            tokio::task::yield_now().await;
        }

        let (mut text, mut events) = (String::new(), 0u64);
        loop {
            // A lagging subscriber gets an error here instead of the event.
            match rx.recv().await.expect("no events lost") {
                Event::Terminals(TerminalsEvent::Output { data, seq, .. }) => {
                    events += 1;
                    assert_eq!(seq, events);
                    text += &data;
                }
                Event::Terminals(TerminalsEvent::Exited { code, .. }) => {
                    assert_eq!(code, Some(0));
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(text.matches("line é\r\n").count(), READS);
        assert_eq!(text.len(), READS * "line é\r\n".len());
        assert_eq!(lock(&output).tail().1, events);
    }
}
