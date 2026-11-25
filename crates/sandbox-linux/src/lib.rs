//! The agent sandbox on Linux (`docs/specs/security.md`, task 4.A).
//!
//! [`Bubblewrap::wrap`] turns a session's command line into a `bwrap` line:
//! a new user, PID, IPC and (unless the network is open) network
//! namespace, the system read-only, the home folder and `/run` hidden, and
//! only what [`plan`] lists bound back. Inside it, the helper (the program
//! [`Setup::helper`] names, started with [`HELPER_FLAG`]) applies Landlock
//! and a seccomp filter, bridges the netproxy socket to a local port, and
//! then runs `claude`. Contract: `ovra_core::services::Sandbox`;
//! `ovra-sandbox` picks it.

#![cfg(target_os = "linux")]

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{CommandLine, Result, Sandbox, SandboxProfile, SandboxStatus};

mod helper;
mod plan;
mod seccomp;

use helper::Landlock;
pub use helper::helper_if_asked;
pub use plan::Protected;

/// The first argument that makes a program the sandbox helper (see
/// [`helper_if_asked`]).
pub const HELPER_FLAG: &str = "--ovra-sandbox-helper";

/// Where bubblewrap may be. Only system folders: a `bwrap` found through
/// the user's `PATH` could be anyone's.
const BWRAP: [&str; 4] = [
    "/usr/bin/bwrap",
    "/bin/bwrap",
    "/usr/local/bin/bwrap",
    "/run/current-system/sw/bin/bwrap",
];

/// What the sandbox needs to know about the machine, apart from the
/// session's profile.
#[derive(Debug, Clone)]
pub struct Setup {
    /// A program that runs [`helper_if_asked`] first thing: the daemon or
    /// the app itself.
    pub helper: PathBuf,
    /// The user's home folder: hidden, except what [`plan`] binds back.
    pub home: PathBuf,
    /// `PATH` as Ovra sees it; the sandbox gets it without entries that
    /// would expose the home folder.
    pub path: OsString,
    /// Toolchain homes that stay readable (`CARGO_HOME`, `RUSTUP_HOME`,
    /// `NVM_DIR`, or their defaults).
    pub toolchains: Vec<PathBuf>,
    /// Folders hidden even outside the home folder: Ovra's data folder.
    pub hidden: Vec<PathBuf>,
    /// Where sessions keep their package caches, one folder per project, so
    /// what an agent puts in a cache never reaches the user's own builds.
    pub caches: PathBuf,
    /// The protected paths every platform shares (`ovra-sandbox`).
    pub protected: Protected,
}

impl Setup {
    /// The setup for this process: its own executable as the helper, the
    /// home folder and toolchains from the environment, and `data_dir`
    /// hidden.
    pub fn from_env(data_dir: &Path, protected: Protected) -> Option<Self> {
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
            caches: data_dir.join("sandbox-caches"),
            path: std::env::var_os("PATH").unwrap_or_default(),
            toolchains: vec![
                var("CARGO_HOME", ".cargo"),
                var("RUSTUP_HOME", ".rustup"),
                var("NVM_DIR", ".nvm"),
            ],
            // The temporary folder holds the netproxy's sockets of every
            // session; `/tmp` is hidden anyway.
            hidden: vec![data_dir, std::env::temp_dir()],
            home,
            protected,
        })
    }
}

/// The Linux sandbox: bubblewrap, plus Landlock and seccomp in the helper.
#[derive(Debug)]
pub struct Bubblewrap {
    setup: Setup,
    bwrap: PathBuf,
    status: SandboxStatus,
    /// Whether the probe found Landlock enforced; then every session
    /// requires it.
    landlock: bool,
}

impl Bubblewrap {
    /// Finds bubblewrap and tries the whole sandbox once on a harmless
    /// command, so that [`Sandbox::status`] says what will really happen.
    pub fn detect(setup: Setup) -> Self {
        let Some(bwrap) = BWRAP.iter().map(PathBuf::from).find(|p| p.is_file()) else {
            return Self::unavailable(setup, "install bubblewrap (bwrap) to sandbox agents");
        };
        let mut sandbox = Self {
            setup,
            bwrap,
            status: SandboxStatus::Unavailable {
                reason: String::new(),
            },
            landlock: false,
        };
        match sandbox.probe() {
            Ok(landlock) => {
                sandbox.landlock = landlock;
                let name = if landlock {
                    "bubblewrap, Landlock, seccomp"
                } else {
                    "bubblewrap, seccomp"
                };
                sandbox.status = SandboxStatus::Available { name: name.into() };
            }
            Err(reason) => sandbox.status = SandboxStatus::Unavailable { reason },
        }
        sandbox
    }

    fn unavailable(setup: Setup, reason: &str) -> Self {
        Self {
            setup,
            bwrap: PathBuf::new(),
            status: SandboxStatus::Unavailable {
                reason: reason.into(),
            },
            landlock: false,
        }
    }

    /// Runs `true` in an empty sandbox with a home folder of its own, so
    /// the user's is not touched; the helper prints whether Landlock held.
    /// Returns that, or why the sandbox does not work here.
    fn probe(&self) -> std::result::Result<bool, String> {
        static PROBES: AtomicU64 = AtomicU64::new(0);
        let n = PROBES.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let empty = std::env::temp_dir().join(format!(
            "ovra-sandbox-probe-{}-{n}-{nanos}",
            std::process::id()
        ));
        // A new folder of its own: `create_dir` fails if someone made it first.
        let (home, work) = (empty.join("home"), empty.join("work"));
        std::fs::create_dir(&empty)
            .and_then(|()| std::fs::create_dir(&home))
            .and_then(|()| std::fs::create_dir(&work))
            .map_err(|e| format!("no temporary folder: {e}"))?;
        let setup = Setup {
            home,
            path: "/usr/bin:/bin".into(),
            toolchains: Vec::new(),
            hidden: Vec::new(),
            ..self.setup.clone()
        };
        let profile = SandboxProfile {
            work_dir: work.clone(),
            read: Vec::new(),
            write: Vec::new(),
            network: ovra_core::services::ProfileNetwork::Blocked,
        };
        let line = CommandLine {
            program: "/bin/true".into(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: work,
        };
        let out = plan::Plan::new(&setup, &profile)
            .and_then(|plan| {
                plan.line(
                    &self.bwrap,
                    &setup.helper,
                    Landlock::Report,
                    line,
                    &outer_env(),
                )
            })
            .map_err(|e| e.message)
            .and_then(|line| {
                Command::new(&line.program)
                    .args(&line.args)
                    .current_dir(&line.cwd)
                    .stdin(Stdio::null())
                    .output()
                    .map_err(|e| format!("bubblewrap did not start: {e}"))
            });
        let _ = std::fs::remove_dir_all(&empty);
        let out = out?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("bubblewrap does not work here: {}", err.trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).contains(helper::LANDLOCK_HELD))
    }
}

impl Sandbox for Bubblewrap {
    fn status(&self) -> SandboxStatus {
        self.status.clone()
    }

    fn wrap(&self, profile: &SandboxProfile, line: CommandLine) -> Result<CommandLine> {
        if !matches!(self.status, SandboxStatus::Available { .. }) {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "the sandbox is not available",
            ));
        }
        let plan = plan::Plan::new(&self.setup, profile)?;
        let landlock = if self.landlock {
            Landlock::Require
        } else {
            Landlock::Try
        };
        plan.line(
            &self.bwrap,
            &self.setup.helper,
            landlock,
            line,
            &outer_env(),
        )
    }
}

fn invalid(what: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, what.to_string())
}

/// The names in Ovra's environment now, when the session starts.
fn outer_env() -> Vec<OsString> {
    std::env::vars_os().map(|(name, _)| name).collect()
}

fn os(s: impl AsRef<OsStr>) -> OsString {
    s.as_ref().to_os_string()
}
