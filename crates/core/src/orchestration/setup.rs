//! `.ovra/worktree.json`: the steps that prepare a worker's new worktree
//! (`docs/specs/worktree-setup.md`), checked as the spec says. `.ovra` is
//! protected, so only the person changes it.

// WS.C reads the file and runs the steps; until then only tests use this.
#![cfg_attr(not(test), expect(dead_code, reason = "used by worktree setup (WS.C)"))]

use std::time::Duration;

use serde::Deserialize;

/// Where a project defines its worktree setup.
pub(crate) const FILE: &str = ".ovra/worktree.json";

/// A step's time limit when the file gives none.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// The longest a step may be given.
const MAX_TIMEOUT: Duration = Duration::from_secs(3600);

/// More steps than this in one file is a mistake, not a project.
const MAX_STEPS: usize = 32;

/// One step: a program and its arguments, run without a shell in the
/// worktree's root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SetupDef {
    pub(crate) command: Vec<String>,
    pub(crate) timeout: Duration,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    setup: Vec<Step>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    command: Vec<String>,
    /// Seconds.
    timeout: Option<u64>,
}

/// Parses and checks the file's text: the steps in order, or why the file
/// cannot be used, in words for people.
pub(crate) fn parse(text: &str) -> Result<Vec<SetupDef>, String> {
    let file: File = serde_json::from_str(text).map_err(|e| format!("{FILE}: {e}"))?;
    if file.setup.len() > MAX_STEPS {
        return Err(format!("{FILE}: more than {MAX_STEPS} setup steps"));
    }
    file.setup
        .into_iter()
        .enumerate()
        .map(|(i, step)| check(step).map_err(|why| format!("{FILE}: setup[{i}]: {why}")))
        .collect()
}

fn check(step: Step) -> Result<SetupDef, String> {
    match step.command.first() {
        None => return Err("the command is empty".to_owned()),
        Some(program) if program.is_empty() => return Err("the program is empty".to_owned()),
        Some(_) if step.command.iter().any(|a| a.contains('\0')) => {
            return Err("the command contains a NUL byte".to_owned());
        }
        Some(_) => {}
    }
    let timeout = match step.timeout {
        None => DEFAULT_TIMEOUT,
        Some(0) => return Err("the timeout must be at least 1 second".to_owned()),
        Some(secs) if secs > MAX_TIMEOUT.as_secs() => {
            return Err(format!(
                "the timeout must be at most {} seconds",
                MAX_TIMEOUT.as_secs()
            ));
        }
        Some(secs) => Duration::from_secs(secs),
    };
    Ok(SetupDef {
        command: step.command,
        timeout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_keep_their_order_and_get_the_default_timeout() {
        let text = r#"{"setup": [
            {"command": ["pnpm", "install", "--frozen-lockfile"]},
            {"command": ["sh", ".ovra/setup.sh"], "timeout": 900}
        ]}"#;
        let steps = parse(text).unwrap_or_default();
        let summary: Vec<_> = steps
            .iter()
            .map(|s| (s.command.join(" "), s.timeout.as_secs()))
            .collect();
        assert_eq!(
            summary,
            [
                ("pnpm install --frozen-lockfile".to_owned(), 600),
                ("sh .ovra/setup.sh".to_owned(), 900)
            ]
        );
        assert_eq!(parse("{}"), Ok(Vec::new()));
    }

    #[test]
    fn a_broken_file_says_what_is_wrong() {
        for (text, why) in [
            (
                r#"{"setup": [{"command": []}]}"#,
                "setup[0]: the command is empty",
            ),
            (
                r#"{"setup": [{"command": ["x"], "timeout": 3601}]}"#,
                "setup[0]: the timeout must be at most 3600 seconds",
            ),
            (
                r#"{"setup": [{"command": ["x"]}, {"command": [""]}]}"#,
                "setup[1]: the program is empty",
            ),
            (
                r#"{"setup": [{"command": ["x"], "shell": true}]}"#,
                "unknown field",
            ),
            ("not json", "expected"),
        ] {
            let err = parse(text).err().unwrap_or_default();
            assert!(err.starts_with(FILE) && err.contains(why), "{text}: {err}");
        }
    }
}
