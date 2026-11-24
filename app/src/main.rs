//! Tauri shell. It forwards frames between the window and the core and does
//! nothing else; all logic lives in `ovra-core`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use ovra_agent_runtime::ClaudeCli;
use ovra_core::{Client, Core};
use ovra_fs_index::FsIndex;
use ovra_mcp_server::IdeServer;
use ovra_opener::SystemOpener;
use ovra_pty::Pty;
use ovra_store::Store;
use ovra_worktree::GitWorktrees;
use tauri::{Emitter, Manager};

/// Carries one client frame to the core and returns the reply frame.
#[tauri::command]
async fn ovra_call(client: tauri::State<'_, Client>, frame: String) -> Result<String, ()> {
    Ok(ovra_transport::dispatch(&client, &frame).await)
}

fn main() {
    let result = tauri::Builder::default()
        // Provides window.Notification to the UI for system notifications.
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = Arc::new(Store::open(&data_dir)?);
            let core = Core::builder("tauri")
                .projects(store.clone())
                .sessions(store.clone())
                .audit(store)
                .fs(Arc::new(FsIndex::new()))
                .opener(Arc::new(SystemOpener::new()))
                .worktrees(Arc::new(GitWorktrees::new()))
                .worktrees_dir(data_dir.join("worktrees"))
                .ide_tools(Arc::new(IdeServer::new()))
                .terminals(Arc::new(Pty::new()))
                .agents(Arc::new(ClaudeCli::new(None, ovra_sandbox::for_this_os())))
                .build();
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
        .invoke_handler(tauri::generate_handler![ovra_call])
        .run(tauri::generate_context!());
    if let Err(err) = result {
        eprintln!("ovra failed to start: {err}");
        std::process::exit(1);
    }
}
