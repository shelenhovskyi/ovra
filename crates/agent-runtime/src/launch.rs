//! The command line `claude` is started with.

use std::ffi::OsString;

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
pub(crate) fn args(conversation: &Conversation, trusted: bool) -> Vec<OsString> {
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
        // `.claude/` settings, hooks, agents and skills load only once the
        // user trusts the project in Ovra.
        "--setting-sources",
        if trusted {
            "user,project,local"
        } else {
            "user"
        },
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

    fn line(conversation: &Conversation, trusted: bool) -> String {
        args(conversation, trusted)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_command_line_keeps_the_safe_defaults() {
        let new = line(&Conversation::New("0b8e…".into()), false);
        insta::assert_snapshot!(new, @r#"-p --input-format stream-json --output-format stream-json --verbose --permission-prompt-tool stdio --permission-mode default --setting-sources user --strict-mcp-config --mcp-config {"mcpServers":{}} --session-id 0b8e…"#);
        let resume = line(
            &Conversation::Resume {
                id: "abc".into(),
                fork: true,
            },
            false,
        );
        assert!(resume.ends_with("--setting-sources user --strict-mcp-config --mcp-config {\"mcpServers\":{}} --resume abc --fork-session"));
        assert!(!resume.contains("--bare"));
    }

    #[test]
    fn only_a_trusted_project_loads_its_own_settings() {
        let trusted = line(&Conversation::New("id".into()), true);
        assert!(trusted.contains("--setting-sources user,project,local --strict-mcp-config"));
        assert!(trusted.contains("--permission-prompt-tool stdio --permission-mode default"));
    }
}
