//! The sandbox helper alone, for tests: `ovra-sandbox-helper
//! --ovra-sandbox-helper ... -- <program>`. For the tests only, it also
//! plays a pod's listeners: `ovra-sandbox-helper --listen <port> <socket>`
//! listens on `127.0.0.1:<port>` and on the Unix socket, then says so.

fn main() {
    #[cfg(target_os = "linux")]
    {
        ovra_sandbox_linux::helper_if_asked();
        listen_if_asked();
    }
    eprintln!("ovra-sandbox-helper runs only inside the sandbox");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn listen_if_asked() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [flag, port, socket] = args.as_slice() else {
        return;
    };
    if flag != "--listen" {
        return;
    }
    let tcp = port
        .parse::<u16>()
        .map_err(std::io::Error::other)
        .and_then(|port| std::net::TcpListener::bind(("127.0.0.1", port)));
    let unix = std::os::unix::net::UnixListener::bind(socket);
    match (tcp, unix) {
        (Ok(_), Ok(_)) => println!("listening"),
        (tcp, unix) => eprintln!("tcp: {:?}, unix: {:?}", tcp.err(), unix.err()),
    }
    std::process::exit(0);
}
