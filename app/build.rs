fn main() {
    // Listing the window's commands makes each one need an entry in
    // `capabilities/`, so a new command cannot skip that review.
    let manifest = tauri_build::AppManifest::new().commands(&[
        "ovra_call",
        "ovra_pick_folder",
        "ovra_import_drop",
    ]);
    if let Err(err) = tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
    {
        panic!("{err:#}");
    }
}
