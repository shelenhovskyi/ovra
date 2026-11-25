//! The command line `claude` is started with.

use std::ffi::OsString;
use std::path::Path;

use tokio::process::Command;

/// Variables a running Claude Code session sets for the processes it starts.
/// When Ovra itself runs inside such a session (an IDE terminal, a Claude
/// Code tool call), they would make our `claude` think it is nested in that
/// session or attach to its IDE, so they are removed. Exact names only; the
/// sandbox, or without one [`crate::env::unsandboxed`], then keeps only an
/// allowlist of the rest.
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

/// Where `claude` keeps its settings, login and transcripts. Set only for
/// an account the user added with its own folder (ADR 0006).
pub(crate) const CONFIG_DIR_VAR: &str = "CLAUDE_CONFIG_DIR";

/// A command for `claude` (its version, its sign-in) with the environment a
/// session gets, so a check sees what a session will: the allowlist of
/// [`crate::env`], without [`NESTED_SESSION_VARS`], in the account's folder
/// `config_dir` if given.
pub(crate) fn command(executable: &Path, config_dir: Option<&Path>) -> Command {
    let mut command = Command::new(executable);
    let outer = std::env::vars_os().map(|(name, _)| name);
    for name in crate::env::removed(outer, cfg!(windows)) {
        command.env_remove(name);
    }
    if let Some(path) = ovra_core::child_path::login() {
        command.env("PATH", path);
    }
    for name in NESTED_SESSION_VARS {
        command.env_remove(name);
    }
    if let Some(dir) = config_dir {
        command.env(CONFIG_DIR_VAR, dir);
    }
    command
}

/// The session's command line before a sandbox wraps it: `args` for
/// `executable` in `cwd`, minus [`NESTED_SESSION_VARS`], in the account's
/// folder `config_dir` if given.
pub(crate) fn line(
    executable: &Path,
    args: Vec<OsString>,
    cwd: &Path,
    config_dir: Option<&Path>,
) -> CommandLine {
    let mut env: Vec<(OsString, Option<OsString>)> = NESTED_SESSION_VARS
        .iter()
        .map(|name| (OsString::from(name), None))
        .collect();
    if let Some(dir) = config_dir {
        env.push((CONFIG_DIR_VAR.into(), Some(dir.as_os_str().to_owned())));
    }
    CommandLine {
        program: executable.to_path_buf(),
        args,
        env,
        cwd: cwd.to_path_buf(),
    }
}

/// The process a (possibly wrapped) command line starts.
pub(crate) fn spawnable(line: &CommandLine) -> Command {
    let mut command = Command::new(&line.program);
    command.args(&line.args).current_dir(&line.cwd);
    // The login shell's `PATH`, then the line's: a sandbox's own wins.
    for (name, value) in ovra_core::child_path::env(&line.env) {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    command
}

use ovra_core::services::{AgentSpec, CommandLine, ToolRole};

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

/// What a coordinator and a worker are told about their role, after Claude
/// Code's own system prompt.
const COORDINATOR_PROMPT: &str = include_str!("../../../agents/roles/coordinator.md");
const WORKER_PROMPT: &str = include_str!("../../../agents/roles/worker.md");
const RESEARCH_PROMPT: &str = include_str!("../../../agents/roles/research.md");

/// The built-in tools of a research session (ADR 0011): it reads and
/// searches, nothing else. The core refuses any other tool too.
const RESEARCH_TOOLS: &str = "Read,Grep,Glob,WebSearch";

/// The arguments for one session. Ovra never passes `--bare` (it ignores the
/// subscription login) and never touches credentials; the environment is an
/// allowlist without [`NESTED_SESSION_VARS`], and `CLAUDE_CONFIG_DIR` is
/// set only to the folder of an account the user added (ADR 0006).
///
/// `ide` passes Ovra's in-process `ovra` MCP server (its IDE tools), whose
/// messages come back as `mcp_message` control requests. A coordinator only
/// reads the project and directs workers: it gets no built-in tool that
/// writes or runs anything (owner decision, 2026-11-24). A `research`
/// session gets [`RESEARCH_TOOLS`] and no IDE tools.
pub(crate) fn args(
    conversation: &Conversation,
    trusted: bool,
    ide: Option<ToolRole>,
    research: bool,
) -> Vec<OsString> {
    let ide = ide.filter(|_| !research);
    let mut args: Vec<&str> = vec![
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        // Text deltas as `stream_event` lines, so the UI shows text as it
        // is written (every version the range in `detect.rs` allows has it).
        "--include-partial-messages",
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
        if trusted && !research {
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
    match ide {
        Some(ToolRole::Coordinator) => args.extend([
            "--tools",
            "Read,Grep,Glob",
            "--append-system-prompt",
            COORDINATOR_PROMPT,
        ]),
        Some(ToolRole::Worker) => args.extend(["--append-system-prompt", WORKER_PROMPT]),
        None if research => args.extend([
            "--tools",
            RESEARCH_TOOLS,
            "--append-system-prompt",
            RESEARCH_PROMPT,
        ]),
        None => {}
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
    fn claude_gets_the_login_shells_path_before_a_sandboxs_own() {
        // Each test runs in its own process (nextest), so this stays here.
        ovra_core::child_path::set_login("/opt/homebrew/bin:/usr/bin".into());
        let path = |command: &Command| {
            command
                .as_std()
                .get_envs()
                .filter(|(name, _)| *name == "PATH")
                .map(|(_, value)| value.map(std::ffi::OsStr::to_owned))
                .collect::<Vec<_>>()
        };
        let login = Some(OsString::from("/opt/homebrew/bin:/usr/bin"));
        assert_eq!(
            path(&command(Path::new("claude"), None)),
            std::slice::from_ref(&login)
        );
        let mut line = line(Path::new("claude"), Vec::new(), Path::new("/"), None);
        assert_eq!(path(&spawnable(&line)), [login]);
        line.env.push(("PATH".into(), Some("/usr/bin".into())));
        assert_eq!(path(&spawnable(&line)), [Some("/usr/bin".into())]);
    }

    #[test]
    fn a_check_gets_the_sessions_environment_without_nested_session_markers() {
        let command = command(Path::new("claude"), None);
        let envs: Vec<_> = command.as_std().get_envs().collect();
        let removed: Vec<_> = envs
            .iter()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();
        for name in NESTED_SESSION_VARS {
            assert!(removed.iter().any(|r| r == name), "{name}");
        }
        assert!(envs.iter().all(|(_, value)| value.is_none()));
        let configuration = [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "CLAUDE_CODE_USE_BEDROCK",
        ];
        assert!(!removed.iter().any(|r| configuration.contains(&r.as_str())));
        // Ovra's own environment beyond the allowlist is removed, as for a session.
        let outer: Vec<String> = std::env::vars_os()
            .filter_map(|(name, _)| name.into_string().ok())
            .collect();
        for name in crate::env::removed(outer.iter().map(OsString::from), cfg!(windows)) {
            let name = name.to_string_lossy().into_owned();
            assert!(removed.contains(&name), "{name}");
        }
    }

    #[test]
    fn a_wrapped_line_still_removes_the_nested_session_markers() {
        let mut wrapped = line(
            Path::new("claude"),
            vec!["-p".into()],
            Path::new("/w"),
            None,
        );
        wrapped.program = "sandbox-run".into();
        wrapped.args.insert(0, "claude".into());
        wrapped
            .env
            .push(("HTTPS_PROXY".into(), Some("http://127.0.0.1:9".into())));
        let command = spawnable(&wrapped);
        let std = command.as_std();
        assert_eq!(std.get_program(), "sandbox-run");
        assert_eq!(std.get_args().collect::<Vec<_>>(), ["claude", "-p"]);
        assert_eq!(std.get_current_dir(), Some(Path::new("/w")));
        let envs: Vec<_> = std.get_envs().collect();
        for name in NESTED_SESSION_VARS {
            assert!(envs.contains(&(name.as_ref(), None)), "{name}");
        }
        assert!(envs.contains(&("HTTPS_PROXY".as_ref(), Some("http://127.0.0.1:9".as_ref()))));
    }

    #[test]
    fn an_accounts_folder_is_given_to_claude_and_nothing_else_changes() {
        let dir = Path::new("/home/me/.claude-work");
        let envs = |command: &Command| -> Vec<(OsString, Option<OsString>)> {
            command
                .as_std()
                .get_envs()
                .map(|(n, v)| (n.to_owned(), v.map(ToOwned::to_owned)))
                .collect()
        };
        let set = (
            OsString::from(CONFIG_DIR_VAR),
            Some(dir.as_os_str().to_owned()),
        );
        assert!(envs(&command(Path::new("claude"), Some(dir))).contains(&set));
        assert!(
            !envs(&command(Path::new("claude"), None))
                .iter()
                .any(|(n, _)| n == CONFIG_DIR_VAR)
        );
        let spawned = spawnable(&line(
            Path::new("claude"),
            Vec::new(),
            Path::new("/w"),
            Some(dir),
        ));
        let all = envs(&spawned);
        assert!(all.contains(&set));
        assert_eq!(all.len(), NESTED_SESSION_VARS.len() + 1);
        let default = line(Path::new("claude"), Vec::new(), Path::new("/w"), None);
        assert!(!default.env.iter().any(|(n, _)| n == CONFIG_DIR_VAR));
    }

    fn line_text(conversation: &Conversation, trusted: bool) -> String {
        args(conversation, trusted, None, false)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_command_line_keeps_the_safe_defaults() {
        let new = line_text(&Conversation::New("0b8e…".into()), false);
        insta::assert_snapshot!(new, @r#"-p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --permission-prompt-tool stdio --permission-mode default --setting-sources user --strict-mcp-config --mcp-config {"mcpServers":{}} --session-id 0b8e…"#);
        let resume = line_text(
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
            let args = args(&Conversation::New("id".into()), false, Some(role), false);
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
            after(ToolRole::Worker, "--append-system-prompt").as_deref(),
            Some(WORKER_PROMPT)
        );
        assert_eq!(
            after(ToolRole::Coordinator, "--append-system-prompt").as_deref(),
            Some(COORDINATOR_PROMPT)
        );
        assert_eq!(
            after(ToolRole::Coordinator, "--tools").as_deref(),
            Some("Read,Grep,Glob")
        );
    }

    #[test]
    fn a_research_session_only_reads_and_searches() {
        let args: Vec<String> = args(
            &Conversation::New("id".into()),
            false,
            Some(ToolRole::Worker),
            true,
        )
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
        let after = |flag: &str| {
            let at = args.iter().position(|a| a == flag)?;
            args.get(at + 1).map(String::as_str)
        };
        assert_eq!(after("--tools"), Some("Read,Grep,Glob,WebSearch"));
        assert_eq!(after("--append-system-prompt"), Some(RESEARCH_PROMPT));
        assert_eq!(after("--mcp-config"), Some(r#"{"mcpServers":{}}"#));
        assert_eq!(after("--permission-mode"), Some("default"));
        // A project's hooks could change files unseen: research has no review.
        let trusted: Vec<String> = super::args(&Conversation::New("id".into()), true, None, true)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let at = trusted
            .iter()
            .position(|a| a == "--setting-sources")
            .unwrap_or(0);
        assert_eq!(trusted.get(at + 1).map(String::as_str), Some("user"));
    }

    #[test]
    fn only_a_trusted_project_loads_its_own_settings() {
        let trusted = line_text(&Conversation::New("id".into()), true);
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
            config_dir: None,
            trusted: false,
            resume: None,
            sandbox: ovra_core::services::SandboxProfile {
                kind: ovra_core::services::ProfileKind::Agent,
                claude_dir: None,
                project: None,
                work_env: None,
                work_dir: "/work".into(),
                work_dir_read_only: false,
                read: Vec::new(),
                write: Vec::new(),
                expose: Vec::new(),
                forward: Vec::new(),
                network: ovra_core::services::ProfileNetwork::Open,
            },
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
