//! Tauri shell. It forwards frames between the window and the core and does
//! nothing else; all logic lives in `ovra-core`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod menu;

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
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

/// Carries one client frame to the core and returns the reply frame.
#[tauri::command]
async fn ovra_call(client: tauri::State<'_, Client>, frame: String) -> Result<String, ()> {
    Ok(ovra_transport::dispatch(&client, &frame).await)
}

/// Shows the system's folder dialog over `window` and returns the chosen
/// folder, or `None` when the person cancels. The UI hands the path to
/// `projects.add`, which validates it like a typed one. Called from Rust, so
/// the window gets no dialog permission of its own.
#[tauri::command]
async fn ovra_pick_folder(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
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
    let at = at.to_logical::<f64>(window.scale_factor().unwrap_or(1.0));
    let frame = DropFrame {
        id,
        paths: paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        x: at.x,
        y: at.y,
    };
    let _ = window.emit("ovra://drop", frame);
}

/// Copies the drop `drop_id` into the folder `target_dir` of `project`
/// (`FsIndex::import_drop`). With `clash: "ask"` and a name that exists
/// already, nothing is copied, the names come back and the drop stays for
/// the person's answer until it expires; any other outcome uses it up.
#[tauri::command]
async fn ovra_import_drop(
    importer: tauri::State<'_, Importer>,
    drop_id: String,
    project: ProjectId,
    target_dir: String,
    clash: ClashAnswer,
) -> Result<ImportReply, String> {
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
        } => ImportReply::Copied {
            paths: paths.iter().map(|p| p.as_str().to_owned()).collect(),
            skipped_links,
        },
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
            app.manage(Importer {
                fs: fs.clone(),
                projects: store.clone(),
                drops: Arc::new(Drops::new()),
            });
            let core = Core::builder("tauri")
                .secrets(secrets.clone())
                .projects(store.clone())
                .sessions(store.clone())
                .audit(store)
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
                .agents(Arc::new(ClaudeCli::new(
                    None,
                    ovra_sandbox::for_this_os(&data_dir),
                )))
                .dev_servers(Arc::new(PodServers::new(
                    ovra_sandbox::for_this_os(&data_dir),
                    servers_dir.clone(),
                )))
                .browser(Arc::new(ChromiumBrowser::new(
                    ovra_sandbox::for_this_os(&data_dir),
                    None,
                )))
                .build();
            app.manage(ServersDir(servers_dir));
            let mut events = core.subscribe();
            // One window, one client.
            app.manage(core.connect());
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // A lagging window gets `info.resync` and keeps receiving.
                while let Some(event) = Core::next_event(&mut events).await {
                    let _ = handle.emit("ovra://frame", ovra_transport::event_frame(event));
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, position }) =
                event
            {
                on_drop(window, paths, *position);
            }
        })
        .invoke_handler(tauri::generate_handler![
            ovra_call,
            ovra_pick_folder,
            ovra_import_drop
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
