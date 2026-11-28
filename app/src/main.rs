//! Tauri shell. It forwards frames between the window and the core and does
//! nothing else; all logic lives in `ovra-core`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod mailbox;
mod menu;
mod remote;

use std::sync::Arc;
use std::time::Instant;

use ovra_agent_runtime::ClaudeCli;
use ovra_browser::ChromiumBrowser;
use ovra_core::path::RelPath;
use ovra_core::protocol::projects::ProjectId;
use ovra_core::services::{ProjectStore, Secrets};
use ovra_core::{Client, Core};
use ovra_forge::GhForge;
use ovra_fs_index::{Clash, Drops, FsIndex, Imported};
use ovra_mcp_server::IdeServer;
use ovra_opener::SystemOpener;
use ovra_pty::Pty;
use ovra_resources::SysResources;
use ovra_secrets::KeychainSecrets;
use ovra_servers::PodServers;
use ovra_store::Store;
use ovra_worktree::{GitRepo, GitWorktrees};
use serde_json::json;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

/// Carries one client frame to the core of the calling window and returns
/// the reply frame: the laptop's for `main`, the machine's for `ssh-<id>`.
#[tauri::command]
async fn ovra_call(
    window: tauri::WebviewWindow,
    client: tauri::State<'_, Client>,
    remotes: tauri::State<'_, remote::Remotes>,
    frame: String,
) -> Result<String, ()> {
    if window.label() == "main" {
        return Ok(ovra_transport::dispatch(&client, &frame).await);
    }
    match ovra_ssh::window::machine(window.label()) {
        Some(id) => Ok(remotes.call(&id, frame).await),
        None => Err(()),
    }
}

/// The calling window's channel for everything the app tells it: frames,
/// menu commands, drops and link status (`mailbox`).
#[tauri::command]
fn ovra_subscribe(
    window: tauri::WebviewWindow,
    mailboxes: tauri::State<'_, mailbox::Mailboxes>,
    remotes: tauri::State<'_, remote::Remotes>,
    channel: tauri::ipc::Channel<serde_json::Value>,
) -> Result<(), ()> {
    let label = window.label();
    if label != "main" && ovra_ssh::window::machine(label).is_none() {
        return Err(());
    }
    mailboxes.set(label, channel);
    remotes.replay(&mailboxes, label);
    Ok(())
}

/// Refuses a command meant for the main window from any other, should a
/// capability ever grant it to one.
fn main_only(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("only the main window may do this".to_owned())
    }
}

/// Opens the window of remote machine `id`, or focuses it, and connects.
/// For the main window only.
#[tauri::command]
fn ovra_remote_show(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    remotes: tauri::State<'_, remote::Remotes>,
    id: String,
) -> Result<(), String> {
    main_only(&window)?;
    let id = ovra_core::ssh::HostId::parse(&id).map_err(|e| e.message)?;
    remotes.show(&app, &id)
}

/// Connects the calling remote window to its machine again.
#[tauri::command]
fn ovra_remote_reconnect(
    window: tauri::WebviewWindow,
    remotes: tauri::State<'_, remote::Remotes>,
) -> Result<(), String> {
    let id = ovra_ssh::window::machine(window.label()).ok_or("not a remote window")?;
    remotes.connect(window.app_handle(), &id);
    Ok(())
}

/// Ends the calling remote window's link; the window stays.
#[tauri::command]
fn ovra_remote_disconnect(
    window: tauri::WebviewWindow,
    remotes: tauri::State<'_, remote::Remotes>,
) -> Result<(), String> {
    let id = ovra_ssh::window::machine(window.label()).ok_or("not a remote window")?;
    remotes.close(window.app_handle(), &id);
    Ok(())
}

/// Ends the core on the calling remote window's machine; the window stays.
#[tauri::command]
async fn ovra_remote_stop(
    window: tauri::WebviewWindow,
    remotes: tauri::State<'_, remote::Remotes>,
) -> Result<(), String> {
    let id = ovra_ssh::window::machine(window.label()).ok_or("not a remote window")?;
    remotes.stop(&id).await
}

/// Restarts Ovra on machine `id` with the installed build and opens its
/// window. For the main window only: the person confirmed it there.
#[tauri::command]
fn ovra_remote_restart(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    remotes: tauri::State<'_, remote::Remotes>,
    id: String,
) -> Result<(), String> {
    main_only(&window)?;
    let id = ovra_core::ssh::HostId::parse(&id).map_err(|e| e.message)?;
    remotes.restart(&app, &id)
}

/// Installs this laptop's build of Ovra on machine `id`. For the main
/// window only: the person clicked Install there.
#[tauri::command]
async fn ovra_remote_install(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    remotes: tauri::State<'_, remote::Remotes>,
    id: String,
) -> Result<(), String> {
    main_only(&window)?;
    let id = ovra_core::ssh::HostId::parse(&id).map_err(|e| e.message)?;
    remotes.install(&app, &id).await
}

/// Opens a web link from a remote window in the laptop's browser, unless
/// it points at this computer or its network.
#[tauri::command]
fn ovra_open_url(url: String) -> Result<(), String> {
    remote::open_url(&url)
}

/// Shows the system's folder dialog over `window` and returns the chosen
/// folder, or `None` when the person cancels. The UI hands the path to
/// `projects.add`, which validates it like a typed one. Called from Rust, so
/// the window gets no dialog permission of its own.
#[tauri::command]
async fn ovra_pick_folder(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    main_only(&window)?;
    let dialog = window.dialog().file().set_parent(&window);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_folder())
        .await
        .map_err(|err| format!("The folder dialog failed: {err}"))?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|_| "That folder is not on this computer.".to_owned())?;
    path.into_os_string()
        .into_string()
        .map(Some)
        .map_err(|_| "Ovra cannot open folders whose path is not valid Unicode yet.".to_owned())
}

/// What `ovra_import_drop` copies with. `drops` holds the paths of the
/// last drop onto the window, in Rust: the window gets only their id
/// (`ovra://drop`), so no text a page sends names a file outside a
/// project. Headless mode has no drops.
struct Importer {
    fs: Arc<FsIndex>,
    projects: Arc<Store>,
    drops: Arc<Drops>,
    /// Told what an import changed, so a direct session never counts it
    /// as its agent's.
    core: Core,
}

/// What the window learns about a drop: its id, the paths (for opening a
/// dropped folder as a project, through `projects.add` like a typed path)
/// and where it landed, in CSS pixels.
#[derive(Clone, serde::Serialize)]
struct DropFrame {
    id: String,
    paths: Vec<String>,
    x: f64,
    y: f64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum ClashAnswer {
    Ask,
    Replace,
    KeepBoth,
}

#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ImportReply {
    Clashes {
        names: Vec<String>,
    },
    Copied {
        paths: Vec<String>,
        skipped_links: Vec<String>,
    },
}

/// Keeps a drop onto the main window under a new id and tells the window.
fn on_drop(window: &tauri::Window, paths: &[std::path::PathBuf], at: tauri::PhysicalPosition<f64>) {
    if window.label() != "main" {
        return;
    }
    let Some(importer) = window.try_state::<Importer>() else {
        return;
    };
    let id = importer.drops.put(paths.to_vec(), Instant::now());
    let at = css_point(window, at);
    let frame = DropFrame {
        id,
        paths: paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        x: at.x,
        y: at.y,
    };
    if let Some(mailboxes) = window.try_state::<mailbox::Mailboxes>() {
        mailboxes.send("main", json!({ "drop": frame }));
    }
}

/// Where files dragged in from outside are while they are over the main
/// window, in CSS pixels (`ovra://drag-over`); null once they leave or drop.
fn on_drag_over(window: &tauri::Window, at: Option<tauri::PhysicalPosition<f64>>) {
    if window.label() != "main" {
        return;
    }
    let at = at.map(|at| css_point(window, at));
    if let Some(mailboxes) = window.try_state::<mailbox::Mailboxes>() {
        mailboxes.send("main", json!({ "dragOver": at }));
    }
}

/// A drag position from the window's drop handler in CSS pixels. Tauri
/// calls it physical, but only WebView2 reports device pixels: WebKit on
/// macOS (points) and WebKitGTK report CSS pixels already, so dividing
/// there put a Retina drop at half its place.
fn css_point(
    window: &tauri::Window,
    at: tauri::PhysicalPosition<f64>,
) -> tauri::LogicalPosition<f64> {
    if cfg!(windows) {
        at.to_logical(window.scale_factor().unwrap_or(1.0))
    } else {
        tauri::LogicalPosition::new(at.x, at.y)
    }
}

/// Copies the drop `drop_id` into the folder `target_dir` of `project`
/// (`FsIndex::import_drop`). With `clash: "ask"` and a name that exists
/// already, nothing is copied, the names come back and the drop stays for
/// the person's answer until it expires; any other outcome uses it up.
#[tauri::command]
async fn ovra_import_drop(
    window: tauri::WebviewWindow,
    importer: tauri::State<'_, Importer>,
    drop_id: String,
    project: ProjectId,
    target_dir: String,
    clash: ClashAnswer,
) -> Result<ImportReply, String> {
    main_only(&window)?;
    let into = RelPath::parse(&target_dir).map_err(|e| e.message)?;
    let root = importer.projects.get(&project).map_err(|e| e.message)?.root;
    let clash = match clash {
        ClashAnswer::Ask => Clash::Ask,
        ClashAnswer::Replace => Clash::Replace,
        ClashAnswer::KeepBoth => Clash::KeepBoth,
    };
    let (fs, drops) = (Arc::clone(&importer.fs), Arc::clone(&importer.drops));
    let done = tauri::async_runtime::spawn_blocking(move || {
        let root = std::path::Path::new(&root);
        fs.import_drop(&drops, &drop_id, Instant::now(), root, &into, clash)
    })
    .await
    .map_err(|err| format!("Copying failed: {err}"))?;
    Ok(match done.map_err(|e| e.message)? {
        Imported::Clashes(names) => ImportReply::Clashes { names },
        Imported::Copied {
            paths,
            skipped_links,
        } => {
            // Replaced entries are among `paths`: they keep their names.
            importer.core.person_changed(&project, &paths);
            ImportReply::Copied {
                paths: paths.iter().map(|p| p.as_str().to_owned()).collect(),
                skipped_links,
            }
        }
    })
}

/// Tells the UI, before it starts, that the window draws its content under
/// the title bar (`titleBarStyle: Overlay`, macOS only), so the title bar
/// leaves room for the traffic lights. The UI never asks which OS it runs on.
fn window_chrome<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    let script = if cfg!(target_os = "macos") {
        "(() => {
          const set = () => document.documentElement.setAttribute('data-chrome', 'overlay');
          if (document.documentElement) set();
          else document.addEventListener('readystatechange', set, { once: true });
        })();"
    } else {
        ""
    };
    tauri::plugin::Builder::new("ovra-chrome")
        .js_init_script(script)
        .build()
}

/// The pods' folder (`ovra_servers::servers_dir`), removed when the app
/// exits.
struct ServersDir(std::path::PathBuf);

fn main() {
    // Inside an agent's sandbox this process is the sandbox's helper.
    ovra_sandbox::helper_if_asked();
    // As a server pod, this process is the pod's init.
    ovra_servers::pod_init_if_asked();
    // Started from the Finder or a launcher, the app lacks the user's PATH.
    ovra_sandbox::use_login_path();
    let builder = tauri::Builder::default();
    // On macOS the menu bar runs the UI's commands; elsewhere there is none.
    let builder = if cfg!(target_os = "macos") {
        builder.menu(menu::build).on_menu_event(menu::forward)
    } else {
        builder
    };
    let result = builder
        // Provides window.Notification to the UI for system notifications.
        .plugin(tauri_plugin_notification::init())
        // Folder dialogs, opened from Rust only (`ovra_pick_folder`).
        .plugin(tauri_plugin_dialog::init())
        .plugin(window_chrome())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = Arc::new(Store::open(&data_dir)?);
            let secrets: Arc<dyn Secrets> = Arc::new(KeychainSecrets::new(&data_dir));
            let servers_dir = ovra_servers::servers_dir()?;
            let fs = Arc::new(FsIndex::new());
            let ssh = Arc::new(ovra_ssh::OpenSsh::new(&data_dir));
            // Phone access (ADR 0014) serves the mobile app from the
            // app's own embedded files.
            let resolver = app.handle().clone();
            let remote = Arc::new(ovra_remote::RemoteServer::new(
                &data_dir.join("remote"),
                Arc::new(move |path: &str| {
                    let asset = resolver.asset_resolver().get(path.to_owned())?;
                    Some(ovra_remote::Asset::new(path, asset.bytes))
                }),
            ));
            let core = Core::builder("tauri")
                .secrets(secrets.clone())
                .projects(store.clone())
                .sessions(store.clone())
                .audit(store.clone())
                .fs(fs.clone())
                .opener(Arc::new(SystemOpener::new()))
                .worktrees(Arc::new(GitWorktrees::new()))
                .git(Arc::new(GitRepo::new()))
                .forge(Arc::new(GhForge::new()))
                .resources(Arc::new(SysResources::new()))
                .keep_awake(Arc::new(ovra_power::SystemKeepAwake::default()))
                .worktrees_dir(data_dir.join("worktrees"))
                .ide_tools(Arc::new(IdeServer::new()))
                .terminals(Arc::new(Pty::new()))
                .netproxy(Arc::new(ovra_netproxy::NetProxies::from_env(secrets)?))
                .lsp(Arc::new(ovra_lsp::Servers::new(
                    ovra_sandbox::for_this_os(&data_dir),
                    data_dir.join("lsp"),
                )))
                .agents(Arc::new(ovra_core::runtimes::AgentRuntimes::new(vec![
                    Arc::new(ClaudeCli::new(None, ovra_sandbox::for_this_os(&data_dir))),
                    Arc::new(ovra_acp_client::AcpAgents::new(
                        ovra_acp_client::known(),
                        ovra_sandbox::for_this_os(&data_dir),
                    )),
                ])))
                .dev_servers(Arc::new(PodServers::new(
                    ovra_sandbox::for_this_os(&data_dir),
                    servers_dir.clone(),
                )))
                .browser(Arc::new(ChromiumBrowser::new(
                    ovra_sandbox::for_this_os(&data_dir),
                    None,
                )))
                .remote(remote.clone())
                .ssh(ssh.clone())
                .sandbox(ovra_sandbox::for_this_os(&data_dir))
                .build();
            let restored = core.clone();
            tauri::async_runtime::spawn(async move { remote.restore(restored) });
            app.manage(ServersDir(servers_dir));
            app.manage(remote::Remotes::new(ssh));
            app.manage(mailbox::Mailboxes::default());
            app.manage(Importer {
                fs: fs.clone(),
                projects: store.clone(),
                drops: Arc::new(Drops::new()),
                core: core.clone(),
            });
            let mut events = core.subscribe();
            // One window, one client.
            app.manage(core.connect());
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // A lagging window gets `info.resync` and keeps receiving.
                while let Some(event) = Core::next_event(&mut events).await {
                    // The laptop's events reach its own window only.
                    let frame = ovra_transport::event_frame(event);
                    handle
                        .state::<mailbox::Mailboxes>()
                        .send("main", json!({ "frame": frame }));
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(mailboxes) = window.try_state::<mailbox::Mailboxes>() {
                    mailboxes.forget(window.label());
                }
                if let Some(id) = ovra_ssh::window::machine(window.label())
                    && let Some(remotes) = window.try_state::<remote::Remotes>()
                {
                    remotes.close(window.app_handle(), &id);
                }
            }
            let tauri::WindowEvent::DragDrop(drag) = event else {
                return;
            };
            match drag {
                tauri::DragDropEvent::Enter { position, .. }
                | tauri::DragDropEvent::Over { position } => on_drag_over(window, Some(*position)),
                tauri::DragDropEvent::Drop { paths, position } => {
                    on_drag_over(window, None);
                    on_drop(window, paths, *position);
                }
                _ => on_drag_over(window, None),
            }
        })
        .invoke_handler(tauri::generate_handler![
            ovra_call,
            ovra_subscribe,
            ovra_pick_folder,
            ovra_import_drop,
            ovra_remote_show,
            ovra_remote_reconnect,
            ovra_remote_disconnect,
            ovra_remote_stop,
            ovra_remote_restart,
            ovra_remote_install,
            ovra_open_url
        ])
        .build(tauri::generate_context!());
    match result {
        Ok(app) => app.run(|app, event| {
            if let tauri::RunEvent::Exit = event
                && let Some(dir) = app.try_state::<ServersDir>()
            {
                let _ = std::fs::remove_dir_all(&dir.0);
            }
        }),
        Err(err) => {
            eprintln!("ovra failed to start: {err}");
            std::process::exit(1);
        }
    }
}
