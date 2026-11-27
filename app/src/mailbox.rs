//! The only way the app tells a window anything: a `tauri::ipc::Channel`
//! per window, handed over by the window itself (`ovra_subscribe`). Tauri
//! events reach every window that listens for them, whatever window they
//! are sent to, so none are used: a remote machine's window must never
//! hear the laptop's frames, drops or menu, nor the laptop's window the
//! machine's.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use serde_json::Value;
use tauri::ipc::Channel;

#[derive(Default)]
pub(crate) struct Mailboxes(Mutex<HashMap<String, Channel<Value>>>);

impl Mailboxes {
    /// `label`'s channel from now on, replacing one it had (a reload).
    pub(crate) fn set(&self, label: &str, channel: Channel<Value>) {
        self.lock().insert(label.to_owned(), channel);
    }

    pub(crate) fn forget(&self, label: &str) {
        self.lock().remove(label);
    }

    /// Hands `message` to the window `label` only.
    pub(crate) fn send(&self, label: &str, message: Value) {
        let channel = self.lock().get(label).cloned();
        if let Some(channel) = channel {
            let _ = channel.send(message);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Channel<Value>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
