//! What every platform's sandbox shares (`docs/specs/security.md`): the
//! paths an agent never writes, and [`for_this_os`], the sandbox the daemon
//! and the app hand to the agent runtime. The platform crates
//! (`sandbox-linux`, `sandbox-macos`) implement the wrapping itself.
//! Contract: `ovra_core::services::Sandbox`; fake:
//! `ovra_core::fakes::FakeSandbox`.

use std::sync::Arc;

use ovra_core::services::{CommandLine, Result, Sandbox, SandboxProfile, SandboxStatus};

/// Paths under a session's `work_dir` that stay read-only: agent
/// configuration and code that tools run on their own. `**/` matches any
/// folder, `*` one name.
pub const PROTECTED_IN_WORK_DIR: [&str; 9] = [
    ".claude",
    ".mcp.json",
    "**/CLAUDE.md",
    "**/CLAUDE.local.md",
    "**/AGENTS.md",
    ".vscode",
    ".idea",
    ".devcontainer",
    ".envrc",
];

/// Paths under the repository's git folder that stay read-only. The folder
/// `core.hooksPath` names is protected too; the platform reads it.
pub const PROTECTED_IN_GIT_DIR: [&str; 6] = [
    "hooks",
    "config",
    "modules/*/hooks",
    "modules/*/config",
    "refs/ovra",
    "logs/refs/ovra",
];

/// For a worker: files in its `<git dir>/worktrees/<name>` that stay
/// read-only, because Ovra later runs git there unsandboxed. Its
/// worktree's `.git` file is protected as well.
pub const PROTECTED_IN_WORKTREE_GIT_DIR: [&str; 3] = ["commondir", "gitdir", "config.worktree"];

/// The sandbox for this OS. No platform crate exists yet, so every session
/// runs unsandboxed, as it did before phase 4, and says so.
pub fn for_this_os() -> Arc<dyn Sandbox> {
    Arc::new(Unsandboxed)
}

/// No sandbox: [`SandboxStatus::Unavailable`], and the line runs as it is.
#[derive(Debug)]
pub struct Unsandboxed;

impl Sandbox for Unsandboxed {
    fn status(&self) -> SandboxStatus {
        SandboxStatus::Unavailable {
            reason: "Ovra has no sandbox for this system yet".into(),
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
    fn without_a_platform_sessions_run_unsandboxed_and_say_so() {
        let sandbox = for_this_os();
        assert!(matches!(
            sandbox.status(),
            SandboxStatus::Unavailable { .. }
        ));
        let line = CommandLine {
            program: "claude".into(),
            args: vec!["-p".into()],
            env: vec![("CLAUDECODE".into(), None)],
            cwd: "/w".into(),
        };
        let profile = SandboxProfile {
            work_dir: "/w".into(),
            read: Vec::new(),
            write: Vec::new(),
            network: ProfileNetwork::Open,
        };
        let wrapped = sandbox.wrap(&profile, line.clone());
        assert_eq!(wrapped.ok(), Some(line));
    }

    #[test]
    fn protected_paths_are_relative_and_stay_inside() {
        let all = PROTECTED_IN_WORK_DIR
            .iter()
            .chain(&PROTECTED_IN_GIT_DIR)
            .chain(&PROTECTED_IN_WORKTREE_GIT_DIR);
        for path in all {
            assert!(!path.starts_with('/') && !path.contains('\\'), "{path}");
            assert!(
                path.split('/').all(|s| !s.is_empty() && s != ".."),
                "{path}"
            );
        }
    }
}
