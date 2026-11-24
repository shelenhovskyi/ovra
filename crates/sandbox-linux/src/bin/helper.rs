//! The sandbox helper alone, for tests: `ovra-sandbox-helper
//! --ovra-sandbox-helper ... -- <program>`.

fn main() {
    #[cfg(target_os = "linux")]
    ovra_sandbox_linux::helper_if_asked();
    eprintln!("ovra-sandbox-helper runs only inside the sandbox");
    std::process::exit(2);
}
