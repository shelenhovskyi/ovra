//! A language-server service whose servers are driven by the test.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::{lock, not_found};
use crate::protocol::lsp::KnownServer;
use crate::services::{
    BoxFuture, LanguageServers, Result, RunningServer, ServerHost, ServerSpec, StartedServer,
};

/// Starts a [`FakeServer`] for every language in `known` that has a path,
/// and `not_found` for any other. The test reads what the core sent each
/// server and makes it say things.
#[derive(Debug)]
pub struct FakeLanguageServers {
    pub known: Vec<KnownServer>,
    pub sandbox: Option<String>,
    started: Mutex<Vec<FakeServer>>,
}

impl Default for FakeLanguageServers {
    /// `fake-ls` for `rust`, installed, in the sandbox `fake`.
    fn default() -> Self {
        Self::new(vec![KnownServer {
            language: "rust".into(),
            name: "fake-ls".into(),
            path: Some(crate::test_path!("/bin/fake-ls").into()),
            install: "install fake-ls".into(),
        }])
    }
}

impl FakeLanguageServers {
    pub fn new(known: Vec<KnownServer>) -> Self {
        Self {
            known,
            sandbox: Some("fake".into()),
            started: Mutex::default(),
        }
    }

    /// Every server started so far, first first.
    pub fn started(&self) -> Vec<FakeServer> {
        lock(&self.started).clone()
    }
}

impl LanguageServers for FakeLanguageServers {
    fn known(&self) -> BoxFuture<'_, Vec<KnownServer>> {
        Box::pin(async move { self.known.clone() })
    }

    fn sandbox(&self) -> Option<String> {
        self.sandbox.clone()
    }

    fn start(&self, spec: ServerSpec, host: ServerHost) -> BoxFuture<'_, Result<StartedServer>> {
        Box::pin(async move {
            let known = self
                .known
                .iter()
                .find(|k| k.language == spec.language && k.path.is_some())
                .ok_or_else(|| not_found(format!("a language server for {}", spec.language)))?;
            let server = FakeServer {
                spec,
                host,
                sent: Arc::default(),
                stopped: Arc::default(),
            };
            lock(&self.started).push(server.clone());
            Ok(StartedServer {
                process: Box::new(server),
                name: known.name.clone(),
                sandbox: self.sandbox.clone(),
            })
        })
    }
}

/// One fake server: records what it was sent, says what the test tells it.
#[derive(Debug, Clone)]
pub struct FakeServer {
    pub spec: ServerSpec,
    host: ServerHost,
    sent: Arc<Mutex<Vec<serde_json::Value>>>,
    stopped: Arc<AtomicBool>,
}

impl FakeServer {
    /// Every message the core sent, in order.
    pub fn sent(&self) -> Vec<serde_json::Value> {
        lock(&self.sent).clone()
    }

    /// Hands `message` to the core as if the server wrote it.
    pub fn say(&self, message: serde_json::Value) {
        self.host.message(message);
    }

    /// Ends the server as if it exited by itself.
    pub fn exit(&self, code: Option<i32>, reason: Option<String>) {
        self.host.exited(code, reason);
    }

    /// Whether the core stopped it.
    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
}

impl RunningServer for FakeServer {
    fn send(&self, message: serde_json::Value) -> Result<()> {
        lock(&self.sent).push(message);
        Ok(())
    }

    fn stop(&self) {
        if !self.stopped.swap(true, Ordering::SeqCst) {
            self.host.exited(None, None);
        }
    }
}

/// What a [`ServerHost::recording`] host was handed.
#[derive(Debug, Default)]
pub struct HostRecord {
    messages: Mutex<Vec<serde_json::Value>>,
    exited: Mutex<Option<(Option<i32>, Option<String>)>>,
    changed: tokio::sync::Notify,
    /// What [`ServerHost::busy`] answers.
    pub busy: AtomicBool,
}

impl HostRecord {
    pub(crate) fn message(&self, message: serde_json::Value) {
        lock(&self.messages).push(message);
        self.changed.notify_waiters();
    }

    pub(crate) fn exited(&self, code: Option<i32>, reason: Option<String>) {
        *lock(&self.exited) = Some((code, reason));
        self.changed.notify_waiters();
    }

    pub(crate) fn busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    /// The messages so far.
    pub fn messages(&self) -> Vec<serde_json::Value> {
        lock(&self.messages).clone()
    }

    /// How the server ended, once it has.
    pub fn exit(&self) -> Option<(Option<i32>, Option<String>)> {
        lock(&self.exited).clone()
    }

    /// Waits until `done` holds for this record. Tests end on the test
    /// runner's time limit if it never does.
    pub async fn wait(&self, done: impl Fn(&Self) -> bool) {
        loop {
            // Created before the check, so a change in between still wakes it.
            let changed = self.changed.notified();
            if done(self) {
                return;
            }
            changed.await;
        }
    }
}
