//! The helper alone plus a few probes, for this crate's tests:
//! `ovra-sandbox-probe --ovra-sandbox-helper ... -- <this> write <path> ...`.
//! `connect` takes `ip:port`. Each probe prints `<what> <path>: yes` if it worked, else `no`.

fn main() {
    #[cfg(windows)]
    ovra_sandbox_windows::helper_if_asked();
    let args: Vec<String> = std::env::args().skip(1).collect();
    for pair in args.chunks(2) {
        let [what, path] = pair else { break };
        let worked = match what.as_str() {
            "write" => std::fs::write(path, "planted").is_ok(),
            "read" => std::fs::read(path).is_ok(),
            "remove" => std::fs::remove_dir_all(path).is_ok() || std::fs::remove_file(path).is_ok(),
            "env" => std::env::var_os(path).is_some(),
            "connect" => path.parse().is_ok_and(|address| {
                std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_secs(2))
                    .is_ok()
            }),
            _ => false,
        };
        println!("{what} {path}: {}", if worked { "yes" } else { "no" });
    }
}
