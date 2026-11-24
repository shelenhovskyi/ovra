//! Tauri shell. It forwards frames between the window and the core and does
//! nothing else; all logic lives in `ovra-core`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ovra_core::Core;
use tauri::{Emitter, Manager};

/// Carries one client frame to the core and returns the reply frame.
#[tauri::command]
fn ovra_call(core: tauri::State<'_, Core>, frame: String) -> String {
    ovra_transport::dispatch(&core, &frame)
}

fn main() {
    let core = Core::new("tauri");
    let result = tauri::Builder::default()
        .manage(core)
        .setup(|app| {
            let handle = app.handle().clone();
            let mut events = app.state::<Core>().subscribe();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = events.recv().await {
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
