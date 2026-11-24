//! The command line `claude` is started with.

use std::ffi::OsString;
use std::path::Path;

use tokio::process::Command;

/// Variables a running Claude Code session sets for the processes it starts.
/// When Ovra itself runs inside such a session (an IDE terminal, a Claude
/// Code tool call), they would make our `claude` think it is nested in that
/// session or attach to its IDE, so they are removed. Exact names only:
/// everything else, including the user's configuration (`ANTHROPIC_*`,
/// `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_VERTEX`, proxies), is kept.
pub(crate) const NESTED_SESSION_VARS: [&str; 9] = [
    // Set to 1 inside a session; the CLI treats its children as nested.
    "CLAUDECODE",
    // How the parent was started (`cli`, `sdk-ts`, ...).
    "CLAUDE_CODE_ENTRYPOINT",
    // The parent's IDE connection.
    "CLAUDE_CODE_SSE_PORT",
    "ENABLE_IDE_INTEGRATION",
    // The parent session's identity and process.
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_PID",
    "CLAUDE_AGENT_SDK_VERSION",
];

/// A command for `claude` with the user's environment minus
/// [`NESTED_SESSION_VARS`].
pub(crate) fn command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    for name in NESTED_SESSION_VARS {
        command.env_remove(name);
    }
    command
}

use ovra_core::services::{AgentSpec, ToolRole};

/// Which Claude Code session the process works in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Conversation {
    /// A new session with an id Ovra chose.
    New(String),
    /// Continues an earlier session; `fork` branches it into a new one.
    Resume { id: String, fork: bool },
}

impl Conversation {
    /// Continues `spec.resume` (keeping its id), or starts anew.
    pub(crate) fn for_spec(spec: &AgentSpec) -> Self {
        match spec.resume.clone() {
            Some(id) => Self::Resume { id, fork: false },
            None => Self::New(uuid::Uuid::new_v4().to_string()),
        }
    }

    /// The Claude session id, for a resume without a fork the one continued.
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::New(id) | Self::Resume { id, .. } => id,
        }
    }
}

/// The arguments for one session. Ovra never passes `--bare` (it ignores the
/// subscription login) and never touches `CLAUDE_CONFIG_DIR` or credentials;
/// the environment is inherited except for [`NESTED_SESSION_VARS`] (ADR 0006).
/// `ide` passes Ovra's in-process `ovra` MCP server (its IDE tools), whose
/// messages come back as `mcp_message` control requests. A coordinator only
/// reads the project and directs workers: it gets no built-in tool that
/// writes or runs anything (owner decision, 2026-11-24).
pub(crate) fn args(
    conversation: &Conversation,
    trusted: bool,
    ide: Option<ToolRole>,
) -> Vec<OsString> {
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
        // Only the MCP servers Ovra passes.
        "--strict-mcp-config",
        "--mcp-config",
        if ide.is_some() {
            r#"{"mcpServers":{"ovra":{"type":"sdk","name":"ovra"}}}"#
        } else {
            r#"{"mcpServers":{}}"#
        },
    ];
    if ide == Some(ToolRole::Coordinator) {
        args.extend(["--tools", "Read,Grep,Glob"]);
    }
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
    fn nested_session_markers_are_removed_and_configuration_is_kept() {
        let command = command(Path::new("claude"));
        let envs: Vec<_> = command.as_std().get_envs().collect();
        let removed: Vec<_> = envs
            .iter()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();
        assert_eq!(removed.len(), NESTED_SESSION_VARS.len());
        for name in NESTED_SESSION_VARS {
            assert!(removed.iter().any(|r| r == name), "{name}");
        }
        assert!(envs.iter().all(|(_, value)| value.is_none()));
        assert!(
            !removed
                .iter()
                .any(|r| r.contains("BEDROCK") || r.starts_with("ANTHROPIC"))
        );
    }

    fn line(conversation: &Conversation, trusted: bool) -> String {
        args(conversation, trusted, None)
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
    fn sessions_with_ide_tools_get_the_in_process_ovra_server() {
        let after = |role, flag: &str| {
            let args = args(&Conversation::New("id".into()), false, Some(role));
            let args: Vec<_> = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            let at = args.iter().position(|a| a == flag)?;
            args.get(at + 1).cloned()
        };
        let ovra = r#"{"mcpServers":{"ovra":{"type":"sdk","name":"ovra"}}}"#;
        assert_eq!(
            after(ToolRole::Worker, "--mcp-config").as_deref(),
            Some(ovra)
        );
        assert_eq!(
            after(ToolRole::Coordinator, "--mcp-config").as_deref(),
            Some(ovra)
        );
        assert_eq!(after(ToolRole::Worker, "--tools"), None);
        assert_eq!(
            after(ToolRole::Coordinator, "--tools").as_deref(),
            Some("Read,Grep,Glob")
        );
    }

    #[test]
    fn only_a_trusted_project_loads_its_own_settings() {
        let trusted = line(&Conversation::New("id".into()), true);
        assert!(trusted.contains("--setting-sources user,project,local --strict-mcp-config"));
        assert!(trusted.contains("--permission-prompt-tool stdio --permission-mode default"));
    }

    #[test]
    fn a_resumed_session_keeps_its_id() {
        let mut spec = AgentSpec {
            session: ovra_core::protocol::agents::SessionId("s1".into()),
            agent: ovra_core::protocol::agents::AgentKind::Claude,
            mode: ovra_core::protocol::agents::SessionMode::Direct,
            cwd: "/work".into(),
            tools: None,
            prompt: "go".into(),
            executable: None,
            trusted: false,
            resume: None,
        };
        let new = Conversation::for_spec(&spec);
        assert!(
            matches!(&new, Conversation::New(id) if id.len() == 36),
            "{new:?}"
        );
        spec.resume = Some("abc".into());
        let resumed = Conversation::for_spec(&spec);
        assert_eq!(
            resumed,
            Conversation::Resume {
                id: "abc".into(),
                fork: false
            }
        );
        assert_eq!(resumed.id(), "abc");
    }
}
