//! The protected paths under a session's `work_dir` (`docs/specs/security.md`,
//! "Protected paths"). The sandbox keeps them read-only (`ovra-sandbox`
//! re-exports the list for every platform); the review marks any change to
//! one, because not every platform can protect a path that does not exist
//! yet.

/// Paths under a session's `work_dir` that stay read-only: agent
/// configuration and code that tools run on their own. `**/` matches any
/// folder, `*` one name. A folder covers everything in it.
pub const PROTECTED_IN_WORK_DIR: [&str; 10] = [
    ".claude",
    ".mcp.json",
    "**/CLAUDE.md",
    "**/CLAUDE.local.md",
    "**/AGENTS.md",
    ".vscode",
    ".idea",
    ".devcontainer",
    ".envrc",
    // Ovra's own: `servers.json` holds the commands a server start runs.
    ".ovra",
];

/// Paths the review marks too although the sandbox does not keep them
/// read-only: code that tools run on their own wherever it is (direnv
/// loads an `.envrc` in any folder, Claude Code a nested `.claude/`), git
/// hooks managers and attributes, and other agents' configuration
/// (SEC-5).
const MARKED_IN_REVIEW: [&str; 10] = [
    "**/.envrc",
    "**/.claude",
    ".husky",
    "**/.gitattributes",
    ".gitmodules",
    ".lfsconfig",
    ".zed",
    ".cursor",
    ".gemini",
    "**/GEMINI.md",
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
/// (`crates/sandbox-linux`, `FILES_MADE`).
const PLACEHOLDERS: [&str; 5] = [
    "CLAUDE.md",
    "CLAUDE.local.md",
    "AGENTS.md",
    ".mcp.json",
    ".envrc",
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
        ] {
            assert!(is_protected(path), "{path}");
        }
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
        ] {
            assert!(!is_protected(path), "{path}");
        }
    }
}
