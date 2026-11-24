//! What every platform's sandbox shares (`docs/specs/security.md`): the
//! paths an agent never writes, and [`for_this_os`], the sandbox the daemon
//! and the app hand to the agent runtime. The platform crates
//! (`sandbox-linux`, `sandbox-macos`) implement the wrapping itself.
//! Contract: `ovra_core::services::Sandbox`; fake:
//! `ovra_core::fakes::FakeSandbox`.

use std::path::Path;
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
/// `config.worktree` is read when `extensions.worktreeConfig` is on.
pub const PROTECTED_IN_GIT_DIR: [&str; 7] = [
    "hooks",
    "config",
    "config.worktree",
    "modules/*/hooks",
    "modules/*/config",
    "refs/ovra",
    "logs/refs/ovra",
];

/// For a worker: files in its `<git dir>/worktrees/<name>` that stay
/// read-only, because Ovra later runs git there unsandboxed. Its
/// worktree's `.git` file is protected as well.
pub const PROTECTED_IN_WORKTREE_GIT_DIR: [&str; 3] = ["commondir", "gitdir", "config.worktree"];

/// The sandbox for this OS, hiding Ovra's `data_dir` from agents. Where no
/// platform crate exists (macOS until 4.B, Windows until 5.A), sessions run
/// unsandboxed, as they did before phase 4, and say so. On Linux the
/// sandbox tries itself once here and reports why if it cannot work.
pub fn for_this_os(data_dir: &Path) -> Arc<dyn Sandbox> {
    #[cfg(target_os = "linux")]
    {
        let protected = ovra_sandbox_linux::Protected {
            work_dir: &PROTECTED_IN_WORK_DIR,
            worktree_git_dir: &PROTECTED_IN_WORKTREE_GIT_DIR,
        };
        match ovra_sandbox_linux::Setup::from_env(data_dir, protected) {
            Some(setup) => Arc::new(ovra_sandbox_linux::Bubblewrap::detect(setup)),
            None => Arc::new(Unsandboxed),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = data_dir;
        Arc::new(Unsandboxed)
    }
}

/// Call first in `main`: if this process was started as the sandbox's
/// helper inside a sandbox, does that job and never returns.
pub fn helper_if_asked() {
    #[cfg(target_os = "linux")]
    ovra_sandbox_linux::helper_if_asked();
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
    fn unsandboxed_says_so_and_runs_the_line_as_it_is() {
        let sandbox = Unsandboxed;
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
