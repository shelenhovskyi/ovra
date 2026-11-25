//! Tauri shell. It forwards frames between the window and the core and does
//! nothing else; all logic lives in `ovra-core`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod menu;

use std::sync::Arc;

use ovra_agent_runtime::ClaudeCli;
use ovra_browser::ChromiumBrowser;
use ovra_core::services::Secrets;
use ovra_core::{Client, Core};
use ovra_forge::GhForge;
use ovra_fs_index::FsIndex;
use ovra_mcp_server::IdeServer;
use ovra_opener::SystemOpener;
use ovra_pty::Pty;
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
            let core = Core::builder("tauri")
                .secrets(secrets.clone())
                .projects(store.clone())
                .sessions(store.clone())
                .audit(store)
                .fs(Arc::new(FsIndex::new()))
                .opener(Arc::new(SystemOpener::new()))
                .worktrees(Arc::new(GitWorktrees::new()))
                .git(Arc::new(GitRepo::new()))
                .forge(Arc::new(GhForge::new()))
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
        .invoke_handler(tauri::generate_handler![ovra_call, ovra_pick_folder])
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
