//! The agent sandbox on Windows (`docs/specs/sandbox-windows.md`, task
//! 5.A): `claude` runs as the local sandbox user with a write-restricted
//! token, behind a firewall rule that leaves it only the netproxy.
//! Contract: `ovra_core::services::Sandbox`; `ovra-sandbox` picks it.
//!
//! What to grant, deny and keep ([`plan`]) is planned from paths alone, so
//! it is built and tested on every OS; only applying it needs Windows.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{CommandLine, Result, Sandbox, SandboxProfile, SandboxStatus};

#[cfg(windows)]
mod helper;
mod plan;
mod protected;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use helper::helper_if_asked;

/// The first argument that makes a program the sandbox helper.
pub const HELPER_FLAG: &str = "--ovra-sandbox-helper";
/// The file in Ovra's data folder that holds the sandbox user's password,
/// sealed with DPAPI for the person: there once setup has run.
pub(crate) const PASSWORD_FILE: &str = "sandbox-user";

/// What the sandbox needs to know about the machine, apart from the
/// session's profile.
#[derive(Debug, Clone)]
pub struct Setup {
    /// A program that runs the helper first thing: the daemon or the app
    /// itself.
    pub helper: PathBuf,
    /// The person's home folder: unreadable to the sandbox user, except
    /// what the plan grants.
    pub home: PathBuf,
    /// `PATH` as Ovra sees it.
    pub path: OsString,
    /// Ovra's data folder: never granted, except the folders below.
    pub data_dir: PathBuf,
    /// Where sessions keep their package caches, one folder per project.
    pub caches: PathBuf,
    /// Where sessions keep their temporary files, one folder per project.
    pub temp: PathBuf,
    /// The protected paths under `work_dir` every platform shares
    /// (`ovra-sandbox`); `**/name` is any depth.
    pub protected: &'static [&'static str],
}

impl Setup {
    /// The setup for this process: its own executable as the helper, the
    /// home folder from the environment, and folders in `data_dir`.
    pub fn from_env(data_dir: &Path, protected: &'static [&'static str]) -> Option<Self> {
        let home = std::env::home_dir().filter(|h| h.is_absolute() && h.parent().is_some())?;
        let data_dir = std::path::absolute(data_dir).ok()?;
        Some(Self {
            helper: std::env::current_exe().ok()?,
            home,
            path: std::env::var_os("PATH").unwrap_or_default(),
            caches: data_dir.join("sandbox-caches"),
            temp: data_dir.join("sandbox-temp"),
            data_dir,
            protected,
        })
    }
}

/// What the Windows sandbox does not isolate yet, as sessions report it
/// (`docs/specs/sandbox-windows.md`, "What is not isolated"). WFP filters
/// are meant to close the network ones.
const GAPS: [&str; 5] = [
    "DNS lookups",
    "loopback services",
    "Windows file sharing (SMB, WebDAV, BITS)",
    "reads in other sandboxed projects, caches and temp folders",
    "processes started through WMI, the task scheduler or COM servers",
];

/// The Windows sandbox: the sandbox user and a write-restricted token.
#[derive(Debug)]
pub struct Restricted {
    setup: Setup,
    status: SandboxStatus,
}

impl Restricted {
    /// Whether this machine can sandbox agents: once the one-time setup
    /// has run for this data folder. Until then sessions run unsandboxed,
    /// as before 5.A. The helper checks the rest when a session starts (the
    /// account, its password, the firewall rule) and refuses it if any is
    /// missing.
    pub fn detect(setup: Setup) -> Self {
        let status = if setup.data_dir.join(PASSWORD_FILE).is_file() {
            SandboxStatus::Available {
                name: "Windows sandbox user".into(),
                gaps: GAPS.map(String::from).to_vec(),
            }
        } else {
            SandboxStatus::Unavailable {
                reason: "set up the Windows sandbox (administrator, once)".into(),
            }
        };
        Self { setup, status }
    }
}

impl Sandbox for Restricted {
    fn status(&self) -> SandboxStatus {
        self.status.clone()
    }

    fn wrap(&self, profile: &SandboxProfile, line: CommandLine) -> Result<CommandLine> {
        if matches!(self.status, SandboxStatus::Unavailable { .. }) {
            return Ok(line);
        }
        let plan = plan::Plan::new(&self.setup, profile, &line.program)?;
        let outer: Vec<OsString> = std::env::vars_os().map(|(name, _)| name).collect();
        Ok(plan.line(&self.setup, line, &outer))
    }
}

fn invalid(what: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, what.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use ovra_core::services::ProfileNetwork;

    use super::*;

    #[test]
    fn until_it_is_set_up_it_says_so_and_changes_nothing() {
        let setup = Setup {
            helper: "ovra".into(),
            home: "home".into(),
            path: OsString::new(),
            data_dir: "no-such-data".into(),
            caches: "data/caches".into(),
            temp: "data/temp".into(),
            protected: &[],
        };
        let sandbox = Restricted::detect(setup);
        assert!(matches!(
            sandbox.status(),
            SandboxStatus::Unavailable { .. }
        ));
        let line = CommandLine {
            program: "claude".into(),
            args: vec!["-p".into()],
            env: Vec::new(),
            cwd: "w".into(),
        };
        let profile = SandboxProfile {
            kind: ovra_core::services::ProfileKind::Agent,
            claude_dir: None,
            project: None,
            work_env: None,
            work_dir: "w".into(),
            work_dir_read_only: false,
            read: Vec::new(),
            write: Vec::new(),
            expose: Vec::new(),
            forward: Vec::new(),
            network: ProfileNetwork::Blocked,
        };
        assert_eq!(sandbox.wrap(&profile, line.clone()).ok(), Some(line));
    }

    #[test]
    fn once_set_up_it_is_available() {
        let data = std::env::temp_dir().join(format!("ovra-win-detect-{}", std::process::id()));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join(PASSWORD_FILE), b"sealed").unwrap();
        let setup = Setup {
            helper: "ovra".into(),
            home: "home".into(),
            path: OsString::new(),
            data_dir: data.clone(),
            caches: data.join("caches"),
            temp: data.join("temp"),
            protected: &[],
        };
        let status = Restricted::detect(setup).status();
        let _ = std::fs::remove_dir_all(&data);
        let SandboxStatus::Available { name, gaps } = status else {
            panic!("{status:?}");
        };
        assert_eq!(name, "Windows sandbox user");
        // Every network gap the spec lists stays visible until WFP closes it.
        for gap in [
            "DNS lookups",
            "loopback services",
            "Windows file sharing (SMB, WebDAV, BITS)",
        ] {
            assert!(gaps.iter().any(|g| g == gap), "{gap} missing from {gaps:?}");
        }
        assert_eq!(gaps.len(), 5, "{gaps:?}");
    }
}
