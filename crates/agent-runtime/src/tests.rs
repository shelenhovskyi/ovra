//! Contract tests: recorded sessions (`fixtures/`) replayed against the
//! driver through the core, plus the process seam with a stand-in `claude`.

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use ovra_core::Core;
use ovra_core::fakes::MemoryProjects;
use ovra_core::protocol::agents::{AgentMessage, AgentStatus, AgentsEvent, AgentsResponse};
use ovra_core::protocol::{Event, Request, Response};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use super::*;
use ovra_core::services::{BoxFuture, IdeTools, ToolCaller, ToolRole};

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

async fn call(core: &Core, request: Value) -> Response {
    let request: Request = serde_json::from_value(request).unwrap_or_else(|e| panic!("{e}"));
    core.handle(request)
        .await
        .unwrap_or_else(|e| panic!("request failed: {e}"))
}

/// Whether `actual` has every field of `expected`, recursively.
fn contains(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(a), Value::Object(e)) => e
            .iter()
            .all(|(k, v)| a.get(k).is_some_and(|a| contains(a, v))),
        _ => actual == expected,
    }
}

async fn wait_for(
    rx: &mut broadcast::Receiver<Event>,
    expected: &Value,
) -> std::result::Result<(), String> {
    loop {
        let event = Core::next_event(rx)
            .await
            .ok_or_else(|| "the core closed".to_owned())?;
        let event = serde_json::to_value(&event).map_err(|e| e.to_string())?;
        if contains(&event, expected) {
            return Ok(());
        }
    }
}

/// Plays the CLI side of a recorded session.
async fn fake_cli(
    steps: Vec<Value>,
    mut stdout: DuplexStream,
    stdin: DuplexStream,
    core: Core,
    mut events: broadcast::Receiver<Event>,
) -> std::result::Result<(), String> {
    let mut stdin = BufReader::new(stdin).lines();
    for (n, step) in steps.into_iter().enumerate() {
        let fail = |what: String| format!("step {}: {what}", n + 1);
        if let Some(line) = step.get("cli") {
            let mut bytes = line.to_string().into_bytes();
            bytes.push(b'\n');
            stdout
                .write_all(&bytes)
                .await
                .map_err(|e| fail(e.to_string()))?;
        } else if let Some(expected) = step.get("ovra") {
            let line = stdin.next_line().await.map_err(|e| fail(e.to_string()))?;
            let line = line.ok_or_else(|| fail("stdin closed".to_owned()))?;
            let actual: Value = serde_json::from_str(&line).map_err(|e| fail(e.to_string()))?;
            if &actual != expected {
                return Err(fail(format!("Ovra wrote {actual}, expected {expected}")));
            }
        } else if let Some(expected) = step.get("wait") {
            wait_for(&mut events, expected).await.map_err(fail)?;
        } else if let Some(request) = step.get("core") {
            call(&core, request.clone()).await;
        } else if step.get("hang").is_some() {
            return std::future::pending().await;
        } else {
            return Err(fail(format!("unknown step {step}")));
        }
    }
    match stdin.next_line().await {
        Ok(None) => Ok(()),
        Ok(Some(line)) => Err(format!("Ovra wrote {line} after the last step")),
        Err(e) => Err(e.to_string()),
    }
}

/// A runtime that connects the driver to [`fake_cli`] instead of a process.
#[derive(Debug, Default)]
struct Replay {
    steps: Vec<Value>,
    core: OnceLock<Core>,
    cli: Mutex<Option<JoinHandle<std::result::Result<(), String>>>>,
    ended: Arc<Mutex<Vec<Ended>>>,
    /// The IDE tools role the session gets, whatever the core asked for.
    ide: Option<ToolRole>,
}

impl AgentRuntime for Replay {
    fn start(&self, spec: AgentSpec, host: AgentHost) -> Result<Box<dyn RunningAgent>> {
        let core = self
            .core
            .get()
            .cloned()
            .unwrap_or_else(|| panic!("no core"));
        let (cli_stdout, ovra_stdout) = tokio::io::duplex(1 << 16);
        let (ovra_stdin, cli_stdin) = tokio::io::duplex(1 << 16);
        let events = core.subscribe();
        let (commands, rx) = mpsc::unbounded_channel();
        let cli = tokio::spawn(fake_cli(
            self.steps.clone(),
            cli_stdout,
            cli_stdin,
            core,
            events,
        ));
        *lock(&self.cli) = Some(cli);
        let ended = self.ended.clone();
        let ide = self.ide.or(spec.tools);
        tokio::spawn(async move {
            let end = driver::run(
                BufReader::new(ovra_stdout),
                ovra_stdin,
                spec.cwd,
                host.clone(),
                ide,
                spec.prompt,
                rx,
            )
            .await;
            let status = match &end {
                Ended::Failed(message) => AgentStatus::Failed {
                    message: message.clone(),
                },
                Ended::Closed { .. } | Ended::Unresponsive => AgentStatus::Stopped,
            };
            lock(&ended).push(end);
            host.status(status);
        });
        Ok(Box::new(ClaudeProcess(commands)))
    }
}

fn steps(fixture: &str) -> Vec<Value> {
    fixture
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}

/// The project root of replayed sessions; the fixtures' paths start with `/w`.
const ROOT: &str = if cfg!(windows) { r"C:\w" } else { "/w" };

fn core_with(runtime: Arc<dyn AgentRuntime>) -> Core {
    Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(runtime)
        .build()
}

async fn start(core: &Core, root: &str, prompt: &str) {
    call(
        core,
        json!({"method": "projects.add", "params": {"root": root}}),
    )
    .await;
    call(
        core,
        json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": prompt}}),
    )
    .await;
}

/// Waits until session `s1` ends and returns its final state.
async fn end(rx: &mut broadcast::Receiver<Event>) -> AgentStatus {
    loop {
        if let Some(Event::Agents(AgentsEvent::Status { status, .. })) = Core::next_event(rx).await
            && status.is_ended()
        {
            return status;
        }
    }
}

async fn transcript(core: &Core) -> Vec<AgentMessage> {
    match call(
        core,
        json!({"method": "agents.transcript", "params": {"session": "s1"}}),
    )
    .await
    {
        Response::Agents(AgentsResponse::Transcript(t)) => t,
        other => panic!("unexpected {other:?}"),
    }
}

/// The prompt a recorded session starts with: Ovra's first user line.
fn prompt(steps: &[Value]) -> String {
    steps
        .iter()
        .filter_map(|step| step.get("ovra"))
        .find(|line| line["type"] == "user")
        .and_then(|line| line.pointer("/message/content"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("the fixture sends no prompt"))
        .to_owned()
}

/// Replays a fixture to its end and returns the session's transcript.
async fn replay(fixture: &str) -> (Vec<AgentMessage>, Vec<Ended>) {
    let steps = steps(fixture);
    let prompt = prompt(&steps);
    let runtime = Arc::new(Replay {
        steps,
        ..Replay::default()
    });
    let core = core_with(runtime.clone());
    let _ = runtime.core.set(core.clone());
    let mut rx = core.subscribe();
    start(&core, ROOT, &prompt).await;
    let cli = lock(&runtime.cli).take();
    match cli {
        Some(cli) => cli
            .await
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|e| panic!("{e}")),
        None => panic!("the session did not start"),
    }
    assert_eq!(end(&mut rx).await, AgentStatus::Stopped);
    let ended = std::mem::take(&mut *lock(&runtime.ended));
    (transcript(&core).await, ended)
}

// Sessions recorded from the real CLI by the live suite (tests/agents).

const EDIT_ALLOWED: &str = include_str!("../fixtures/edit_allowed.jsonl");
const DENIED_OUTSIDE_FOLDER: &str = include_str!("../fixtures/denied_outside_folder.jsonl");
const INTERRUPTED: &str = include_str!("../fixtures/interrupted.jsonl");
const SUBAGENT: &str = include_str!("../fixtures/subagent.jsonl");

#[test]
fn recorded_sessions_hold_no_local_paths_or_keys() {
    for fixture in [EDIT_ALLOWED, DENIED_OUTSIDE_FOLDER, INTERRUPTED, SUBAGENT] {
        for leak in ["/root/", "/Users/", "/tmp/", "/private/", "sk-ant-"] {
            let found = fixture.lines().find(|line| line.contains(leak));
            assert!(found.is_none(), "{leak} in {found:?}");
        }
    }
}

#[tokio::test]
async fn an_allowed_edit_and_an_allowed_command_over_two_turns() {
    let (transcript, ended) = replay(EDIT_ALLOWED).await;
    assert_eq!(ended, [Ended::Closed { stopped: true }]);
    insta::assert_json_snapshot!(transcript);
}

#[tokio::test]
async fn a_write_outside_the_folder_is_asked_about_and_denied_with_a_message() {
    let steps = steps(DENIED_OUTSIDE_FOLDER);
    let hook_asks = steps.iter().filter_map(|s| s.get("ovra")).any(|line| {
        line.pointer("/response/response/hookSpecificOutput/permissionDecision")
            == Some(&json!("ask"))
    });
    assert!(hook_asks, "the policy hook did not ask");
    let (transcript, ended) = replay(DENIED_OUTSIDE_FOLDER).await;
    assert_eq!(ended, [Ended::Closed { stopped: true }]);
    insta::assert_json_snapshot!(transcript);
}

#[tokio::test]
async fn an_interrupt_ends_the_turn_and_keeps_the_session() {
    let (transcript, ended) = replay(INTERRUPTED).await;
    assert_eq!(ended, [Ended::Closed { stopped: true }]);
    let turns = transcript
        .iter()
        .filter(|m| matches!(m, AgentMessage::TurnEnd { .. }))
        .count();
    assert_eq!(turns, 2);
    insta::assert_json_snapshot!(transcript);
}

#[tokio::test]
async fn a_subagents_messages_name_their_parent() {
    let (transcript, ended) = replay(SUBAGENT).await;
    assert_eq!(ended, [Ended::Closed { stopped: true }]);
    let parented = transcript.iter().any(|m| {
        matches!(
            m,
            AgentMessage::ToolUse {
                parent: Some(_),
                ..
            } | AgentMessage::ToolResult {
                parent: Some(_),
                ..
            }
        )
    });
    assert!(parented, "no message names its parent");
    insta::assert_json_snapshot!(transcript);
}

const INIT: &str = r#"{"ovra":{"type":"control_request","request_id":"init","request":{"subtype":"initialize","hooks":{"PreToolUse":[{"matcher":null,"hookCallbackIds":["ovra_policy"]}]}}}}
{"cli":{"type":"control_response","response":{"subtype":"success","request_id":"init","response":{}}}}"#;

#[tokio::test]
async fn a_failed_initialize_fails_the_session() {
    let runtime = Arc::new(Replay {
        steps: steps(concat!(
            r#"{"ovra":{"type":"control_request","request_id":"init","request":{"subtype":"initialize","hooks":{"PreToolUse":[{"matcher":null,"hookCallbackIds":["ovra_policy"]}]}}}}"#,
            "\n",
            r#"{"cli":{"type":"control_response","response":{"subtype":"error","request_id":"init","error":"bad hooks"}}}"#,
        )),
        ..Replay::default()
    });
    let core = core_with(runtime.clone());
    let _ = runtime.core.set(core.clone());
    let mut rx = core.subscribe();
    start(&core, ROOT, "hi").await;
    assert_eq!(
        end(&mut rx).await,
        AgentStatus::Failed {
            message: "claude did not start the session: bad hooks".into()
        }
    );
}

/// The hook is the only place a call the user's settings already allow
/// meets the project's policy.
#[tokio::test]
async fn the_hook_refuses_what_the_projects_policy_denies() {
    let runtime = Arc::new(Replay {
        steps: steps(&format!(
            "{INIT}\n{}",
            concat!(
                r#"{"ovra":{"type":"user","message":{"role":"user","content":"hi"}}}"#,
                "\n",
                r#"{"core":{"method":"policy.set","params":{"project":"p1","policy":{"rules":[{"tool":"WebFetch","action":"deny"}]}}}}"#,
                "\n",
                r#"{"cli":{"type":"control_request","request_id":"h1","request":{"subtype":"hook_callback","callback_id":"ovra_policy","input":{"hook_event_name":"PreToolUse","tool_name":"WebFetch","tool_input":{"url":"https://example.com"},"tool_use_id":"toolu_01"}}}}"#,
                "\n",
                r#"{"ovra":{"response":{"request_id":"h1","response":{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"The project's policy refuses this (WebFetch). Ask the person if it is needed."}},"subtype":"success"},"type":"control_response"}}"#,
                "\n",
                r#"{"core":{"method":"agents.stop","params":{"session":"s1"}}}"#,
                "\n",
                r#"{"ovra":{"request":{"subtype":"interrupt"},"request_id":"interrupt1","type":"control_request"}}"#,
                "\n",
                r#"{"cli":{"type":"control_response","response":{"subtype":"success","request_id":"interrupt1","response":{"still_queued":[]}}}}"#,
            )
        )),
        ..Replay::default()
    });
    let core = core_with(runtime.clone());
    let _ = runtime.core.set(core.clone());
    let mut rx = core.subscribe();
    start(&core, ROOT, "hi").await;
    let cli = lock(&runtime.cli).take();
    match cli {
        Some(cli) => cli
            .await
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|e| panic!("{e}")),
        None => panic!("the session did not start"),
    }
    assert_eq!(end(&mut rx).await, AgentStatus::Stopped);
}

/// An ask rule holds for calls the user's settings allow: the hook has the
/// CLI ask, which then asks Ovra through `can_use_tool`.
#[tokio::test]
async fn the_hook_asks_about_what_the_projects_policy_asks_about() {
    let runtime = Arc::new(Replay {
        steps: steps(&format!(
            "{INIT}\n{}",
            concat!(
                r#"{"ovra":{"type":"user","message":{"role":"user","content":"hi"}}}"#,
                "\n",
                r#"{"core":{"method":"policy.set","params":{"project":"p1","policy":{"rules":[{"tool":"Bash","match":"git push","action":"ask"},{"tool":"Bash","match":"git push","action":"auto"}]}}}}"#,
                "\n",
                r#"{"cli":{"type":"control_request","request_id":"h1","request":{"subtype":"hook_callback","callback_id":"ovra_policy","input":{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"git push"},"tool_use_id":"toolu_01"}}}}"#,
                "\n",
                r#"{"ovra":{"response":{"request_id":"h1","response":{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"The project's policy asks the person first (Bash(git push))."}},"subtype":"success"},"type":"control_response"}}"#,
                "\n",
                r#"{"core":{"method":"agents.stop","params":{"session":"s1"}}}"#,
                "\n",
                r#"{"ovra":{"request":{"subtype":"interrupt"},"request_id":"interrupt1","type":"control_request"}}"#,
                "\n",
                r#"{"cli":{"type":"control_response","response":{"subtype":"success","request_id":"interrupt1","response":{"still_queued":[]}}}}"#,
            )
        )),
        ..Replay::default()
    });
    let core = core_with(runtime.clone());
    let _ = runtime.core.set(core.clone());
    let mut rx = core.subscribe();
    start(&core, ROOT, "hi").await;
    let cli = lock(&runtime.cli).take();
    match cli {
        Some(cli) => cli
            .await
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|e| panic!("{e}")),
        None => panic!("the session did not start"),
    }
    assert_eq!(end(&mut rx).await, AgentStatus::Stopped);
}

#[tokio::test(start_paused = true)]
async fn a_process_that_ignores_a_stop_is_given_up_on() {
    let runtime = Arc::new(Replay {
        steps: steps(&format!(
            "{INIT}\n{}",
            concat!(
                r#"{"ovra":{"type":"user","message":{"role":"user","content":"hi"}}}"#,
                "\n",
                r#"{"core":{"method":"agents.stop","params":{"session":"s1"}}}"#,
                "\n",
                r#"{"hang":true}"#,
            )
        )),
        ..Replay::default()
    });
    let core = core_with(runtime.clone());
    let _ = runtime.core.set(core.clone());
    let mut rx = core.subscribe();
    let started = tokio::time::Instant::now();
    start(&core, ROOT, "hi").await;
    assert_eq!(end(&mut rx).await, AgentStatus::Stopped);
    assert!(started.elapsed() >= driver::STOP_GRACE);
    assert_eq!(*lock(&runtime.ended), [Ended::Unresponsive]);
}

/// A stand-in `claude`: a shell script, so the real process seam is tested
/// without the real CLI.
#[cfg(unix)]
mod process {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("ovra-agent-runtime-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
            Self(dir)
        }

        /// Writes an executable `claude` that prints `version` and runs `body`.
        fn claude(&self, version: &str, body: &str) -> PathBuf {
            let path = self.0.join("claude");
            let script = format!(
                "#!/bin/sh\nif [ \"$1\" = --version ]; then echo '{version}'; exit 0; fi\n\
                 if [ \"$1\" = auth ]; then cat \"$0.auth\" 2>/dev/null; exit 0; fi\n\
                 pwd > \"$0.cwd\"; echo \"$@\" > \"$0.args\"\n{body}\n"
            );
            std::fs::write(&path, script).unwrap_or_else(|e| panic!("{e}"));
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .unwrap_or_else(|e| panic!("{e}"));
            path
        }

        fn read(&self, name: &str) -> String {
            std::fs::read_to_string(self.0.join(name)).unwrap_or_else(|e| panic!("{e}"))
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn wait_idle(rx: &mut broadcast::Receiver<Event>) {
        loop {
            if let Some(Event::Agents(AgentsEvent::Status {
                status: AgentStatus::Idle,
                ..
            })) = Core::next_event(rx).await
            {
                return;
            }
        }
    }

    const ONE_TURN: &str = r#"read init
echo '{"type":"control_response","response":{"subtype":"success","request_id":"init","response":{}}}'
read prompt
echo '{"type":"system","subtype":"init","session_id":"x"}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"hello from sh"}]}}'
echo '{"type":"result","subtype":"success","usage":{"input_tokens":1,"output_tokens":2}}'"#;

    async fn run(dir: &Dir, claude: &Path) -> (Core, broadcast::Receiver<Event>) {
        let core = core_with(Arc::new(ClaudeCli::new(
            Some(claude.to_owned()),
            unsandboxed(),
        )));
        let rx = core.subscribe();
        start(&core, &dir.0.to_string_lossy(), "hi").await;
        (core, rx)
    }

    #[tokio::test]
    async fn a_session_runs_the_executable_with_the_safe_flags() {
        let dir = Dir::new("ok");
        let claude = dir.claude(
            "2.1.331 (Claude Code)",
            &format!("{ONE_TURN}\ncat > /dev/null"),
        );
        let (core, mut rx) = run(&dir, &claude).await;
        wait_idle(&mut rx).await;
        let texts: Vec<_> = transcript(&core).await;
        assert!(texts.contains(&AgentMessage::AssistantText {
            text: "hello from sh".into(),
            parent: None
        }));
        assert_eq!(sessions(&core).await[0].sandbox, None);
        call(
            &core,
            json!({"method": "agents.stop", "params": {"session": "s1"}}),
        )
        .await;
        assert_eq!(end(&mut rx).await, AgentStatus::Stopped);
        let args = dir.read("claude.args");
        assert!(args.starts_with("-p --input-format stream-json"), "{args}");
        assert!(args.contains("--setting-sources user --strict-mcp-config"));
        assert!(!args.contains("--bare"));
        let cwd = PathBuf::from(dir.read("claude.cwd").trim());
        assert_eq!(cwd.canonicalize().ok(), dir.0.canonicalize().ok());
    }

    async fn setup(core: &Core) -> AgentSetup {
        match call(
            core,
            json!({"method": "agents.setup", "params": {"agent": "claude"}}),
        )
        .await
        {
            Response::Agents(AgentsResponse::Setup(setup)) => setup,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn setup_reports_the_version_and_the_sign_in_state() {
        use ovra_core::protocol::agents::SetupState;
        let dir = Dir::new("setup");
        let claude = dir.claude("2.1.400 (Claude Code)", "exit 0");
        let core = core_with(Arc::new(ClaudeCli::new(
            Some(claude.clone()),
            unsandboxed(),
        )));
        let ready = setup(&core).await;
        assert_eq!(ready.state, SetupState::Ready);
        assert_eq!(ready.version.as_deref(), Some("2.1.400"));
        assert_eq!(
            ready.executable,
            Some(claude.to_string_lossy().into_owned())
        );
        std::fs::write(dir.0.join("claude.auth"), r#"{"loggedIn": false}"#)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(setup(&core).await.state, SetupState::SignedOut);
        let old = dir.claude("2.0.1", "exit 0");
        let saved = call(
            &core,
            json!({"method": "agents.set_path", "params": {"agent": "claude", "path": old}}),
        )
        .await;
        let Response::Agents(AgentsResponse::Setup(outdated)) = saved else {
            panic!("unexpected {saved:?}");
        };
        assert_eq!(outdated.version.as_deref(), Some("2.0.1"));
        assert!(matches!(outdated.state, SetupState::Outdated { .. }));
        let gone = dir.0.join("gone");
        call(
            &core,
            json!({"method": "agents.set_path", "params": {"agent": "claude", "path": gone}}),
        )
        .await;
        assert_eq!(setup(&core).await.state, SetupState::NotFound);
    }

    #[tokio::test]
    async fn a_trusted_project_loads_its_own_settings() {
        let dir = Dir::new("trusted");
        let claude = dir.claude("2.1.331", &format!("{ONE_TURN}\ncat > /dev/null"));
        let core = core_with(Arc::new(ClaudeCli::new(Some(claude), unsandboxed())));
        let mut rx = core.subscribe();
        start(&core, &dir.0.to_string_lossy(), "hi").await;
        wait_idle(&mut rx).await;
        assert!(
            dir.read("claude.args")
                .contains("--setting-sources user --strict")
        );
        call(
            &core,
            json!({"method": "agents.stop", "params": {"session": "s1"}}),
        )
        .await;
        end(&mut rx).await;
        let review = call(
            &core,
            json!({"method": "agents.project_config", "params": {"project": "p1"}}),
        )
        .await;
        let Response::Agents(AgentsResponse::ProjectConfig(review)) = review else {
            panic!("unexpected {review:?}");
        };
        let fingerprint = review.current.fingerprint;
        call(
            &core,
            json!({"method": "projects.trust", "params": {
                "id": "p1", "trusted": true, "fingerprint": fingerprint}}),
        )
        .await;
        call(
            &core,
            json!({"method": "agents.start", "params": {
                "project": "p1", "agent": "claude", "mode": "direct", "prompt": "again"}}),
        )
        .await;
        wait_idle(&mut rx).await;
        let args = dir.read("claude.args");
        assert!(
            args.contains("--setting-sources user,project,local --strict"),
            "{args}"
        );
    }

    fn unsandboxed() -> Arc<dyn ovra_core::services::Sandbox> {
        Arc::new(ovra_core::fakes::FakeSandbox {
            status: ovra_core::services::SandboxStatus::Unavailable {
                reason: "none".into(),
            },
            fails: false,
            wrapped: Default::default(),
            servers: Default::default(),
        })
    }

    async fn sessions(core: &Core) -> Vec<ovra_core::protocol::agents::SessionInfo> {
        match call(
            core,
            json!({"method": "agents.list", "params": {"project": "p1"}}),
        )
        .await
        {
            Response::Agents(AgentsResponse::List(sessions)) => sessions,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_sandboxed_session_runs_the_wrapped_line_and_reports_it() {
        use ovra_core::fakes::FakeSandbox;
        use ovra_core::protocol::agents::{SandboxNetwork, SandboxReport};
        let dir = Dir::new("sandboxed");
        let claude = dir.claude(
            "2.1.331",
            &format!("echo \"$OVRA_FAKE_SANDBOX\" > \"$0.sandbox\"\n{ONE_TURN}\ncat > /dev/null"),
        );
        // What the platform says it does not cover reaches the report.
        let sandbox = Arc::new(FakeSandbox {
            status: ovra_core::services::SandboxStatus::Available {
                name: "fake".into(),
                gaps: vec!["DNS lookups".into(), "loopback services".into()],
            },
            ..FakeSandbox::available()
        });
        let core = core_with(Arc::new(ClaudeCli::new(Some(claude), sandbox.clone())));
        let mut rx = core.subscribe();
        start(&core, &dir.0.to_string_lossy(), "hi").await;
        wait_idle(&mut rx).await;
        assert_eq!(dir.read("claude.sandbox").trim(), "1");
        let profiles = lock(&sandbox.wrapped).clone();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].work_dir, dir.0);
        assert_eq!(
            sessions(&core).await[0].sandbox,
            Some(SandboxReport {
                name: "fake".into(),
                network: SandboxNetwork::Open,
                gaps: vec!["DNS lookups".into(), "loopback services".into()],
            })
        );
    }

    #[tokio::test]
    async fn a_sandbox_that_cannot_wrap_fails_the_session_instead_of_running_it_bare() {
        use ovra_core::fakes::FakeSandbox;
        let dir = Dir::new("unwrappable");
        let claude = dir.claude("2.1.331", &format!("{ONE_TURN}\ncat > /dev/null"));
        let sandbox = FakeSandbox {
            fails: true,
            ..FakeSandbox::available()
        };
        let core = core_with(Arc::new(ClaudeCli::new(Some(claude), Arc::new(sandbox))));
        let mut rx = core.subscribe();
        start(&core, &dir.0.to_string_lossy(), "hi").await;
        let AgentStatus::Failed { message } = end(&mut rx).await else {
            panic!("the session did not fail");
        };
        assert!(message.contains("sandbox"), "{message}");
        assert!(!dir.0.join("claude.args").exists());
        assert_eq!(sessions(&core).await[0].sandbox, None);
    }

    #[tokio::test]
    async fn a_crash_reports_the_end_of_stderr() {
        let dir = Dir::new("crash");
        let claude = dir.claude("2.1.331", "read init\necho 'boom' >&2\nexit 3");
        let (_core, mut rx) = run(&dir, &claude).await;
        match end(&mut rx).await {
            AgentStatus::Failed { message } => {
                assert!(
                    message.contains("exit status: 3") && message.ends_with(": boom"),
                    "{message}"
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_unsupported_version_does_not_start() {
        let dir = Dir::new("old");
        let claude = dir.claude("1.0.0 (Claude Code)", "exit 0");
        let (_core, mut rx) = run(&dir, &claude).await;
        match end(&mut rx).await {
            AgentStatus::Failed { message } => assert!(message.contains("too old"), "{message}"),
            other => panic!("unexpected {other:?}"),
        }
        assert!(!dir.0.join("claude.args").exists());
    }

    #[tokio::test]
    async fn a_missing_executable_says_how_to_install() {
        let dir = Dir::new("missing");
        let (_core, mut rx) = run(&dir, &dir.0.join("no-claude")).await;
        match end(&mut rx).await {
            AgentStatus::Failed { message } => {
                assert!(message.contains("Install Claude Code"), "{message}");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

/// `claude` installed by npm on Windows is `claude.cmd`, which Rust runs
/// through `cmd.exe` with its own quoting. The command line, JSON included,
/// must get through it.
#[cfg(windows)]
#[tokio::test]
async fn a_cmd_script_gets_the_whole_command_line() {
    let dir = tempfile::TempDir::new().unwrap_or_else(|e| panic!("{e}"));
    let script = dir.path().join("claude.cmd");
    std::fs::write(&script, "@echo off\r\necho %*> \"%~dp0claude.args\"\r\n")
        .unwrap_or_else(|e| panic!("{e}"));
    let args = launch::args(&launch::Conversation::New("id".into()), false, None);
    let status = tokio::process::Command::new(&script)
        .args(&args)
        .status()
        .await
        .unwrap_or_else(|e| panic!("std refused the arguments for a .cmd: {e}"));
    assert!(status.success());
    let line =
        std::fs::read_to_string(dir.path().join("claude.args")).unwrap_or_else(|e| panic!("{e}"));
    assert!(line.contains("--strict-mcp-config"), "{line}");
    assert!(line.contains("mcpServers"), "{line}");
    assert!(line.trim_end().ends_with("--session-id id"), "{line}");
    assert_eq!(
        line.matches('"').count() % 2,
        0,
        "unbalanced quotes: {line}"
    );
}

/// IDE tools that answer every request with who called.
#[derive(Debug)]
struct WhoCalls;

impl IdeTools for WhoCalls {
    fn handle(
        &self,
        _core: Core,
        caller: ToolCaller,
        message: Value,
    ) -> BoxFuture<'_, Option<Value>> {
        Box::pin(async move {
            let id = message.get("id")?.clone();
            Some(json!({"jsonrpc": "2.0", "id": id, "result": {
                "session": caller.session.0, "role": format!("{:?}", caller.role)}}))
        })
    }
}

#[tokio::test]
async fn messages_for_the_ovra_mcp_server_reach_the_ide_tools() {
    let mcp = |id: &str, server: &str, message: Value| {
        json!({"cli": {"type": "control_request", "request_id": id,
            "request": {"subtype": "mcp_message", "server_name": server, "message": message}}})
    };
    let answer = |id: &str, reply: Value| {
        json!({"ovra": {"type": "control_response", "response": {
            "subtype": "success", "request_id": id, "response": {"mcp_response": reply}}}})
    };
    let steps = vec![
        json!({"ovra": wire::initialize()}),
        json!({"cli": {"type": "control_response",
            "response": {"subtype": "success", "request_id": "init", "response": {}}}}),
        json!({"ovra": wire::user("go")}),
        mcp(
            "m1",
            "ovra",
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/list"}),
        ),
        answer(
            "m1",
            json!({"jsonrpc": "2.0", "id": 4,
            "result": {"session": "s1", "role": "Worker"}}),
        ),
        mcp(
            "m2",
            "ovra",
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        ),
        answer("m2", json!({"jsonrpc": "2.0", "id": 0, "result": {}})),
        mcp(
            "m3",
            "github",
            json!({"jsonrpc": "2.0", "id": 5, "method": "tools/list"}),
        ),
        json!({"ovra": {"type": "control_response", "response": {"subtype": "error",
            "request_id": "m3", "error": "Ovra has no MCP server github for this session"}}}),
        json!({"core": {"method": "agents.stop", "params": {"session": "s1"}}}),
        json!({"ovra": wire::interrupt("interrupt1")}),
    ];
    let runtime = Arc::new(Replay {
        steps,
        ide: Some(ToolRole::Worker),
        ..Replay::default()
    });
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(runtime.clone())
        .ide_tools(Arc::new(WhoCalls))
        .build();
    let _ = runtime.core.set(core.clone());
    start(&core, ROOT, "go").await;
    let cli = lock(&runtime.cli).take();
    match cli {
        Some(cli) => cli
            .await
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|e| panic!("{e}")),
        None => panic!("the session did not start"),
    }
}
