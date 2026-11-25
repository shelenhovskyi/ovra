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

/// Whether `path` (relative, `/`-separated, as in a review) is a protected
/// path or inside one. Case is always ignored: any system can hold a
/// project on a case-insensitive file system, and a mark too many costs
/// nothing. (The sandbox binds the names it finds on disk instead.)
pub(crate) fn is_protected(path: &str) -> bool {
    let name = |s: &str| s.to_lowercase();
    let path: Vec<String> = path
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .map(name)
        .collect();
    PROTECTED_IN_WORK_DIR.iter().any(|pattern| {
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

/// Whether a change to `path`, renamed from `from`, touches a protected path.
pub(crate) fn changes(path: &str, from: Option<&str>) -> bool {
    is_protected(path) || from.is_some_and(is_protected)
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
