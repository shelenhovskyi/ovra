//! The program inside the attack tests' sandboxes: the sandbox helper first,
//! as in the daemon and the app, then probes a shell script cannot do on
//! its own. Each probe exits 0 only if the attack worked.

use std::process::ExitCode;

// Without Unix sockets every arm returns, so the rest is unreachable.
#[cfg_attr(not(unix), allow(unreachable_code, unused_variables))]
fn main() -> ExitCode {
    ovra_sandbox::helper_if_asked();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let worked = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        #[cfg(unix)]
        ["connect-unix", path] => std::os::unix::net::UnixStream::connect(path).is_ok(),
        #[cfg(target_os = "linux")]
        ["connect-abstract", name] => connect_abstract(name),
        _ => {
            eprintln!("usage: connect-unix PATH | connect-abstract NAME");
            return ExitCode::from(2);
        }
    };
    if worked {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Connects to an abstract Unix socket, the kind D-Bus and some agents use.
#[cfg(target_os = "linux")]
fn connect_abstract(name: &str) -> bool {
    use std::os::linux::net::SocketAddrExt as _;
    use std::os::unix::net::{SocketAddr, UnixStream};
    SocketAddr::from_abstract_name(name).is_ok_and(|addr| UnixStream::connect_addr(&addr).is_ok())
}
