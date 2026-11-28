//! Remote machines' windows (ADR 0016): one window per machine, labelled
//! `ssh-<id>`, bridged to that machine's core over `ovra_ssh::Link`.
//! Frames are routed by the window that sent them, never by their content.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use ovra_core::protocol::ssh::RemoteHost;
use ovra_core::services::{Opener, RemoteHosts};
use ovra_core::ssh::HostId;
use ovra_opener::SystemOpener;
use ovra_ssh::install::{Arch, Build, Sha256, UPLOAD_MAX};
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
    /// The last status of each link, by machine id, for a window that
    /// subscribes after it was sent.
    statuses: Statuses,
}

type Statuses = Arc<Mutex<HashMap<String, serde_json::Value>>>;

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
            statuses: Statuses::default(),
        }
    }

    /// Hands a window that just subscribed the status it may have missed.
    pub(crate) fn replay(&self, mailboxes: &crate::mailbox::Mailboxes, label: &str) {
        for status in replay_for(label, &lock(&self.statuses)) {
            mailboxes.send(label, status);
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
        self.start(app, id, false);
    }

    /// Ends the core on `id` and starts the installed build there
    /// (`--attach --restart`), opening its window. For the main window,
    /// after the person confirmed that the agents there stop.
    pub(crate) fn restart(&self, app: &AppHandle, id: &HostId) -> Result<(), String> {
        let host = self
            .host(id)
            .ok_or_else(|| "Ovra knows no such remote machine.".to_owned())?;
        self.close(app, id);
        if app.get_webview_window(&window::label(id)).is_none() {
            build(app, id, &host)?;
        }
        self.start(app, id, true);
        Ok(())
    }

    /// Installs this laptop's build of Ovra on `id`, telling the main
    /// window how far the upload is; an open window of the machine then
    /// connects again. For the main window only, after the person's click.
    pub(crate) async fn install(&self, app: &AppHandle, id: &HostId) -> Result<(), String> {
        let mailboxes = app.state::<crate::mailbox::Mailboxes>();
        let progress = |sent: u64, total: u64| {
            let message = serde_json::json!({
                "sshInstall": { "id": id.as_str(), "sent": sent, "total": total }
            });
            mailboxes.send("main", message);
        };
        self.ssh
            .install(id, |arch| build_for(app, arch), progress)
            .await
            .map_err(|f| {
                if f.details.is_empty() {
                    format!(
                        "Ovra could not install itself there ({}).",
                        f.reason.as_str()
                    )
                } else {
                    f.details
                }
            })?;
        if app.get_webview_window(&window::label(id)).is_some() {
            self.close(app, id);
            self.connect(app, id);
        }
        Ok(())
    }

    /// Ends the core on `id` (its agents stop, sessions stay resumable)
    /// and closes the link. Only the app sends the quit line.
    pub(crate) async fn stop(&self, app: &AppHandle, id: &HostId) -> Result<(), String> {
        let open = lock(&self.open).get(id.as_str()).cloned();
        let sender = open.and_then(|o| o.sender.borrow().clone());
        let Some(sender) = sender else {
            return Err("Ovra is not connected to this machine.".to_owned());
        };
        sender.quit().await.map_err(|f| f.details)?;
        self.end(app, id, "stopped");
        Ok(())
    }

    fn start(&self, app: &AppHandle, id: &HostId, restart: bool) {
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
            self.statuses.clone(),
            sender_tx,
            waiting.clone(),
            restart,
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
    pub(crate) fn close(&self, app: &AppHandle, id: &HostId) {
        self.end(app, id, "closed");
    }

    fn end(&self, app: &AppHandle, id: &HostId, state: &'static str) {
        // Held until `closed` is told, so a `connect` in between cannot
        // have its `connecting` overwritten by this older `closed`.
        let mut open = lock(&self.open);
        if let Some(link) = open.remove(id.as_str())
            && !link.task.inner().is_finished()
        {
            link.task.abort();
            tell(app, &self.statuses, id, state, None);
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

/// This laptop's build of `ovra-daemon` for `arch`, if the app was built
/// with one: its bytes must still hash to the SHA-256 fixed at build time
/// (`build.rs`). A development build reads it from `OVRA_REMOTE_DAEMON_DIR`
/// as it was then, a release from the app's resources.
fn build_for(app: &AppHandle, arch: Arch) -> Result<Build, String> {
    let expected = match arch {
        Arch::X86_64 => option_env!("OVRA_REMOTE_SHA256_X86_64"),
        Arch::Aarch64 => option_env!("OVRA_REMOTE_SHA256_AARCH64"),
    };
    let dir = if cfg!(debug_assertions) {
        std::path::PathBuf::from(env!("OVRA_REMOTE_DIR"))
    } else {
        app.path()
            .resource_dir()
            .map_err(|e| e.to_string())?
            .join("remote")
    };
    build_in(&dir, arch, expected)
}

/// The build for `arch` in `dir`, checked against `expected`.
fn build_in(dir: &std::path::Path, arch: Arch, expected: Option<&str>) -> Result<Build, String> {
    use std::io::Read as _;
    let none = || {
        format!(
            "This Ovra has no build for {} Linux. Install `ovra-daemon` there yourself.",
            arch.as_str()
        )
    };
    let expected = expected.and_then(Sha256::parse).ok_or_else(none)?;
    let file = std::fs::File::open(dir.join(arch.file_name())).map_err(|_| none())?;
    let mut bytes = Vec::new();
    file.take(UPLOAD_MAX as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Build::checked(bytes, &expected)
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

/// The statuses a window that subscribes is owed: its own machine's, or
/// every machine's for the main window (which the spec gives every
/// status), and none for any other label.
fn replay_for(
    label: &str,
    statuses: &HashMap<String, serde_json::Value>,
) -> Vec<serde_json::Value> {
    if label == "main" {
        return statuses.values().cloned().collect();
    }
    window::machine(label)
        .and_then(|id| statuses.get(id.as_str()).cloned())
        .into_iter()
        .collect()
}

/// The app message for a link's state; never in-band, so the machine
/// cannot fake it.
fn status_message(
    id: &HostId,
    state: &'static str,
    failure: Option<&Failure>,
) -> serde_json::Value {
    let status = Status {
        id: id.as_str().to_owned(),
        state,
        reason: failure.map(|f| f.reason.as_str()),
        details: failure.map(|f| f.details.clone()),
    };
    serde_json::json!({ "sshStatus": status })
}

/// Keeps the link's new state for later subscribers and tells the
/// machine's window and the main window.
fn tell(
    app: &AppHandle,
    statuses: &Statuses,
    id: &HostId,
    state: &'static str,
    failure: Option<&Failure>,
) {
    let message = status_message(id, state, failure);
    lock(statuses).insert(id.as_str().to_owned(), message.clone());
    let mailboxes = app.state::<crate::mailbox::Mailboxes>();
    mailboxes.send(&window::label(id), message.clone());
    mailboxes.send("main", message);
}

async fn run(
    app: AppHandle,
    ssh: Arc<OpenSsh>,
    id: HostId,
    statuses: Statuses,
    sender: watch::Sender<Option<ovra_ssh::Sender>>,
    waiting: Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>>,
    restart: bool,
) {
    let label = window::label(&id);
    let status = |state, failure: Option<&Failure>| tell(&app, &statuses, &id, state, failure);
    status("connecting", None);
    let connected = if restart {
        ssh.restart(&id).await
    } else {
        ssh.connect(&id).await
    };
    let mut link = match connected {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> HostId {
        HostId::parse(text).unwrap_or_else(|e| panic!("{}", e.message))
    }

    fn statuses() -> HashMap<String, serde_json::Value> {
        HashMap::from([
            ("aaa".to_owned(), status_message(&id("aaa"), "open", None)),
            ("bbb".to_owned(), status_message(&id("bbb"), "closed", None)),
        ])
    }

    #[test]
    fn a_remote_window_is_owed_only_its_own_machines_status() {
        let statuses = statuses();
        assert_eq!(
            replay_for("ssh-aaa", &statuses),
            vec![statuses["aaa"].clone()]
        );
        assert!(replay_for("ssh-ccc", &statuses).is_empty());
        assert!(replay_for("other", &statuses).is_empty());
        assert!(replay_for("ssh-", &statuses).is_empty());
        let mut main = replay_for("main", &statuses);
        main.sort_by_key(ToString::to_string);
        assert_eq!(main, vec![statuses["aaa"].clone(), statuses["bbb"].clone()]);
    }

    #[test]
    fn a_build_is_uploaded_only_when_the_app_was_built_with_its_hash() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let none = build_in(dir.path(), Arch::X86_64, None).err();
        assert_eq!(
            none.as_deref(),
            Some("This Ovra has no build for x86_64 Linux. Install `ovra-daemon` there yourself.")
        );
        // "abc", hashed.
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let missing = build_in(dir.path(), Arch::Aarch64, Some(hash)).err();
        assert!(missing.is_some_and(|e| e.contains("aarch64")));
        let file = dir.path().join(Arch::X86_64.file_name());
        std::fs::write(&file, "abc").unwrap_or_else(|e| panic!("{e}"));
        assert!(build_in(dir.path(), Arch::X86_64, Some(hash)).is_ok());
        std::fs::write(&file, "abd").unwrap_or_else(|e| panic!("{e}"));
        assert!(build_in(dir.path(), Arch::X86_64, Some(hash)).is_err());
    }

    #[test]
    fn a_status_names_its_machine_and_why_it_ended() {
        let failure = Failure {
            reason: ovra_ssh::Reason::AuthFailed,
            details: "denied".into(),
        };
        assert_eq!(
            status_message(&id("aaa"), "failed", Some(&failure)),
            serde_json::json!({ "sshStatus": {
                "id": "aaa", "state": "failed", "reason": "auth_failed", "details": "denied"
            }})
        );
    }
}
