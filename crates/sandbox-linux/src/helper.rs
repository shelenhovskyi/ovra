//! The helper: the first program inside bubblewrap. It applies what
//! bubblewrap cannot (Landlock, the seccomp filter), starts the netproxy
//! bridge, and then becomes the agent with `exec`.
//!
//! The daemon and the app are the helper themselves: they call
//! [`helper_if_asked`] before anything else, so no second binary has to be
//! installed next to them.

use std::ffi::OsString;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use landlock::{
    ABI, Access as _, AccessFs, CompatLevel, Compatible as _, Ruleset, RulesetAttr as _,
    RulesetCreatedAttr as _, RulesetStatus, Scope, path_beneath_rules,
};

use crate::HELPER_FLAG;

/// What the helper prints in [`Landlock::Report`] mode when Landlock holds.
pub(crate) const LANDLOCK_HELD: &str = "ovra-sandbox: landlock enforced";

/// Runs the netproxy bridge instead of an agent.
const BRIDGE_FLAG: &str = "--ovra-sandbox-bridge";

/// Starts bubblewrap from outside the sandbox and removes folders made for
/// the session once it is over: `--ovra-sandbox-outer <folder>... --
/// <bwrap> <args>`.
pub(crate) const OUTER_FLAG: &str = "--ovra-sandbox-outer";

/// Waits for the session to end, then removes the folders it names.
const CLEANER_FLAG: &str = "--ovra-sandbox-cleaner";

/// The newest Landlock version the rules are written for.
const ABI_WANTED: ABI = ABI::V6;

/// How much Landlock the helper insists on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Landlock {
    /// Refuse to start the agent unless Landlock holds.
    Require,
    /// Use it where the kernel has it.
    Try,
    /// As [`Landlock::Try`], and print [`LANDLOCK_HELD`] if it holds (the
    /// probe only: the agent's output must stay its own).
    Report,
}

impl Landlock {
    pub(crate) fn as_arg(self) -> &'static str {
        match self {
            Self::Require => "require",
            Self::Try => "try",
            Self::Report => "report",
        }
    }
}

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
        Some(flag) if flag == OUTER_FLAG => {
            let error = outer(args.collect());
            eprintln!("ovra-sandbox: {error}");
            std::process::exit(126);
        }
        Some(flag) if flag == CLEANER_FLAG => {
            let Some(record) = args.next() else {
                std::process::exit(2);
            };
            let Ok(held) = crate::made::hold(Path::new(&record)) else {
                std::process::exit(1);
            };
            println!("held");
            // End of file: the session and everything in it are gone.
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
            let _ = crate::made::clean(held);
            std::process::exit(0);
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
#[derive(Debug, Default)]
struct Args {
    landlock: Option<Landlock>,
    writable: Vec<PathBuf>,
    bridge: Option<(OsString, OsString)>,
    /// Ports to listen on and forward through the bridge's socket.
    forward: Vec<u16>,
    /// The only variables the agent gets.
    env: Vec<OsString>,
    /// No `socket(AF_UNIX)` (a language server's sandbox).
    no_unix: bool,
    command: Vec<OsString>,
}

fn parse(args: Vec<OsString>) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = args.into_iter();
    let value = |args: &mut std::vec::IntoIter<OsString>, flag: &str| {
        args.next().ok_or_else(|| format!("{flag} needs a value"))
    };
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--landlock") => {
                parsed.landlock = match value(&mut args, "--landlock")?.to_str() {
                    Some("require") => Some(Landlock::Require),
                    Some("try") => Some(Landlock::Try),
                    Some("report") => Some(Landlock::Report),
                    _ => return Err("unknown --landlock mode".into()),
                };
            }
            Some("--rw") => parsed.writable.push(value(&mut args, "--rw")?.into()),
            Some("--env") => parsed.env.push(value(&mut args, "--env")?),
            Some("--no-unix-sockets") => parsed.no_unix = true,
            Some("--bridge") => {
                let socket = value(&mut args, "--bridge")?;
                parsed.bridge = Some((socket, value(&mut args, "--bridge")?));
            }
            Some("--forward") => {
                let port = value(&mut args, "--forward")?;
                parsed
                    .forward
                    .push(port_of(&port).ok_or("bad --forward port")?);
            }
            Some("--") => {
                parsed.command = args.collect();
                break;
            }
            _ => return Err(format!("unknown argument {}", arg.to_string_lossy())),
        }
    }
    if parsed.command.is_empty() {
        return Err("no command".into());
    }
    if !parsed.forward.is_empty() && parsed.bridge.is_none() {
        return Err("--forward needs --bridge".into());
    }
    // The bridge reaches the netproxy over a Unix socket.
    if parsed.no_unix && parsed.bridge.is_some() {
        return Err("--no-unix-sockets leaves no way to the bridge".into());
    }
    Ok(parsed)
}

/// Starts a cleaner that holds the work dir's record (`made.rs`) until the
/// last holder of a pipe is gone, then becomes bubblewrap, which hands the
/// pipe to every process in the sandbox. A session that is killed still
/// closes it. Returns only if something failed: bubblewrap does not start
/// unless the cleaner holds the record.
fn outer(args: Vec<OsString>) -> String {
    let [record, dashes, program, rest @ ..] = args.as_slice() else {
        return "no command".into();
    };
    if dashes != "--" {
        return "no command".into();
    }
    // Close-on-exec while the cleaner starts, so it does not hold the pipe
    // open itself.
    let (read, write) = match rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC) {
        Ok(pipe) => pipe,
        Err(e) => return format!("no pipe: {e}"),
    };
    let cleaner = std::env::current_exe().and_then(|exe| {
        Command::new(exe)
            .arg(CLEANER_FLAG)
            .arg(record)
            .stdin(Stdio::from(read))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            // Not stopped with the session's process group.
            .process_group(0)
            .spawn()
    });
    let mut cleaner = match cleaner {
        Ok(cleaner) => cleaner,
        Err(e) => return format!("the cleaner did not start: {e}"),
    };
    // One line once it holds the record; nothing if it could not.
    let mut held = String::new();
    if let Some(out) = cleaner.stdout.take() {
        let _ = BufReader::new(out).read_line(&mut held);
    }
    if held != "held\n" {
        return format!("the cleaner could not hold {}", Path::new(record).display());
    }
    if let Err(e) = rustix::io::fcntl_setfd(&write, rustix::io::FdFlags::empty()) {
        return format!("cannot pass the pipe on: {e}");
    }
    let error = Command::new(program).args(rest).exec();
    drop(write);
    format!("could not run {}: {error}", program.to_string_lossy())
}

/// Returns only if something failed; the agent never starts then.
fn run(args: Vec<OsString>) -> String {
    let args = match parse(args) {
        Ok(args) => args,
        Err(e) => return e,
    };
    let held = match landlock(&args.writable) {
        Ok(held) => held,
        Err(e) => return format!("Landlock failed: {e}"),
    };
    match args.landlock {
        Some(Landlock::Require) if !held => return "Landlock did not hold".into(),
        Some(Landlock::Report) if held => println!("{LANDLOCK_HELD}"),
        _ => {}
    }
    if let Err(e) = crate::seccomp::apply(args.no_unix) {
        return format!("seccomp failed: {e}");
    }
    // After both: the bridge is held as tightly as the agent, which could
    // otherwise trace it and act through it.
    if let Some((socket, port)) = &args.bridge
        && let Err(e) = start_bridge(socket, port, &args.forward)
    {
        return format!("the network bridge did not start: {e}");
    }
    let mut command = args.command.into_iter();
    let program = command.next().unwrap_or_default();
    let error = Command::new(&program)
        .args(command)
        .env_clear()
        .envs(
            args.env
                .iter()
                .filter_map(|name| std::env::var_os(name).map(|value| (name, value))),
        )
        .exec();
    format!("could not run {}: {error}", program.to_string_lossy())
}

/// Everything readable, only `writable` writable. Abstract Unix sockets and
/// signals outside the sandbox are out of reach where the kernel can scope
/// them (Landlock 6). Returns whether the kernel enforced any of it.
fn landlock(writable: &[PathBuf]) -> Result<bool, landlock::RulesetError> {
    let status = Ruleset::default()
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(AccessFs::from_all(ABI_WANTED))?
        .scope(Scope::from_all(ABI_WANTED))?
        .create()?
        .add_rules(path_beneath_rules(["/"], AccessFs::from_read(ABI_WANTED)))?
        .add_rules(path_beneath_rules(writable, AccessFs::from_all(ABI_WANTED)))?
        .restrict_self()?;
    Ok(status.ruleset != RulesetStatus::NotEnforced)
}

/// Starts the bridge as a process of its own (this one becomes the agent)
/// and waits until it listens on `port` and every port of `forward`.
fn start_bridge(socket: &OsString, port: &OsString, forward: &[u16]) -> std::io::Result<()> {
    // The agent can read the bridge's environment: it gets none.
    let mut child = Command::new(std::env::current_exe()?)
        .env_clear()
        .arg(BRIDGE_FLAG)
        .arg(socket)
        .arg(port)
        .args(forward.iter().map(u16::to_string))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut line = String::new();
    if let Some(out) = child.stdout.take() {
        BufReader::new(out).read_line(&mut line)?;
    }
    if line.trim() != "ready" {
        return Err(std::io::Error::other("the bridge exited"));
    }
    Ok(())
}

/// A port number a listener can use: 1 to 65535.
fn port_of(arg: &OsString) -> Option<u16> {
    arg.to_str()
        .and_then(|p| p.parse::<u16>().ok())
        .filter(|p| *p != 0)
}

/// Forwards each connection to `127.0.0.1:<port>` to the netproxy's Unix
/// socket as it is, and each connection to `127.0.0.1:<P>` for every
/// further port `P` (a managed server's, `docs/specs/servers-and-browser.md`)
/// after the line `OVRA-LOCAL <P>`. Returns only on failure.
fn bridge(args: Vec<OsString>) -> String {
    let [socket, ports @ ..] = args.as_slice() else {
        return "needs a socket and a port".into();
    };
    if ports.is_empty() {
        return "needs a socket and a port".into();
    }
    let mut listeners = Vec::new();
    for (n, port) in ports.iter().enumerate() {
        let Some(port) = port_of(port) else {
            return "bad port".into();
        };
        match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
            // The first port is the proxy's own: no preamble.
            Ok(listener) => listeners.push((listener, port, (n > 0).then_some(port))),
            Err(e) => return format!("port {port}: {e}"),
        }
    }
    println!("ready");
    let _ = std::io::stdout().flush();
    let socket = PathBuf::from(socket);
    let (ended, first) = std::sync::mpsc::channel();
    for (listener, port, local) in listeners {
        let socket = socket.clone();
        let ended = Ended(ended.clone(), port);
        std::thread::spawn(move || {
            let _ended = ended;
            accept(&listener, &socket, local);
        });
    }
    // Each runs until its listener fails; the bridge stops with the first,
    // so no port is lost without the sandbox noticing.
    match first.recv() {
        Ok(port) => format!("the listener on port {port} closed"),
        Err(_) => "the listeners closed".into(),
    }
}

/// Tells the bridge that a listener's thread ended, however it ended.
#[derive(Debug)]
struct Ended(std::sync::mpsc::Sender<u16>, u16);

impl Drop for Ended {
    fn drop(&mut self) {
        let _ = self.0.send(self.1);
    }
}

/// Relays every connection `listener` accepts to `socket`, opening each
/// with `OVRA-LOCAL <port>` when `local` is a server's port. Returns when
/// the listener fails for good.
fn accept(listener: &TcpListener, socket: &Path, local: Option<u16>) {
    let mut wait = Backoff::default();
    for client in listener.incoming() {
        let client = match client {
            Ok(client) => client,
            Err(error) => match wait.after(&error) {
                Some(pause) => {
                    std::thread::sleep(pause);
                    continue;
                }
                None => return,
            },
        };
        wait = Backoff::default();
        let socket = socket.to_path_buf();
        std::thread::spawn(move || {
            let _ = relay(client, &socket, local);
        });
    }
}

/// How long to wait before accepting again after an error, so that one
/// that repeats (`EMFILE` while every file descriptor is taken) does not
/// spin.
#[derive(Debug, Default)]
struct Backoff(u32);

impl Backoff {
    const FIRST: std::time::Duration = std::time::Duration::from_millis(10);
    const MAX: std::time::Duration = std::time::Duration::from_secs(1);

    /// The pause after `error`, twice the last one up to [`Self::MAX`];
    /// `None` when the listener cannot recover from it.
    fn after(&mut self, error: &std::io::Error) -> Option<std::time::Duration> {
        let passing = [libc::EMFILE, libc::ENFILE, libc::ENOBUFS, libc::ENOMEM];
        let recovers = error.raw_os_error().is_some_and(|e| passing.contains(&e))
            || matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionAborted | std::io::ErrorKind::Interrupted
            );
        if !recovers {
            return None;
        }
        let pause = Self::FIRST
            .saturating_mul(1 << self.0.min(7))
            .min(Self::MAX);
        self.0 += 1;
        Some(pause)
    }
}

/// Connects to `socket`, writes the preamble for `local` if any, then
/// copies both ways.
fn relay<C: Duplex>(client: C, socket: &Path, local: Option<u16>) -> std::io::Result<()> {
    let mut upstream = UnixStream::connect(socket)?;
    if let Some(port) = local {
        upstream.write_all(format!("OVRA-LOCAL {port}\r\n").as_bytes())?;
    }
    pipe(client, upstream);
    Ok(())
}

/// A connection the bridge accepts: TCP inside the sandbox, a Unix
/// stream in tests.
trait Duplex: std::io::Read + std::io::Write + Send + Sized + 'static {
    fn split(&self) -> std::io::Result<Self>;
    fn close_write(&self);
}

impl Duplex for TcpStream {
    fn split(&self) -> std::io::Result<Self> {
        self.try_clone()
    }
    fn close_write(&self) {
        let _ = self.shutdown(std::net::Shutdown::Write);
    }
}

impl Duplex for UnixStream {
    fn split(&self) -> std::io::Result<Self> {
        self.try_clone()
    }
    fn close_write(&self) {
        let _ = self.shutdown(std::net::Shutdown::Write);
    }
}

/// Copies both ways until either side closes.
fn pipe<C: Duplex>(client: C, upstream: UnixStream) {
    let (Ok(mut client_r), Ok(mut upstream_w)) = (client.split(), upstream.try_clone()) else {
        return;
    };
    let (mut client_w, mut upstream_r) = (client, upstream);
    let up = std::thread::spawn(move || {
        let _ = std::io::copy(&mut client_r, &mut upstream_w);
        let _ = upstream_w.shutdown(std::net::Shutdown::Write);
    });
    let _ = std::io::copy(&mut upstream_r, &mut client_w);
    client_w.close_write();
    let _ = up.join();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_what_the_plan_writes() {
        let parsed = parse(args(&[
            "--landlock",
            "require",
            "--rw",
            "/w",
            "--bridge",
            "/s",
            "3128",
            "--env",
            "HOME",
            "--",
            "claude",
            "-p",
        ]))
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(parsed.landlock, Some(Landlock::Require));
        assert_eq!(parsed.writable, vec![PathBuf::from("/w")]);
        assert_eq!(parsed.bridge, Some(("/s".into(), "3128".into())));
        assert_eq!(parsed.env, args(&["HOME"]));
        assert_eq!(parsed.command, args(&["claude", "-p"]));
    }

    #[test]
    fn refuses_what_it_does_not_know() {
        assert!(parse(args(&["--net", "open", "--", "x"])).is_err());
        assert!(parse(args(&["--landlock", "maybe", "--", "x"])).is_err());
        assert!(parse(args(&["--rw", "/w"])).is_err());
    }

    #[test]
    fn a_line_without_unix_sockets_has_no_bridge() {
        let parsed =
            parse(args(&["--no-unix-sockets", "--", "x"])).unwrap_or_else(|e| panic!("{e}"));
        assert!(parsed.no_unix);
        assert!(
            !parse(args(&["--", "x"]))
                .unwrap_or_else(|e| panic!("{e}"))
                .no_unix
        );
        let both = parse(args(&[
            "--no-unix-sockets",
            "--bridge",
            "/s",
            "3128",
            "--",
            "x",
        ]));
        assert!(both.is_err());
    }

    #[test]
    fn forwards_ports_only_through_the_bridge() {
        let parsed = parse(args(&[
            "--bridge",
            "/s",
            "3128",
            "--forward",
            "5173",
            "--forward",
            "8080",
            "--",
            "x",
        ]))
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(parsed.forward, vec![5173, 8080]);
        let refused = |list: &[&str]| parse(args(list)).err().unwrap_or_default();
        assert_eq!(
            refused(&["--forward", "5173", "--", "x"]),
            "--forward needs --bridge"
        );
        for bad in ["0", "65536", "http", ""] {
            assert_eq!(
                refused(&["--bridge", "/s", "3128", "--forward", bad, "--", "x"]),
                "bad --forward port"
            );
        }
        assert!(refused(&["--bridge", "/s", "3128", "--forward"]).contains("needs a value"));
    }

    #[test]
    fn accepting_backs_off_on_errors_that_pass_and_stops_on_others() {
        use std::io::{Error, ErrorKind};
        let mut wait = Backoff::default();
        let full = Error::from_raw_os_error(libc::EMFILE);
        let pauses: Vec<u128> = (0..9)
            .filter_map(|_| wait.after(&full))
            .map(|p| p.as_millis())
            .collect();
        assert_eq!(pauses, [10, 20, 40, 80, 160, 320, 640, 1000, 1000]);
        assert!(wait.after(&ErrorKind::ConnectionAborted.into()).is_some());
        assert_eq!(wait.after(&Error::from_raw_os_error(libc::EBADF)), None);
        assert_eq!(wait.after(&Error::from_raw_os_error(libc::EINVAL)), None);
    }

    #[test]
    fn a_listener_that_ends_ends_the_bridge() {
        let (ended, first) = std::sync::mpsc::channel();
        let quiet = Ended(ended.clone(), 3128);
        let failing = Ended(ended, 5173);
        let thread = std::thread::spawn(move || {
            let _ended = failing;
            panic!("the listener failed");
        });
        assert!(thread.join().is_err());
        assert_eq!(first.recv().ok(), Some(5173));
        drop(quiet);
    }

    #[test]
    fn a_forwarded_connection_opens_with_its_port_then_relays_both_ways() {
        use std::io::Read as _;
        use std::os::unix::net::UnixListener;
        let dir = std::env::temp_dir().join(format!("ovra-forward-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        let socket = dir.join("proxy.sock");
        let proxy = UnixListener::bind(&socket).unwrap_or_else(|e| panic!("{e}"));
        let (mut agent, accepted) = UnixStream::pair().unwrap_or_else(|e| panic!("{e}"));
        let relaying = {
            let socket = socket.clone();
            std::thread::spawn(move || relay(accepted, &socket, Some(5173)))
        };
        let (mut upstream, _) = proxy.accept().unwrap_or_else(|e| panic!("{e}"));
        agent
            .write_all(b"GET / HTTP/1.1\r\n\r\n")
            .unwrap_or_else(|e| panic!("{e}"));
        agent
            .shutdown(std::net::Shutdown::Write)
            .unwrap_or_else(|e| panic!("{e}"));
        let mut seen = String::new();
        upstream
            .read_to_string(&mut seen)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(seen, "OVRA-LOCAL 5173\r\nGET / HTTP/1.1\r\n\r\n");
        upstream
            .write_all(b"HTTP/1.1 200 OK\r\n\r\n")
            .unwrap_or_else(|e| panic!("{e}"));
        drop(upstream);
        let mut answer = String::new();
        agent
            .read_to_string(&mut answer)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(answer, "HTTP/1.1 200 OK\r\n\r\n");
        assert!(relaying.join().is_ok_and(|r| r.is_ok()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_proxy_port_itself_gets_no_preamble() {
        use std::io::Read as _;
        use std::os::unix::net::UnixListener;
        let dir = std::env::temp_dir().join(format!("ovra-bridge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        let socket = dir.join("proxy.sock");
        let proxy = UnixListener::bind(&socket).unwrap_or_else(|e| panic!("{e}"));
        let (mut agent, accepted) = UnixStream::pair().unwrap_or_else(|e| panic!("{e}"));
        let relaying = std::thread::spawn(move || relay(accepted, &socket, None));
        let (mut upstream, _) = proxy.accept().unwrap_or_else(|e| panic!("{e}"));
        agent
            .write_all(b"CONNECT")
            .unwrap_or_else(|e| panic!("{e}"));
        drop(agent);
        let mut seen = String::new();
        upstream
            .read_to_string(&mut seen)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(seen, "CONNECT");
        drop(upstream);
        assert!(relaying.join().is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
