//! How `claude` is started: the version check and the command line.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use tokio::process::Command;

/// Oldest version the protocol was checked against (the 2026-11-24 spike).
const MIN_VERSION: Version = Version(2, 1, 331);
/// First version that is not supported: a new major may change the
/// undocumented control messages.
const MAX_VERSION: Version = Version(3, 0, 0);
/// `claude --version` normally answers in well under a second.
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version(u32, u32, u32);

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// Reads `2.1.331` from output like `2.1.331 (Claude Code)`.
fn parse_version(output: &str) -> Option<Version> {
    let word = output.split_whitespace().next()?;
    let mut parts = word.split('.').map(|p| p.parse::<u32>().ok());
    let version = Version(parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// Why a version cannot be used, as a message for the person.
fn check(output: &str) -> Result<Version, String> {
    let Some(version) = parse_version(output) else {
        return Err(format!(
            "could not read the version of claude from {:?}",
            output.trim()
        ));
    };
    if version < MIN_VERSION {
        return Err(format!(
            "Claude Code {version} is too old; Ovra needs {MIN_VERSION} or newer. \
             Update it with `claude update`."
        ));
    }
    if version >= MAX_VERSION {
        return Err(format!(
            "Claude Code {version} is newer than this version of Ovra supports \
             (below {MAX_VERSION}). Update Ovra."
        ));
    }
    Ok(version)
}

/// Runs `claude --version` and checks the result against the tested range.
pub(crate) async fn check_version(executable: &Path) -> Result<Version, String> {
    let run = Command::new(executable)
        .arg("--version")
        .kill_on_drop(true)
        .output();
    let output = match tokio::time::timeout(VERSION_TIMEOUT, run).await {
        Err(_) => return Err(format!("{} --version did not answer", executable.display())),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!(
                "{} was not found. Install Claude Code and sign in once by running \
                 `claude` in a terminal.",
                executable.display()
            ));
        }
        Ok(Err(e)) => return Err(format!("could not run {}: {e}", executable.display())),
        Ok(Ok(output)) => output,
    };
    if !output.status.success() {
        return Err(format!(
            "{} --version failed: {}",
            executable.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    check(&String::from_utf8_lossy(&output.stdout))
}

/// Which Claude Code session the process works in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Conversation {
    /// A new session with an id Ovra chose.
    New(String),
    /// Continues an earlier session; `fork` branches it into a new one.
    Resume { id: String, fork: bool },
}

/// The arguments for one session. Ovra never passes `--bare` (it ignores the
/// subscription login) and never touches `CLAUDE_CONFIG_DIR` or credentials;
/// the environment is inherited unchanged (ADR 0006).
pub(crate) fn args(conversation: &Conversation) -> Vec<OsString> {
    let mut args: Vec<&str> = vec![
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        // Every approval comes to Ovra as a `can_use_tool` control request.
        "--permission-prompt-tool",
        "stdio",
        // Overrides a `defaultMode` (for example `bypassPermissions`) from
        // the user's settings.
        "--permission-mode",
        "default",
        // `-p` skips the workspace trust dialog, so a cloned project's
        // `.claude/` settings and hooks must not load. Projects cannot be
        // trusted in Ovra yet; once they can, trusted ones get
        // `user,project,local`.
        "--setting-sources",
        "user",
        // Only the MCP servers Ovra passes: none yet.
        "--strict-mcp-config",
        "--mcp-config",
        r#"{"mcpServers":{}}"#,
    ];
    match conversation {
        Conversation::New(id) => args.extend(["--session-id", id]),
        Conversation::Resume { id, fork } => {
            args.extend(["--resume", id]);
            if *fork {
                args.push("--fork-session");
            }
        }
    }
    args.into_iter().map(OsString::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_in_the_tested_range_pass() {
        assert_eq!(check("2.1.331 (Claude Code)\n"), Ok(Version(2, 1, 331)));
        assert_eq!(check("2.7.0"), Ok(Version(2, 7, 0)));
        assert!(check("2.1.330 (Claude Code)").is_err_and(|e| e.contains("too old")));
        assert!(check("3.0.0").is_err_and(|e| e.contains("newer")));
        assert!(check("claude 2.1").is_err_and(|e| e.contains("could not read")));
        assert!(check("2.1.331.4").is_err());
        assert!(check("").is_err());
    }

    fn line(conversation: &Conversation) -> String {
        args(conversation)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_command_line_keeps_the_safe_defaults() {
        let new = line(&Conversation::New("0b8e…".into()));
        insta::assert_snapshot!(new, @r#"-p --input-format stream-json --output-format stream-json --verbose --permission-prompt-tool stdio --permission-mode default --setting-sources user --strict-mcp-config --mcp-config {"mcpServers":{}} --session-id 0b8e…"#);
        let resume = line(&Conversation::Resume {
            id: "abc".into(),
            fork: true,
        });
        assert!(resume.ends_with("--setting-sources user --strict-mcp-config --mcp-config {\"mcpServers\":{}} --resume abc --fork-session"));
        assert!(!resume.contains("--bare"));
    }
}
