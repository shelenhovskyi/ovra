//! Rules a `PreToolUse` hook applies to every tool call. Hooks run even for
//! calls the user's own settings allow without asking, so the rules here
//! hold whatever those settings say. They only ever ask or deny; they never
//! allow on their own.

use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

/// What the hook tells the CLI.
#[derive(Debug, PartialEq)]
pub(crate) enum Verdict {
    /// No rule applies; the CLI's normal permission flow decides.
    Pass,
    /// The person must approve the call through Ovra, even if a setting
    /// allows it.
    Ask(&'static str),
}

impl Verdict {
    /// The hook output in the shape the CLI expects.
    pub(crate) fn to_hook_output(&self) -> Value {
        match self {
            Self::Pass => json!({}),
            Self::Ask(reason) => json!({"hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "ask",
                "permissionDecisionReason": reason,
            }}),
        }
    }
}

/// Tools that change a file, and the input field naming it.
const FILE_EDITS: [(&str, &str); 4] = [
    ("Edit", "file_path"),
    ("MultiEdit", "file_path"),
    ("Write", "file_path"),
    ("NotebookEdit", "notebook_path"),
];

/// Checks one `PreToolUse` hook input. `cwd` is the folder Ovra started the
/// session in, not the `cwd` the hook input reports.
pub(crate) fn pre_tool_use(cwd: &Path, input: &Value) -> Verdict {
    let tool = input.get("tool_name").and_then(Value::as_str);
    let Some((_, field)) = FILE_EDITS.iter().find(|(name, _)| Some(*name) == tool) else {
        return Verdict::Pass;
    };
    let path = input
        .pointer(&format!("/tool_input/{field}"))
        .and_then(Value::as_str);
    match path {
        Some(path) if inside(cwd, Path::new(path)) => Verdict::Pass,
        _ => Verdict::Ask("Ovra asks before an agent changes files outside its folder."),
    }
}

/// Whether `path` (absolute, or relative to `cwd`) stays inside `cwd`.
/// Lexical: `..` is resolved, symbolic links are not (the sandbox in phase 4
/// covers those).
fn inside(cwd: &Path, path: &Path) -> bool {
    let mut resolved = PathBuf::new();
    for component in cwd.join(path).components() {
        match component {
            Component::ParentDir => {
                if !resolved.pop() {
                    return false;
                }
            }
            Component::CurDir => {}
            other => resolved.push(other),
        }
    }
    resolved.starts_with(cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(tool: &str, input: Value) -> Verdict {
        let cwd = if cfg!(windows) { r"C:\w\p" } else { "/w/p" };
        pre_tool_use(
            Path::new(cwd),
            &json!({"hook_event_name": "PreToolUse", "tool_name": tool, "tool_input": input}),
        )
    }

    #[test]
    fn edits_inside_the_folder_pass() {
        let inside = if cfg!(windows) {
            r"C:\w\p\src\a.rs"
        } else {
            "/w/p/src/a.rs"
        };
        assert_eq!(verdict("Edit", json!({"file_path": inside})), Verdict::Pass);
        assert_eq!(
            verdict("Write", json!({"file_path": "src/../b.rs"})),
            Verdict::Pass
        );
        assert_eq!(
            verdict("Bash", json!({"command": "rm -rf /"})),
            Verdict::Pass
        );
        assert_eq!(
            verdict("Read", json!({"file_path": "/etc/passwd"})),
            Verdict::Pass
        );
    }

    #[test]
    fn edits_outside_the_folder_ask() {
        let outside = if cfg!(windows) {
            r"C:\w\other\a.rs"
        } else {
            "/w/other/a.rs"
        };
        for (tool, input) in [
            ("Edit", json!({"file_path": outside})),
            ("Write", json!({"file_path": "../p2/x"})),
            ("MultiEdit", json!({"file_path": "a/../../../../../x"})),
            ("NotebookEdit", json!({"notebook_path": "../n.ipynb"})),
            ("Write", json!({})),
        ] {
            let v = verdict(tool, input.clone());
            assert!(matches!(v, Verdict::Ask(_)), "{tool} {input}");
            assert_eq!(
                v.to_hook_output()
                    .pointer("/hookSpecificOutput/permissionDecision"),
                Some(&json!("ask"))
            );
        }
    }
}
