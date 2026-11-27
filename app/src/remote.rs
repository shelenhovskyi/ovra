//! Remote machines' windows (ADR 0016): one window per machine, labelled
//! `ssh-<id>`, bridged to that machine's core over `ovra_ssh::Link`.
//! Frames are routed by the window that sent them, never by their content.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use ovra_core::protocol::ssh::RemoteHost;
use ovra_core::services::{Opener, RemoteHosts};
use ovra_core::ssh::HostId;
use ovra_opener::SystemOpener;
use ovra_ssh::window::{self, Route};
use ovra_ssh::{Failure, OpenSsh};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::{oneshot, watch};

/// How long a call waits for the remote machine's answer.
const CALL_WAIT: std::time::Duration = std::time::Duration::from_secs(300);

/// The open links, by machine id.
pub(crate) struct Remotes {
    ssh: Arc<OpenSsh>,
    open: Mutex<HashMap<String, Arc<Open>>>,
}

/// One machine's link: its sender once connected, and the calls waiting
/// for a reply. Dropping it ends `ssh`.
struct Open {
    sender: watch::Receiver<Option<ovra_ssh::Sender>>,
    waiting: Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>>,
    task: tauri::async_runtime::JoinHandle<()>,
}

impl Drop for Open {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// What a remote window and the main window learn about a link, on an
/// app-only channel, so the remote machine cannot fake it.
#[derive(serde::Serialize)]
struct Status {
    id: String,
    state: &'static str,
    reason: Option<&'static str>,
    details: Option<String>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Remotes {
    pub(crate) fn new(ssh: Arc<OpenSsh>) -> Self {
        Self {
            ssh,
            open: Mutex::new(HashMap::new()),
        }
    }

    fn host(&self, id: &HostId) -> Option<RemoteHost> {
        self.ssh
            .list()
            .ok()?
            .into_iter()
            .find(|h| h.id == id.as_str())
    }

    /// Opens the machine's window, or focuses it, and connects unless a
    /// link is open or opening.
    pub(crate) fn show(&self, app: &AppHandle, id: &HostId) -> Result<(), String> {
        let host = self
            .host(id)
            .ok_or_else(|| "Ovra knows no such remote machine.".to_owned())?;
        let label = window::label(id);
        match app.get_webview_window(&label) {
            Some(existing) => {
                let _ = existing.set_focus();
            }
            None => build(app, id, &host)?,
        }
        self.connect(app, id);
        Ok(())
    }

    /// Starts a link for `id` unless one is open or opening.
    pub(crate) fn connect(&self, app: &AppHandle, id: &HostId) {
        let mut open = lock(&self.open);
        if open
            .get(id.as_str())
            .is_some_and(|o| !o.task.inner().is_finished())
        {
            return;
        }
        let (sender_tx, sender) = watch::channel(None);
        let waiting = Arc::new(Mutex::new(HashMap::new()));
        let task = tauri::async_runtime::spawn(run(
            app.clone(),
            self.ssh.clone(),
            id.clone(),
            sender_tx,
            waiting.clone(),
        ));
        open.insert(
            id.as_str().to_owned(),
            Arc::new(Open {
                sender,
                waiting,
                task,
            }),
        );
    }

    /// Ends the link of `id`, if there is one, even while a call still
    /// holds on to it.
    pub(crate) fn close(&self, id: &HostId) {
        if let Some(open) = lock(&self.open).remove(id.as_str()) {
            open.task.abort();
        }
    }

    /// Carries one frame from the window of `id` to its machine and returns
    /// the reply, or an `unavailable` error when there is no link.
    pub(crate) async fn call(&self, id: &HostId, frame: String) -> String {
        let Some(request) = window::request_id(&frame) else {
            return window::unavailable(0, "a frame without an id");
        };
        let open = lock(&self.open).get(id.as_str()).cloned();
        let Some(open) = open else {
            return window::unavailable(request, "not connected to this machine");
        };
        let mut sender = open.sender.clone();
        let Ok(sender) = sender.wait_for(Option::is_some).await.map(|s| s.clone()) else {
            return window::unavailable(request, "not connected to this machine");
        };
        let Some(sender) = sender else {
            return window::unavailable(request, "not connected to this machine");
        };
        let (reply_tx, reply) = oneshot::channel();
        lock(&open.waiting).insert(request, reply_tx);
        if let Err(failure) = sender.send(&frame).await {
            lock(&open.waiting).remove(&request);
            return window::unavailable(request, &failure.details);
        }
        drop(open);
        match tokio::time::timeout(CALL_WAIT, reply).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => window::unavailable(request, "the connection ended"),
            Err(_) => window::unavailable(request, "the remote machine did not answer"),
        }
    }
}

/// The machine's window: Ovra's own local UI, its own webview storage, no
/// file drops, and no navigation away from the UI.
fn build(app: &AppHandle, id: &HostId, host: &RemoteHost) -> Result<(), String> {
    let label = window::label(id);
    // Read by the UI as the machine it shows; from the host record, never
    // from the remote machine. Its name is checked to hold no line or
    // paragraph separators, so the JSON is a valid script.
    let identity = serde_json::json!({ "id": id.as_str(), "name": host.name });
    let script = format!("window.__OVRA_REMOTE__ = Object.freeze({identity});");
    let dev = app.config().build.dev_url.clone();
    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(format!("Ovra on {}", host.name))
        .inner_size(1200.0, 800.0)
        .min_inner_size(640.0, 400.0)
        .initialization_script(script)
        .disable_drag_drop_handler()
        .on_navigation(move |url| {
            let own = matches!(url.scheme(), "tauri") || url.host_str() == Some("tauri.localhost");
            let dev_server =
                cfg!(debug_assertions) && dev.as_ref().is_some_and(|d| d.origin() == url.origin());
            own || dev_server
        })
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny);
    #[cfg(not(target_os = "macos"))]
    let builder = {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("webview")
            .join(&label);
        builder.data_directory(dir)
    };
    #[cfg(target_os = "macos")]
    let builder = builder.data_store_identifier(window::data_store(id));
    builder.build().map(|_| ()).map_err(|e| e.to_string())
}

async fn run(
    app: AppHandle,
    ssh: Arc<OpenSsh>,
    id: HostId,
    sender: watch::Sender<Option<ovra_ssh::Sender>>,
    waiting: Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>>,
) {
    let label = window::label(&id);
    let status = |state, failure: Option<&Failure>| {
        let status = Status {
            id: id.as_str().to_owned(),
            state,
            reason: failure.map(|f| f.reason.as_str()),
            details: failure.map(|f| f.details.clone()),
        };
        let message = serde_json::json!({ "sshStatus": status });
        let mailboxes = app.state::<crate::mailbox::Mailboxes>();
        mailboxes.send(&label, message.clone());
        mailboxes.send("main", message);
    };
    status("connecting", None);
    let mut link = match ssh.connect(&id).await {
        Ok(link) => link,
        Err(failure) => return status("failed", Some(&failure)),
    };
    let _ = sender.send(Some(link.sender()));
    status("open", None);
    // Only this task reads the link, apart from the calls that send.
    let failure = loop {
        let frame = match link.recv().await {
            Ok(frame) => frame,
            Err(failure) => break failure,
        };
        match window::route(&frame) {
            Route::Reply(request) => {
                // An id this window never sent waits for nobody.
                if let Some(waiter) = lock(&waiting).remove(&request) {
                    let _ = waiter.send(frame);
                }
            }
            Route::Event => {
                app.state::<crate::mailbox::Mailboxes>()
                    .send(&label, serde_json::json!({ "frame": frame }));
            }
            Route::Drop => {}
        }
    };
    let _ = sender.send(None);
    lock(&waiting).clear();
    status("closed", Some(&failure));
}

/// Opens `url` in the laptop's browser for a remote window, if it points
/// away from the laptop (`ovra_ssh::window::may_open`).
pub(crate) fn open_url(url: &str) -> Result<(), String> {
    let url = window::may_open(url).ok_or_else(|| {
        "Ovra opens only web links that do not point at this computer or its network.".to_owned()
    })?;
    SystemOpener::new()
        .open_url(url.as_str())
        .map_err(|e| e.message)
}
