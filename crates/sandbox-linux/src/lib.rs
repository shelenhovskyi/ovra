//! The agent sandbox on Linux (`docs/specs/security.md`, task 4.A):
//! bubblewrap, with Landlock and seccomp applied inside it by a helper.
//! Contract: `ovra_core::services::Sandbox`; `ovra-sandbox` picks it.
//! A stub until 4.A lands: never available.

#![cfg(target_os = "linux")]

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{CommandLine, Result, Sandbox, SandboxProfile, SandboxStatus};

/// The bubblewrap sandbox; not built yet.
#[derive(Debug)]
pub struct Bubblewrap;

impl Sandbox for Bubblewrap {
    fn status(&self) -> SandboxStatus {
        SandboxStatus::Unavailable {
            reason: "the Linux sandbox is not built yet".into(),
        }
    }

    fn wrap(&self, _profile: &SandboxProfile, _line: CommandLine) -> Result<CommandLine> {
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "the Linux sandbox is not built yet",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_is_never_available() {
        assert!(matches!(
            Bubblewrap.status(),
            SandboxStatus::Unavailable { .. }
        ));
        assert!(Bubblewrap.wrap(&profile(), line()).is_err());
    }

    fn profile() -> SandboxProfile {
        SandboxProfile {
            work_dir: "/w".into(),
            read: Vec::new(),
            write: Vec::new(),
            network: ovra_core::services::ProfileNetwork::Blocked,
        }
    }

    fn line() -> CommandLine {
        CommandLine {
            program: "claude".into(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: "/w".into(),
        }
    }
}
