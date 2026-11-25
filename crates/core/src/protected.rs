//! The protected paths under a session's `work_dir` (`docs/specs/security.md`,
//! "Protected paths"). The sandbox keeps them read-only (`ovra-sandbox`
//! re-exports the list for every platform); the review marks any change to
//! one, because not every platform can protect a path that does not exist
//! yet.

/// Paths under a session's `work_dir` that stay read-only: agent
/// configuration and code that tools run on their own. `**/` matches any
/// folder, `*` one name. A folder covers everything in it.
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

/// Whether `path` starts with the segments of `pattern` (`*`: any one name).
fn prefix(pattern: &[String], path: &[String]) -> bool {
    pattern.len() <= path.len() && pattern.iter().zip(path).all(|(p, s)| p == "*" || p == s)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        ] {
            assert!(!is_protected(path), "{path}");
        }
    }
}
