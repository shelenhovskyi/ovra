//! The agent sandbox on Windows (`docs/specs/sandbox-windows.md`, task
//! 5.A): `claude` runs as the local sandbox user with a write-restricted
//! token, behind a firewall rule that leaves it only the netproxy.
//! Contract: `ovra_core::services::Sandbox`; `ovra-sandbox` picks it.
//!
//! What to grant, deny and keep is planned from paths alone, so the plan is
//! built and tested on every OS; only applying it needs Windows.

use ovra_core::services::{CommandLine, Result, Sandbox, SandboxProfile, SandboxStatus};

/// The Windows sandbox. Until its setup and helper land, it reports itself
/// unavailable and sessions run unsandboxed, as before 5.A.
#[derive(Debug, Default)]
pub struct Restricted;

impl Sandbox for Restricted {
    fn status(&self) -> SandboxStatus {
        SandboxStatus::Unavailable {
            reason: "the Windows sandbox is not built yet".into(),
        }
    }

    fn wrap(&self, _profile: &SandboxProfile, line: CommandLine) -> Result<CommandLine> {
        Ok(line)
    }
}

#[cfg(test)]
mod tests {
    use ovra_core::services::ProfileNetwork;

    use super::*;

    #[test]
    fn until_it_is_built_it_says_so_and_changes_nothing() {
        let sandbox = Restricted;
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
            work_dir: "w".into(),
            read: Vec::new(),
            write: Vec::new(),
            network: ProfileNetwork::Blocked,
        };
        assert_eq!(sandbox.wrap(&profile, line.clone()).ok(), Some(line));
    }
}
