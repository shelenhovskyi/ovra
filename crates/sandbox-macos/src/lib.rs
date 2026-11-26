//! The agent sandbox on macOS (`docs/specs/sandbox-macos.md`, task 4.B):
//! Seatbelt, applied by `/usr/bin/sandbox-exec` with a profile written for
//! each session. Contract: `ovra_core::services::Sandbox`; `ovra-sandbox`
//! picks it.
//!
//! [`plan`] decides what a session may read and write from paths alone,
//! and [`sbpl`] writes that as a Seatbelt profile, so both are built and
//! tested on every Unix. The helper (the program [`Setup::helper`] names,
//! started with [`HELPER_FLAG`]) runs outside the sandbox: it starts the
//! netproxy bridge on a local port, puts that port in the profile, and
//! then becomes `sandbox-exec` with `exec`.
//!
//! Until someone has run the checklist in the spec on a Mac, the sandbox
//! reports itself unavailable ([`VERIFIED`]); `OVRA_MACOS_SANDBOX=try`
//! turns it on for that run.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{
    CommandLine, Result, Sandbox, SandboxProfile, SandboxStatus, ServerProfile,
};

#[cfg(unix)]
mod helper;
mod plan;
mod sbpl;

#[cfg(unix)]
pub use helper::helper_if_asked;

/// The first argument that makes a program the sandbox helper.
pub const HELPER_FLAG: &str = "--ovra-sandbox-helper";

/// Where Seatbelt's command-line front end is. Only this one: a
/// `sandbox-exec` found on the person's `PATH` could be anyone's.
pub(crate) const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Whether the checklist in `docs/specs/sandbox-macos.md` has passed on a
/// real Mac. Until then sessions run as they did before 4.B (Claude
/// unsandboxed, ACP agents refused), unless [`TRY_VAR`] says otherwise.
const VERIFIED: bool = false;

/// Set to `try` to use the sandbox before it is verified: for running the
/// checklist on a Mac.
const TRY_VAR: &str = "OVRA_MACOS_SANDBOX";

/// What the sandbox needs to know about the machine, apart from the
/// session's profile.
#[derive(Debug, Clone)]
pub struct Setup {
    /// A program that runs [`helper_if_asked`] first thing: the daemon or
    /// the app itself.
    pub helper: PathBuf,
    /// The person's home folder: unreadable, except what the plan gives
    /// back.
    pub home: PathBuf,
    /// `PATH` as Ovra sees it.
    pub path: OsString,
    /// Toolchain homes that stay readable (`CARGO_HOME`, `RUSTUP_HOME`,
    /// `NVM_DIR`, or their defaults).
    pub toolchains: Vec<PathBuf>,
    /// Ovra's data folder: unreadable, except the session's own work dir
    /// and the folders below.
    pub data_dir: PathBuf,
    /// Where sessions keep their package caches, one folder per project.
    pub caches: PathBuf,
    /// Where sessions keep their temporary files, one folder per project.
    pub temp: PathBuf,
    /// More folders nobody in the sandbox reads: the temporary folder,
    /// which holds every session's netproxy socket.
    pub hidden: Vec<PathBuf>,
    /// The protected paths under `work_dir` every platform shares
    /// (`ovra-sandbox`); `**/name` is any depth.
    pub protected: &'static [&'static str],
}

impl Setup {
    /// The setup for this process: its own executable as the helper, the
    /// home folder and toolchains from the environment, and folders in
    /// `data_dir`.
    pub fn from_env(data_dir: &Path, protected: &'static [&'static str]) -> Option<Self> {
        let home = std::env::home_dir().filter(|h| h.is_absolute() && h.parent().is_some())?;
        let var = |name: &str, default: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(default))
        };
        let data_dir = std::path::absolute(data_dir).ok()?;
        Some(Self {
            helper: std::env::current_exe().ok()?,
            path: std::env::var_os("PATH").unwrap_or_default(),
            toolchains: vec![
                var("CARGO_HOME", ".cargo"),
                var("RUSTUP_HOME", ".rustup"),
                var("NVM_DIR", ".nvm"),
            ],
            caches: data_dir.join("sandbox-caches"),
            temp: data_dir.join("sandbox-temp"),
            hidden: vec![std::env::temp_dir()],
            data_dir,
            home,
            protected,
        })
    }
}

/// What the macOS sandbox does not cover, as sessions report it
/// (`docs/specs/sandbox-macos.md`, "Known gaps"). Keep it in step with
/// that list.
const GAPS: [&str; 6] = [
    "keychain items command-line tools may read",
    "whether a file exists, and its size, anywhere on the Mac",
    "the session's proxy port, for other users of this Mac",
    "processes an agent leaves running after its session",
    "git config values that name project files",
    "the session's own folders in the agent's settings (todos, logs, ...)",
];

/// The macOS sandbox: Seatbelt through `sandbox-exec`.
#[derive(Debug)]
pub struct Seatbelt {
    setup: Setup,
    status: SandboxStatus,
}

impl Seatbelt {
    /// Whether this Mac can sandbox agents: `sandbox-exec` is there and a
    /// probe through it holds. Unavailable until [`VERIFIED`] (or
    /// [`TRY_VAR`]).
    pub fn detect(setup: Setup) -> Self {
        let tried = std::env::var_os(TRY_VAR).is_some_and(|v| v == "try");
        let status = if !VERIFIED && !tried {
            SandboxStatus::Unavailable {
                reason: "the macOS sandbox has not been tested on a Mac yet".into(),
            }
        } else {
            match probe(&setup) {
                Ok(()) => SandboxStatus::Available {
                    name: "Seatbelt".into(),
                    gaps: GAPS.map(String::from).to_vec(),
                },
                Err(reason) => SandboxStatus::Unavailable { reason },
            }
        };
        Self { setup, status }
    }
}

impl Sandbox for Seatbelt {
    fn status(&self) -> SandboxStatus {
        self.status.clone()
    }

    fn wrap(&self, profile: &SandboxProfile, line: CommandLine) -> Result<CommandLine> {
        if matches!(self.status, SandboxStatus::Unavailable { .. }) {
            return Ok(line);
        }
        let plan = plan::Plan::agent(&self.setup, profile, &line)?;
        let outer: Vec<OsString> = std::env::vars_os().map(|(name, _)| name).collect();
        plan.line(&self.setup, line, &outer)
    }

    fn wrap_server(&self, profile: &ServerProfile, line: CommandLine) -> Result<CommandLine> {
        if matches!(self.status, SandboxStatus::Unavailable { .. }) {
            return Ok(line);
        }
        let plan = plan::Plan::server(&self.setup, profile, &line)?;
        let outer: Vec<OsString> = std::env::vars_os().map(|(name, _)| name).collect();
        plan.line(&self.setup, line, &outer)
    }
}

/// Runs `sandbox-exec` once with the profile every session starts from:
/// `/usr/bin/true` must run, and writing a file outside what it allows
/// must fail.
#[cfg(target_os = "macos")]
fn probe(setup: &Setup) -> std::result::Result<(), String> {
    use std::process::{Command, Stdio};
    if !Path::new(SANDBOX_EXEC).is_file() {
        return Err(format!("{SANDBOX_EXEC} is missing"));
    }
    let profile = sbpl::probe();
    let run = |program: &str, args: &[&OsString]| {
        Command::new(SANDBOX_EXEC)
            .arg("-p")
            .arg(&profile)
            .arg(program)
            .args(args)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("{SANDBOX_EXEC} did not start: {e}"))
    };
    let ran = run("/usr/bin/true", &[])?;
    if !ran.status.success() {
        let said = String::from_utf8_lossy(&ran.stderr);
        return Err(format!(
            "{SANDBOX_EXEC} refused Ovra's profile: {}",
            said.trim()
        ));
    }
    let _ = std::fs::create_dir_all(&setup.data_dir);
    let target = setup
        .data_dir
        .join(format!("sandbox-probe-{}", std::process::id()));
    let target = OsString::from(&target);
    let wrote = run("/usr/bin/touch", &[&target])?;
    let made = Path::new(&target).exists();
    let _ = std::fs::remove_file(&target);
    if wrote.status.success() || made {
        return Err("Seatbelt did not stop a write outside the sandbox".into());
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn probe(_setup: &Setup) -> std::result::Result<(), String> {
    Err("Seatbelt exists only on macOS".into())
}

fn invalid(what: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, what.to_string())
}

#[cfg(test)]
mod tests {
    use ovra_core::services::{ProfileKind, ProfileNetwork};

    use super::*;

    #[test]
    fn until_it_is_verified_it_says_so_and_changes_nothing() {
        let setup = Setup {
            helper: "ovra".into(),
            home: "/home/p".into(),
            path: OsString::new(),
            toolchains: Vec::new(),
            data_dir: "/data".into(),
            caches: "/data/caches".into(),
            temp: "/data/temp".into(),
            hidden: Vec::new(),
            protected: &[],
        };
        let sandbox = Seatbelt::detect(setup);
        // Outside macOS it never is; on a Mac not before it is verified.
        assert!(matches!(
            sandbox.status(),
            SandboxStatus::Unavailable { .. }
        ));
        let line = CommandLine {
            program: "claude".into(),
            args: vec!["-p".into()],
            env: Vec::new(),
            cwd: "/w".into(),
        };
        let profile = SandboxProfile {
            kind: ProfileKind::Agent,
            agent_state: None,
            project: None,
            work_env: None,
            work_dir: "/w".into(),
            work_dir_read_only: false,
            read: Vec::new(),
            write: Vec::new(),
            expose: Vec::new(),
            forward: Vec::new(),
            network: ProfileNetwork::Blocked,
        };
        assert_eq!(sandbox.wrap(&profile, line.clone()).ok(), Some(line));
    }
}
