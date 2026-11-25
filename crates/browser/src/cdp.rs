//! A Chrome DevTools Protocol client over `--remote-debugging-pipe`: JSON
//! messages, each ended by a NUL byte, Chromium reading fd 3 and writing
//! fd 4. Any byte stream pair works, so tests talk to a scripted peer.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;
use serde_json::{Value, json};
use tokio::io::{
    AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _,
};
use tokio::sync::{mpsc, oneshot};

/// How long a call waits for its answer.
pub(crate) const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// The largest message read: a full-page screenshot in base64 fits.
const MESSAGE_LIMIT: u64 = 256 << 20;

/// A message Chromium sent on its own, for the page of `session` or (when
/// `None`) for the browser.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Event {
    pub(crate) session: Option<String>,
    pub(crate) method: String,
    pub(crate) params: Value,
}

type Pending = Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>;

/// One connection to a browser. Cheap to clone.
#[derive(Debug, Clone)]
pub(crate) struct Cdp {
    out: mpsc::UnboundedSender<Vec<u8>>,
    pending: Arc<Pending>,
    next: Arc<AtomicU64>,
}

pub(crate) fn gone() -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, "the browser has closed")
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Cdp {
    /// Starts reading `reader` and writing `writer`. The events end when
    /// the browser closes the stream; every call still waiting fails then.
    pub(crate) fn start<R, W>(reader: R, writer: W) -> (Self, mpsc::UnboundedReceiver<Event>)
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (out, mut outgoing) = mpsc::unbounded_channel::<Vec<u8>>();
        let (events, received) = mpsc::unbounded_channel();
        let pending: Arc<Pending> = Arc::default();
        tokio::spawn(async move {
            let mut writer = writer;
            while let Some(mut message) = outgoing.recv().await {
                message.push(0);
                if writer.write_all(&message).await.is_err() || writer.flush().await.is_err() {
                    break;
                }
            }
        });
        let answers = pending.clone();
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(reader);
            let mut message = Vec::new();
            loop {
                message.clear();
                let read = (&mut reader)
                    .take(MESSAGE_LIMIT)
                    .read_until(0, &mut message)
                    .await;
                if !matches!(read, Ok(n) if n > 0) || message.pop() != Some(0) {
                    break;
                }
                let Ok(value) = serde_json::from_slice::<Value>(&message) else {
                    continue;
                };
                if let Some(id) = value.get("id").and_then(Value::as_u64) {
                    if let Some(answer) = lock(&answers).remove(&id) {
                        let _ = answer.send(outcome(value));
                    }
                } else if let Some(method) = value.get("method").and_then(Value::as_str) {
                    let event = Event {
                        session: value
                            .get("sessionId")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        method: method.to_owned(),
                        params: value.get("params").cloned().unwrap_or(Value::Null),
                    };
                    if events.send(event).is_err() {
                        break;
                    }
                }
            }
            // Dropping the senders fails every call still waiting.
            lock(&answers).clear();
        });
        let cdp = Self {
            out,
            pending,
            next: Arc::new(AtomicU64::new(1)),
        };
        (cdp, received)
    }

    /// Writes a command and returns its id.
    fn write(&self, session: Option<&str>, method: &str, params: Value) -> Option<u64> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut message = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        self.out.send(message.to_string().into_bytes()).ok()?;
        Some(id)
    }

    /// Writes a command and waits for its answer. The lock is held while
    /// it is written, so the answer cannot come first.
    fn register(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Option<(u64, oneshot::Receiver<Result<Value>>)> {
        let (answer, answered) = oneshot::channel();
        let mut pending = lock(&self.pending);
        let id = self.write(session, method, params)?;
        pending.insert(id, answer);
        Some((id, answered))
    }

    /// Sends a command without waiting for its answer.
    pub(crate) fn send(&self, session: Option<&str>, method: &str, params: Value) {
        self.write(session, method, params);
    }

    pub(crate) async fn call(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        self.call_for(session, method, params, CALL_TIMEOUT).await
    }

    /// As [`Cdp::call`], waiting up to `limit`.
    pub(crate) async fn call_for(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        limit: Duration,
    ) -> Result<Value> {
        let (id, answered) = self.register(session, method, params).ok_or_else(gone)?;
        match tokio::time::timeout(limit, answered).await {
            Ok(Ok(outcome)) => {
                outcome.map_err(|e| ProtocolError::new(e.code, format!("{method}: {}", e.message)))
            }
            Ok(Err(_)) => Err(gone()),
            Err(_) => {
                lock(&self.pending).remove(&id);
                Err(ProtocolError::new(
                    ErrorCode::Internal,
                    format!("the browser did not answer {method}"),
                ))
            }
        }
    }
}

fn outcome(mut value: Value) -> Result<Value> {
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("failed");
        return Err(ProtocolError::new(ErrorCode::Internal, message));
    }
    Ok(value
        .get_mut("result")
        .map(Value::take)
        .unwrap_or(Value::Null))
}

#[cfg(test)]
pub(crate) mod peer {
    //! A scripted peer in place of Chromium: it reads what the client
    //! writes and answers what the test tells it to.

    use super::*;
    use tokio::io::{DuplexStream, ReadHalf, WriteHalf};

    /// A command the client sent.
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) struct Sent {
        pub(crate) id: u64,
        pub(crate) session: Option<String>,
        pub(crate) method: String,
        pub(crate) params: Value,
    }

    #[derive(Debug)]
    pub(crate) struct Peer {
        reader: tokio::io::BufReader<ReadHalf<DuplexStream>>,
        writer: WriteHalf<DuplexStream>,
    }

    /// The client's two ends and the peer.
    pub(crate) fn pair() -> (ReadHalf<DuplexStream>, WriteHalf<DuplexStream>, Peer) {
        let (ours, theirs) = tokio::io::duplex(1 << 20);
        let (read, write) = tokio::io::split(ours);
        let (peer_read, peer_write) = tokio::io::split(theirs);
        let peer = Peer {
            reader: tokio::io::BufReader::new(peer_read),
            writer: peer_write,
        };
        (read, write, peer)
    }

    impl Peer {
        /// The next command, or `None` once the client stopped writing.
        pub(crate) async fn next(&mut self) -> Option<Sent> {
            let mut message = Vec::new();
            self.reader.read_until(0, &mut message).await.ok()?;
            message.pop()?;
            let value: Value = serde_json::from_slice(&message).ok()?;
            Some(Sent {
                id: value["id"].as_u64()?,
                session: value["sessionId"].as_str().map(str::to_owned),
                method: value["method"].as_str()?.to_owned(),
                params: value["params"].clone(),
            })
        }

        pub(crate) async fn write(&mut self, value: Value) {
            let mut bytes = value.to_string().into_bytes();
            bytes.push(0);
            let _ = self.writer.write_all(&bytes).await;
        }

        pub(crate) async fn answer(&mut self, id: u64, result: Value) {
            self.write(json!({ "id": id, "result": result })).await;
        }

        pub(crate) async fn event(&mut self, session: Option<&str>, method: &str, params: Value) {
            let mut value = json!({ "method": method, "params": params });
            if let Some(session) = session {
                value["sessionId"] = json!(session);
            }
            self.write(value).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::peer::pair;
    use super::*;

    #[tokio::test]
    async fn answers_find_their_calls_and_events_carry_their_session() {
        let (read, write, mut peer) = pair();
        let (cdp, mut events) = Cdp::start(read, write);
        let first = tokio::spawn({
            let cdp = cdp.clone();
            async move { cdp.call(None, "Target.getTargets", json!({})).await }
        });
        let one = peer.next().await.expect("first");
        let second = tokio::spawn({
            let cdp = cdp.clone();
            async move {
                cdp.call(Some("S1"), "Page.navigate", json!({"url": "x"}))
                    .await
            }
        });
        let two = peer.next().await.expect("second");
        assert_eq!(
            (two.session.as_deref(), two.method.as_str()),
            (Some("S1"), "Page.navigate")
        );
        peer.event(Some("S1"), "Page.loadEventFired", json!({"timestamp": 1}))
            .await;
        peer.write(json!({"id": two.id, "error": {"code": -32000, "message": "no frame"}}))
            .await;
        peer.answer(one.id, json!({"targetInfos": []})).await;
        assert_eq!(
            first.await.ok().and_then(Result::ok),
            Some(json!({"targetInfos": []}))
        );
        let failed = second.await.ok().and_then(Result::err).map(|e| e.message);
        assert_eq!(failed.as_deref(), Some("Page.navigate: no frame"));
        assert_eq!(
            events.recv().await,
            Some(Event {
                session: Some("S1".into()),
                method: "Page.loadEventFired".into(),
                params: json!({"timestamp": 1}),
            })
        );
    }

    #[tokio::test]
    async fn a_closed_pipe_fails_waiting_calls_and_ends_the_events() {
        let (read, write, mut peer) = pair();
        let (cdp, mut events) = Cdp::start(read, write);
        let call = tokio::spawn({
            let cdp = cdp.clone();
            async move { cdp.call(None, "Browser.getVersion", json!({})).await }
        });
        assert!(peer.next().await.is_some());
        drop(peer);
        let code = call.await.ok().and_then(Result::err).map(|e| e.code);
        assert_eq!(code, Some(ErrorCode::Unavailable));
        assert_eq!(events.recv().await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_call_without_an_answer_times_out() {
        let (read, write, _peer) = pair();
        let (cdp, _events) = Cdp::start(read, write);
        let error = cdp
            .call_for(None, "Page.reload", json!({}), Duration::from_secs(1))
            .await
            .err()
            .map(|e| e.message);
        assert_eq!(
            error.as_deref(),
            Some("the browser did not answer Page.reload")
        );
    }
}
