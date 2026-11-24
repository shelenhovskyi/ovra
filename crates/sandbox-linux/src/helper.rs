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
use std::path::PathBuf;
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
            Some("--bridge") => {
                let socket = value(&mut args, "--bridge")?;
                parsed.bridge = Some((socket, value(&mut args, "--bridge")?));
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
    Ok(parsed)
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
    if let Err(e) = crate::seccomp::apply() {
        return format!("seccomp failed: {e}");
    }
    // After both: the bridge is held as tightly as the agent, which could
    // otherwise trace it and act through it.
    if let Some((socket, port)) = &args.bridge
        && let Err(e) = start_bridge(socket, port)
    {
        return format!("the network bridge did not start: {e}");
    }
    let mut command = args.command.into_iter();
    let program = command.next().unwrap_or_default();
    let error = Command::new(&program).args(command).exec();
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
/// and waits until it listens.
fn start_bridge(socket: &OsString, port: &OsString) -> std::io::Result<()> {
    let mut child = Command::new(std::env::current_exe()?)
        .arg(BRIDGE_FLAG)
        .arg(socket)
        .arg(port)
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

/// Forwards each connection to `127.0.0.1:<port>` to the netproxy's Unix
/// socket. Returns only on failure.
fn bridge(args: Vec<OsString>) -> String {
    let [socket, port] = args.as_slice() else {
        return "needs a socket and a port".into();
    };
    let Some(port) = port.to_str().and_then(|p| p.parse::<u16>().ok()) else {
        return "bad port".into();
    };
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
        Ok(listener) => listener,
        Err(e) => return e.to_string(),
    };
    println!("ready");
    let _ = std::io::stdout().flush();
    let socket = PathBuf::from(socket);
    for client in listener.incoming() {
        let Ok(client) = client else { continue };
        let socket = socket.clone();
        std::thread::spawn(move || {
            if let Ok(upstream) = UnixStream::connect(&socket) {
                pipe(client, upstream);
            }
        });
    }
    "the listener closed".into()
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
            "--",
            "claude",
            "-p",
        ]))
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(parsed.landlock, Some(Landlock::Require));
        assert_eq!(parsed.writable, vec![PathBuf::from("/w")]);
        assert_eq!(parsed.bridge, Some(("/s".into(), "3128".into())));
        assert_eq!(parsed.command, args(&["claude", "-p"]));
    }

    #[test]
    fn refuses_what_it_does_not_know() {
        assert!(parse(args(&["--net", "open", "--", "x"])).is_err());
        assert!(parse(args(&["--landlock", "maybe", "--", "x"])).is_err());
        assert!(parse(args(&["--rw", "/w"])).is_err());
    }
}
