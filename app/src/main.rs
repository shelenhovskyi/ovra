//! Tauri shell. It forwards frames between the window and the core and does
//! nothing else; all logic lives in `ovra-core`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use ovra_core::Core;
use ovra_fs_index::FsIndex;
use ovra_opener::SystemOpener;
use ovra_store::Store;
use tauri::{Emitter, Manager};

/// Carries one client frame to the core and returns the reply frame.
#[tauri::command]
async fn ovra_call(core: tauri::State<'_, Core>, frame: String) -> Result<String, ()> {
    Ok(ovra_transport::dispatch(&core, &frame).await)
}

fn main() {
    let result = tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let core = Core::builder("tauri")
                .projects(Arc::new(Store::open(&data_dir)?))
                .fs(Arc::new(FsIndex::new()))
                .opener(Arc::new(SystemOpener::new()))
                .build();
            app.manage(core);
            let handle = app.handle().clone();
            let mut events = app.state::<Core>().subscribe();
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
