//! The protected paths under a session's `work_dir` (`docs/specs/security.md`,
//! "Protected paths"). The sandbox keeps them read-only (`ovra-sandbox`
//! re-exports the list for every platform); the review marks any change to
//! one, because not every platform can protect a path that does not exist
//! yet.

/// Names right in the home folder that hold a known agent's own state: its
/// settings, login and transcripts (`.claude`, `.claude.json`). No sandbox
/// lets anything write in them, whoever it is for and whichever provider a
/// session runs. On Linux every name that starts with one is refused too:
/// an account's own folder such as `.claude-work`. Each provider's
/// `ProviderLists::home_dir` and `state_file` must be in this list, or
/// pods, the browser and other providers' sessions could write them; each
/// provider's runtime tests that (for Claude,
/// `claudes_own_state_is_protected_in_every_sandbox` in agent-runtime).
pub const AGENT_STATE_IN_HOME: [&str; 2] = [".claude", ".claude.json"];

/// Paths under a session's `work_dir` that stay read-only: agent
/// configuration and code that tools run on their own. `**/` matches any
/// folder, `*` one name. A folder covers everything in it.
pub const PROTECTED_IN_WORK_DIR: [&str; 19] = [
    ".claude",
    ".mcp.json",
    "**/CLAUDE.md",
    "**/CLAUDE.local.md",
    "**/AGENTS.md",
    // Other agents' project configuration: MCP server commands, hooks and
    // settings they load when the person later runs them outside Ovra's
    // sandbox. Only names whose parent is the work dir itself: a protected
    // path below a folder that is a link could be moved away with it.
    ".codex",
    ".gemini",
    ".cursor",
    ".opencode",
    "opencode.json",
    "opencode.jsonc",
    ".aider.conf.yml",
    ".vscode",
    ".idea",
    ".zed",
    ".devcontainer",
    ".envrc",
    // Ovra's own: `servers.json` holds the commands a server start runs,
    // `worktree.json` the setup steps of new worktrees.
    ".ovra",
    // The ignored files copied into every new worktree: an agent that
    // could add `.env` to it would read the copy in its next worker.
    ".worktreeinclude",
];

/// Paths the review marks too although the sandbox does not keep them
/// read-only: code that tools run on their own wherever it is (direnv
/// loads an `.envrc` in any folder, Claude Code a nested `.claude/`,
/// Codex a nested `.codex/`, opencode an `opencode.json` up to the git
/// root), git hooks managers and attributes (SEC-5), other agents'
/// instruction files (a rules file is often a link to `AGENTS.md`, which
/// the sandbox cannot keep read-only, so it would refuse the session),
/// the configuration of agents Ovra does not know well yet, and GitHub
/// workflows and actions, which run with the repository's secrets as soon
/// as a pull request's branch is pushed.
const MARKED_IN_REVIEW: [&str; 39] = [
    "**/.envrc",
    "**/.claude",
    "**/.codex",
    "**/.gemini",
    "**/.cursor",
    "**/opencode.json",
    "**/opencode.jsonc",
    ".husky",
    "**/.gitattributes",
    ".gitmodules",
    ".lfsconfig",
    "**/GEMINI.md",
    "**/AGENTS.override.md",
    "**/QWEN.md",
    ".cursorrules",
    ".goosehints",
    ".github/copilot-instructions.md",
    ".github/instructions",
    ".github/agents",
    ".github/prompts",
    ".github/chatmodes",
    ".github/hooks",
    ".github/workflows",
    ".github/actions",
    ".agents",
    ".qwen",
    ".kiro",
    ".windsurf",
    ".windsurfrules",
    ".amazonq",
    ".roo",
    ".roomodes",
    ".kilocode",
    ".clinerules",
    ".continue",
    ".crush",
    "crush.json",
    ".factory",
    ".junie",
];

/// Whether `path` (relative, `/`-separated, as in a review) is a protected
/// path or inside one. Case is always ignored: any system can hold a
/// project on a case-insensitive file system, and a mark too many costs
/// nothing. (The sandbox binds the names it finds on disk instead.)
pub(crate) fn is_protected(path: &str) -> bool {
    matches(&PROTECTED_IN_WORK_DIR, path)
}

/// Whether the review marks a change to `path`: a protected path, or one
/// of [`MARKED_IN_REVIEW`].
fn is_marked(path: &str) -> bool {
    is_protected(path) || matches(&MARKED_IN_REVIEW, path)
}

fn matches(patterns: &[&str], path: &str) -> bool {
    let name = |s: &str| s.to_lowercase();
    let path: Vec<String> = path
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .map(name)
        .collect();
    patterns.iter().any(|pattern| {
        let (deep, pattern) = match pattern.strip_prefix("**/") {
            Some(rest) => (true, rest),
            None => (false, *pattern),
        };
        let pattern: Vec<String> = pattern.split('/').map(name).collect();
        let starts = if deep { 0..path.len() } else { 0..1 };
        starts
            .into_iter()
            .any(|start| prefix(&pattern, &path[start..]))
    })
}

/// Whether a change to `path`, renamed from `from`, is marked in a review.
pub(crate) fn changes(path: &str, from: Option<&str>) -> bool {
    is_marked(path) || from.is_some_and(is_marked)
}

/// Files the Linux sandbox makes empty and read-only at the top of a work
/// dir while a session runs, when they are missing, and removes after it
/// (`crates/sandbox-linux`, `FILES_MADE`). Git status leaves them out
/// while they are untracked and empty, so the git panel neither lists them
/// nor commits them.
pub const PLACEHOLDERS: [&str; 6] = [
    "CLAUDE.md",
    "CLAUDE.local.md",
    "AGENTS.md",
    ".mcp.json",
    ".envrc",
    ".worktreeinclude",
];

/// Git's empty blob, in SHA-1 and SHA-256 repositories, as a regular file.
const EMPTY_FILE: [&str; 2] = [
    "100644 e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
    "100644 473a0f4c3be8a93681a267e3b1e9a7dcda1185436fe141f7749120a303721813",
];

/// Whether `delta` is only the sandbox's placeholder: an added empty
/// file at one of [`PLACEHOLDERS`]. No agent can write it, and it goes
/// with the session, so a review never lists it (it would ask the person
/// to include a protected file on every session).
pub(crate) fn is_placeholder(delta: &crate::services::Delta) -> bool {
    delta.kind == crate::protocol::review::ChangeKind::Added
        && delta.from.is_none()
        && PLACEHOLDERS.contains(&delta.path.as_str())
        && EMPTY_FILE.contains(&delta.version.as_str())
}

/// Whether `path` starts with the segments of `pattern` (`*`: any one name).
fn prefix(pattern: &[String], path: &[String]) -> bool {
    pattern.len() <= path.len() && pattern.iter().zip(path).all(|(p, s)| p == "*" || p == s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_added_empty_top_level_placeholder_is_left_out() {
        use crate::protocol::review::ChangeKind;
        use crate::services::Delta;
        let delta = |path: &str, kind, version: &str| Delta {
            path: path.into(),
            kind,
            from: None,
            additions: 0,
            deletions: 0,
            version: version.into(),
        };
        let empty = EMPTY_FILE[0];
        // Every file the Linux sandbox makes, the newest one too.
        assert!(is_placeholder(&delta(
            ".worktreeinclude",
            ChangeKind::Added,
            empty
        )));
        for path in PLACEHOLDERS {
            assert!(
                is_placeholder(&delta(path, ChangeKind::Added, empty)),
                "{path}"
            );
            assert!(is_placeholder(&delta(
                path,
                ChangeKind::Added,
                EMPTY_FILE[1]
            )));
        }
        for (path, kind, version) in [
            ("CLAUDE.md", ChangeKind::Added, "100644 0123"),
            (
                "CLAUDE.md",
                ChangeKind::Added,
                "100755 e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
            ),
            (
                "CLAUDE.md",
                ChangeKind::Added,
                "120000 e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
            ),
            ("CLAUDE.md", ChangeKind::Modified, empty),
            ("CLAUDE.md", ChangeKind::Deleted, ""),
            ("src/CLAUDE.md", ChangeKind::Added, empty),
            ("sub/.worktreeinclude", ChangeKind::Added, empty),
            (".worktreeinclude", ChangeKind::Added, "100644 0123"),
            ("claude.md", ChangeKind::Added, empty),
            (".claude", ChangeKind::Added, empty),
        ] {
            assert!(
                !is_placeholder(&delta(path, kind, version)),
                "{path} {kind:?} {version}"
            );
        }
    }

    #[test]
    fn agent_configuration_and_tool_folders_are_protected() {
        for path in [
            ".claude/settings.json",
            ".claude",
            ".mcp.json",
            "CLAUDE.md",
            "AGENTS.md",
            ".vscode/tasks.json",
            ".devcontainer/devcontainer.json",
            ".envrc",
            ".ovra/servers.json",
            ".OVRA",
            ".worktreeinclude",
        ] {
            assert!(is_protected(path), "{path}");
        }
    }

    #[test]
    fn other_agents_configuration_is_protected() {
        for path in [
            ".codex/config.toml",
            ".gemini/settings.json",
            ".cursor/rules/style.mdc",
            ".cursor/mcp.json",
            ".opencode/agent/a.md",
            "opencode.json",
            "opencode.jsonc",
            ".aider.conf.yml",
            ".zed/settings.json",
        ] {
            assert!(is_protected(path), "{path}");
        }
    }

    #[test]
    fn other_agents_nested_config_and_instructions_are_marked_only() {
        for path in [
            "sub/.codex/config.toml",
            "sub/.gemini/settings.json",
            "sub/.cursor/rules/a.mdc",
            "docs/opencode.json",
            "GEMINI.md",
            "docs/GEMINI.md",
            "src/AGENTS.override.md",
            ".cursorrules",
            ".goosehints",
            ".github/copilot-instructions.md",
            ".github/instructions/rust.instructions.md",
            ".github/agents/reviewer.md",
            ".github/hooks/pre.json",
            ".kiro/hooks/x.json",
            ".windsurf/rules/a.md",
        ] {
            assert!(!is_protected(path), "{path}");
            assert!(changes(path, None), "{path}");
        }
    }

    #[test]
    fn github_workflows_and_actions_are_marked() {
        for path in [
            ".github/workflows/ci.yml",
            ".GitHub/Workflows/release.yaml",
            ".github/actions/setup/action.yml",
        ] {
            assert!(!is_protected(path), "{path}");
            assert!(changes(path, None), "{path}");
        }
        assert!(changes("ci.yml", Some(".github/workflows/ci.yml")));
        assert!(!changes(".github/CODEOWNERS", None));
        assert!(!changes("sub/.github/workflows/ci.yml", None));
    }

    #[test]
    fn claude_md_is_protected_at_any_depth() {
        assert!(is_protected("docs/CLAUDE.md"));
        assert!(is_protected("a/b/c/CLAUDE.local.md"));
        assert!(is_protected("ui/src/AGENTS.md"));
    }

    #[test]
    fn case_is_ignored_on_every_system() {
        assert!(is_protected("docs/claude.md"));
        assert!(is_protected(".Claude/settings.json"));
        assert!(is_protected(".VSCODE/launch.json"));
        assert!(is_protected("Agents.MD"));
    }

    #[test]
    fn a_rename_from_or_to_a_protected_path_is_marked() {
        assert!(changes("notes.json", Some(".claude/settings.json")));
        assert!(changes("CLAUDE.md", Some("a.md")));
        assert!(!changes("b", Some("a")));
        assert!(!changes("b", None));
    }

    #[test]
    fn the_review_also_marks_code_tools_run_from_any_folder() {
        for path in [
            "sub/.envrc",
            "a/b/.claude/settings.json",
            ".husky/pre-commit",
            ".husky/_/h",
            "src/.gitattributes",
            ".gitmodules",
            ".lfsconfig",
            ".cursor/rules",
            "docs/GEMINI.md",
        ] {
            assert!(changes(path, None), "{path}");
            assert!(changes("moved", Some(path)), "{path}");
        }
        assert!(!is_protected("sub/.envrc"), "the sandbox list is unchanged");
        for path in ["notes/.envrc.example", "sub/.vscode/tasks.json", "husky/x"] {
            assert!(!changes(path, None), "{path}");
        }
    }

    #[test]
    fn other_paths_are_not_protected() {
        for path in [
            "src/main.rs",
            // Only the top-level folders are protected, not ones below.
            "sub/.claude/settings.json",
            "sub/.vscode/tasks.json",
            ".claudex/x",
            ".mcp.json.bak",
            "CLAUDE.md.orig",
            "notes/.envrc.example",
            "sub/.ovra/servers.json",
            ".ovra.json",
            "sub/.worktreeinclude",
        ] {
            assert!(!is_protected(path), "{path}");
        }
    }
}
