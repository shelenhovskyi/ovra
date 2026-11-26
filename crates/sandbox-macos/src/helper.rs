//! The helper: runs outside the sandbox, starts the netproxy bridge, puts
//! its port in the profile, and then becomes `sandbox-exec` with `exec`, so
//! the process Ovra started is the sandboxed agent itself.
//!
//! macOS has no network namespace, so the bridge cannot listen inside the
//! sandbox as on Linux: it listens on a free port of the Mac's loopback,
//! outside, and the profile lets the sandbox reach only that port. Any
//! process on the Mac can connect to it, so it relays only connections
//! whose first request carries the session's token (`Proxy-Authorization`,
//! from the user part of the proxy URL). It stops once the agent has
//! ended.
//!
//! The helper starts a session of its own (`setsid`) before `exec`, so the
//! agent has no controlling terminal: it cannot type into the person's
//! shell when Ovra was started from one.
//!
//! The daemon and the app are the helper themselves: they call
//! [`helper_if_asked`] before anything else.

use std::ffi::OsString;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::sbpl::PORT_MARK;
use crate::{HELPER_FLAG, SANDBOX_EXEC};

/// Runs the netproxy bridge instead of an agent.
const BRIDGE_FLAG: &str = "--ovra-sandbox-bridge";

/// How often the bridge checks that the agent still runs.
const WATCH: Duration = Duration::from_millis(250);

/// The user name in the proxy URL; the password is the session's token.
const PROXY_USER: &str = "ovra";

/// The most a client may send before its first request's head ends.
const MAX_HEAD: usize = 64 * 1024;

/// How long a client has to send its first request's head.
const HEAD_WAIT: Duration = Duration::from_secs(10);

/// What a client without the session's token gets.
const REFUSED: &[u8] = b"HTTP/1.1 407 Proxy Authentication Required\r\n\
Proxy-Authenticate: Basic realm=\"ovra\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// If this process was started as the sandbox helper (or its bridge), does
/// that job and never returns. Otherwise returns at once. Call it first in
/// `main`, before any threads start.
pub fn helper_if_asked() {
    let mut args = std::env::args_os().skip(1);
    match args.next() {
        Some(flag) if flag == HELPER_FLAG => {
            let error = run(args.collect());
            eprintln!("ovra-sandbox: {error}");
            std::process::exit(126);
        }
        Some(flag) if flag == BRIDGE_FLAG => {
            let error = bridge(args.collect());
            eprintln!("ovra-sandbox bridge: {error}");
            std::process::exit(1);
        }
        _ => {}
    }
}

/// The helper's arguments, as `Plan::line` writes them.
#[derive(Debug, Default, PartialEq, Eq)]
struct Args {
    /// The netproxy's socket.
    bridge: Option<OsString>,
    /// The only variables the agent gets.
    env: Vec<OsString>,
    profile: String,
    /// The program and its arguments.
    line: Vec<OsString>,
}

fn parse(args: Vec<OsString>) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut profile = None;
    let mut args = args.into_iter();
    let value = |args: &mut std::vec::IntoIter<OsString>, flag: &str| {
        args.next().ok_or_else(|| format!("{flag} needs a value"))
    };
    loop {
        match args.next().as_ref().and_then(|a| a.to_str()) {
            Some("--bridge") => parsed.bridge = Some(value(&mut args, "--bridge")?),
            Some("--env") => parsed.env.push(value(&mut args, "--env")?),
            Some("--profile") => {
                let text = value(&mut args, "--profile")?;
                profile = Some(text.into_string().map_err(|_| "the profile is not UTF-8")?);
            }
            Some("--") => break,
            _ => return Err("unknown or missing arguments".into()),
        }
    }
    parsed.profile = profile.ok_or("no profile")?;
    parsed.line = args.collect();
    if parsed.line.is_empty() {
        return Err("no program".into());
    }
    // A profile that names the bridge's port needs the bridge, and the
    // other way round: neither may be left out silently.
    if parsed.profile.contains(PORT_MARK) != parsed.bridge.is_some() {
        return Err("the profile and the bridge do not match".into());
    }
    Ok(parsed)
}

/// Starts the bridge if asked, then becomes `sandbox-exec` running the
/// line. Returns only on failure.
fn run(args: Vec<OsString>) -> String {
    let mut args = match parse(args) {
        Ok(args) => args,
        Err(e) => return e,
    };
    if let Err(e) = leave_terminal() {
        return format!("cannot leave the terminal: {e}");
    }
    let mut env: Vec<(OsString, OsString)> = args
        .env
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (name.clone(), value)))
        .collect();
    if let Some(socket) = &args.bridge {
        let token = match token() {
            Ok(token) => token,
            Err(e) => return format!("no token for the network bridge: {e}"),
        };
        let port = match start_bridge(socket, &token) {
            Ok(port) => port,
            Err(e) => return format!("the network bridge did not start: {e}"),
        };
        args.profile = args.profile.replace(PORT_MARK, &port.to_string());
        env.extend(proxy_env(port, &token));
    }
    let [program, rest @ ..] = args.line.as_slice() else {
        return "no program".into();
    };
    let error = Command::new(SANDBOX_EXEC)
        .arg("-p")
        .arg(&args.profile)
        .arg(program)
        .args(rest)
        .env_clear()
        .envs(env)
        .exec();
    format!("{SANDBOX_EXEC} did not start: {error}")
}

/// Starts a new session, so this process (and the agent it becomes) has
/// no controlling terminal. A process that already leads its own session
/// (one Ovra started on a pty of its own) keeps it.
fn leave_terminal() -> std::io::Result<()> {
    use rustix::process::{getpid, getsid, setsid};
    match setsid() {
        Ok(_) => Ok(()),
        Err(_) if getsid(None).is_ok_and(|sid| sid == getpid()) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// A random token, as hex.
fn token() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The variables that send every HTTP client through the bridge on `port`,
/// `token` as the proxy's password.
fn proxy_env(port: u16, token: &str) -> Vec<(OsString, OsString)> {
    let proxy = OsString::from(format!("http://{PROXY_USER}:{token}@127.0.0.1:{port}"));
    let mut env: Vec<(OsString, OsString)> =
        ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"]
            .into_iter()
            .map(|name| (name.into(), proxy.clone()))
            .collect();
    env.push(("NODE_USE_ENV_PROXY".into(), "1".into()));
    env
}

/// Starts the bridge as a process of its own and returns its port. It
/// gets no environment and none of the agent's output pipes, and the token
/// on its input, not in its arguments.
fn start_bridge(socket: &OsString, token: &str) -> std::io::Result<u16> {
    let mut child = Command::new(std::env::current_exe()?)
        .env_clear()
        .arg(BRIDGE_FLAG)
        .arg(socket)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut input) = child.stdin.take() {
        writeln!(input, "{token}")?;
    }
    let mut line = String::new();
    if let Some(out) = child.stdout.take() {
        BufReader::new(out).read_line(&mut line)?;
    }
    line.trim()
        .strip_prefix("ready ")
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .ok_or_else(|| std::io::Error::other("the bridge exited"))
}

/// Reads the token from its input, listens on a free loopback port, prints
/// `ready <port>`, and forwards each connection that has the token to the
/// netproxy's socket, until the process that started it (the agent, after
/// `exec`) has ended. Returns only on failure.
fn bridge(args: Vec<OsString>) -> String {
    let [socket] = args.as_slice() else {
        return "needs the netproxy's socket".into();
    };
    let mut token = String::new();
    if std::io::stdin().read_line(&mut token).is_err() || token.trim().is_empty() {
        return "no token".into();
    }
    let expected = authorization(token.trim());
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) {
        Ok(listener) => listener,
        Err(e) => return format!("cannot listen: {e}"),
    };
    let port = match listener.local_addr() {
        Ok(addr) => addr.port(),
        Err(e) => return format!("no port: {e}"),
    };
    let agent = std::os::unix::process::parent_id();
    std::thread::spawn(move || {
        // Once the agent is gone this process belongs to `launchd`.
        while std::os::unix::process::parent_id() == agent {
            std::thread::sleep(WATCH);
        }
        std::process::exit(0);
    });
    println!("ready {port}");
    let _ = std::io::stdout().flush();
    accept(&listener, Path::new(socket), &expected);
    "the listener closed".into()
}

/// The `Proxy-Authorization` value a client with `token` sends: Basic,
/// [`PROXY_USER`] and the token in base64.
fn authorization(token: &str) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let plain = format!("{PROXY_USER}:{token}");
    let mut out = String::from("Basic ");
    for chunk in plain.as_bytes().chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                char::from(DIGITS[(n >> (18 - 6 * i) & 63) as usize])
            } else {
                '='
            });
        }
    }
    out
}

/// Reads `client`'s first request head and checks its `Proxy-Authorization`
/// against `expected`. Returns what to send on: the head without that
/// header, and whatever followed it.
fn admit(client: &mut TcpStream, expected: &str) -> Option<Vec<u8>> {
    client.set_read_timeout(Some(HEAD_WAIT)).ok()?;
    let mut read = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(at) = read.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        if read.len() > MAX_HEAD {
            return None;
        }
        let n = client.read(&mut chunk).ok().filter(|n| *n > 0)?;
        read.extend_from_slice(&chunk[..n]);
    };
    client.set_read_timeout(None).ok()?;
    let head = std::str::from_utf8(&read[..end]).ok()?;
    let mut sent = Vec::with_capacity(read.len());
    let mut admitted = false;
    for line in head.split_inclusive("\r\n") {
        match line.split_once(':') {
            Some((name, value)) if name.eq_ignore_ascii_case("proxy-authorization") => {
                let value = value.trim().as_bytes();
                // As long for any wrong value of the right length.
                admitted |= value.len() == expected.len()
                    && value
                        .iter()
                        .zip(expected.as_bytes())
                        .fold(0u8, |d, (a, b)| d | (a ^ b))
                        == 0;
            }
            _ => sent.extend_from_slice(line.as_bytes()),
        }
    }
    sent.extend_from_slice(&read[end..]);
    admitted.then_some(sent)
}

/// Relays every connection `listener` accepts that [`admit`]s `expected`
/// to `socket`. Returns when the listener fails for good.
fn accept(listener: &TcpListener, socket: &Path, expected: &str) {
    let mut failures = 0u32;
    for client in listener.incoming() {
        let client = match client {
            Ok(client) => client,
            // Out of file descriptors, say: wait a little, up to a second,
            // instead of spinning; give up after many in a row.
            Err(_) if failures < 64 => {
                failures += 1;
                std::thread::sleep(Duration::from_millis(10 << failures.min(6)));
                continue;
            }
            Err(_) => return,
        };
        failures = 0;
        let socket = socket.to_path_buf();
        let expected = expected.to_owned();
        std::thread::spawn(move || {
            let mut client = client;
            let Some(first) = admit(&mut client, &expected) else {
                let _ = client.write_all(REFUSED);
                return;
            };
            if let Ok(mut upstream) = UnixStream::connect(&socket)
                && upstream.write_all(&first).is_ok()
            {
                pipe(client, upstream);
            }
        });
    }
}

/// Copies both ways until either side closes.
fn pipe(client: TcpStream, upstream: UnixStream) {
    let (Ok(mut client_r), Ok(mut upstream_w)) = (client.try_clone(), upstream.try_clone()) else {
        return;
    };
    let (mut client_w, mut upstream_r) = (client, upstream);
    let up = std::thread::spawn(move || {
        let _ = std::io::copy(&mut client_r, &mut upstream_w);
        let _ = upstream_w.shutdown(std::net::Shutdown::Write);
    });
    let _ = std::io::copy(&mut upstream_r, &mut client_w);
    let _ = client_w.shutdown(std::net::Shutdown::Write);
    let _ = up.join();
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;

    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_what_the_plan_writes() {
        let profile = format!("(allow network-outbound (remote ip \"localhost:{PORT_MARK}\"))");
        let parsed = parse(args(&[
            "--bridge",
            "/s",
            "--env",
            "HOME",
            "--profile",
            &profile,
            "--",
            "/bin/claude",
            "-p",
        ]))
        .ok();
        assert_eq!(
            parsed,
            Some(Args {
                bridge: Some("/s".into()),
                env: vec!["HOME".into()],
                profile,
                line: args(&["/bin/claude", "-p"]),
            })
        );
    }

    #[test]
    fn a_bridge_without_its_port_in_the_profile_or_the_reverse_is_refused() {
        let marked = format!("(remote ip \"localhost:{PORT_MARK}\")");
        for list in [
            args(&["--bridge", "/s", "--profile", "(version 1)", "--", "/bin/x"]),
            args(&["--profile", &marked, "--", "/bin/x"]),
            args(&["--profile", "(version 1)", "--"]),
            args(&["--profile", "(version 1)", "/bin/x"]),
            args(&["--", "/bin/x"]),
        ] {
            assert!(parse(list.clone()).is_err(), "{list:?}");
        }
    }

    #[test]
    fn the_proxy_variables_name_the_bridge_with_the_token() {
        let env = proxy_env(4321, "t0k");
        for name in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
            assert!(
                env.iter()
                    .any(|(n, v)| n == name && v == "http://ovra:t0k@127.0.0.1:4321"),
                "{name}"
            );
        }
    }

    #[test]
    fn the_authorization_is_basic_with_the_token() {
        assert_eq!(authorization("abc"), "Basic b3ZyYTphYmM=");
        assert_eq!(authorization("ab"), "Basic b3ZyYTphYg==");
        assert_eq!(authorization("a"), "Basic b3ZyYTph");
        let first = token().ok();
        assert_eq!(first.as_ref().map(String::len), Some(32));
        assert_ne!(first, token().ok());
    }

    /// A bridge for `expected` in front of a fake netproxy: its address,
    /// the fake's listener, and the folder to remove.
    fn relay(name: &str, expected: &str) -> (std::net::SocketAddr, UnixListener, PathBuf) {
        let dir = std::env::temp_dir().join(format!("ovra-mac-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok();
        let socket = dir.join("p.sock");
        let Ok(proxy) = UnixListener::bind(&socket) else {
            panic!("cannot bind {}", socket.display());
        };
        let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) else {
            panic!("cannot listen");
        };
        let Ok(addr) = listener.local_addr() else {
            panic!("no port");
        };
        let expected = expected.to_owned();
        std::thread::spawn(move || accept(&listener, &socket, &expected));
        (addr, proxy, dir)
    }

    #[test]
    fn the_bridge_relays_a_client_with_the_token_both_ways_without_it() {
        let (addr, proxy, dir) = relay("bridge", "Basic b3ZyYTphYmM=");
        let echo = std::thread::spawn(move || {
            let (mut conn, _) = proxy.accept().ok()?;
            let mut buf = [0u8; 44];
            conn.read_exact(&mut buf).ok()?;
            conn.write_all(b"pong!").ok()?;
            Some(buf.to_vec())
        });
        let Ok(mut client) = TcpStream::connect(addr) else {
            panic!("cannot connect");
        };
        client
            .write_all(
                b"CONNECT a:443 HTTP/1.1\r\nHost: a:443\r\n\
                  proxy-authorization:  Basic b3ZyYTphYmM=\r\n\r\nping!",
            )
            .ok();
        let mut back = [0u8; 5];
        client.read_exact(&mut back).ok();
        assert_eq!(&back, b"pong!");
        assert_eq!(
            echo.join().ok().flatten().as_deref(),
            Some(&b"CONNECT a:443 HTTP/1.1\r\nHost: a:443\r\n\r\nping!"[..])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_bridge_refuses_a_client_without_the_token() {
        let (addr, proxy, dir) = relay("refuse", "Basic b3ZyYTphYmM=");
        for head in [
            &b"CONNECT a:443 HTTP/1.1\r\nHost: a:443\r\n\r\n"[..],
            b"CONNECT a:443 HTTP/1.1\r\nProxy-Authorization: Basic b3ZyYTphYmN=\r\n\r\n",
            b"CONNECT a:443 HTTP/1.1\r\nProxy-Authorization: Basic b3ZyYTphYmM\r\n\r\n",
        ] {
            let Ok(mut client) = TcpStream::connect(addr) else {
                panic!("cannot connect");
            };
            client.write_all(head).ok();
            let mut back = Vec::new();
            client.read_to_end(&mut back).ok();
            assert_eq!(back, REFUSED);
        }
        proxy.set_nonblocking(true).ok();
        assert!(proxy.accept().is_err(), "nothing reached the netproxy");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
