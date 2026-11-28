fn main() {
    // Listing the window's commands makes each one need an entry in
    // `capabilities/`, so a new command cannot skip that review.
    let manifest = tauri_build::AppManifest::new().commands(&[
        "ovra_call",
        "ovra_subscribe",
        "ovra_pick_folder",
        "ovra_import_drop",
        "ovra_remote_show",
        "ovra_remote_reconnect",
        "ovra_remote_disconnect",
        "ovra_remote_stop",
        "ovra_remote_restart",
        "ovra_remote_install",
        "ovra_open_url",
    ]);
    remote_builds();
    if let Err(err) = tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
    {
        panic!("{err:#}");
    }
}

/// Fixes the SHA-256 of each Linux build of `ovra-daemon` the app installs
/// on remote machines (ADR 0016) as `OVRA_REMOTE_SHA256_<ARCH>`, from
/// `OVRA_REMOTE_DAEMON_DIR` or else `remote/`. The app uploads a build only
/// if its bytes still hash to this. A development build also reads them
/// from that folder (`OVRA_REMOTE_DIR`).
fn remote_builds() {
    use sha2::Digest as _;
    println!("cargo:rerun-if-env-changed=OVRA_REMOTE_DAEMON_DIR");
    let dir = std::env::var_os("OVRA_REMOTE_DAEMON_DIR").map_or_else(
        || std::path::Path::new("remote").to_path_buf(),
        std::path::PathBuf::from,
    );
    // A path that does not exist would run this script on every build.
    if dir.exists() {
        println!("cargo:rerun-if-changed={}", dir.display());
    }
    println!("cargo:rustc-env=OVRA_REMOTE_DIR={}", dir.display());
    for arch in ["x86_64", "aarch64"] {
        let file = dir.join(format!("ovra-daemon-{arch}-unknown-linux-musl"));
        if let Ok(bytes) = std::fs::read(&file) {
            println!("cargo:rerun-if-changed={}", file.display());
            let digest = sha2::Sha256::digest(&bytes);
            let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
            println!(
                "cargo:rustc-env=OVRA_REMOTE_SHA256_{}={hex}",
                arch.to_uppercase()
            );
        }
    }
}
