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
    process: ClaudeProcess,
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
        } else if step.get("interrupt").is_some() {
            process.interrupt().map_err(|e| fail(e.message))?;
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
            ClaudeProcess(commands.clone()),
        ));
        *lock(&self.cli) = Some(cli);
        let ended = self.ended.clone();
        tokio::spawn(async move {
            let end = driver::run(
                BufReader::new(ovra_stdout),
                ovra_stdin,
                spec.cwd,
                host.clone(),
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

/// Replays a fixture to its end and returns the session's transcript.
async fn replay(fixture: &str, prompt: &str) -> (Vec<AgentMessage>, Vec<Ended>) {
    let runtime = Arc::new(Replay {
        steps: steps(fixture),
        ..Replay::default()
    });
    let core = core_with(runtime.clone());
    let _ = runtime.core.set(core.clone());
    let mut rx = core.subscribe();
    start(&core, "/w", prompt).await;
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

#[tokio::test]
async fn an_allowed_edit_over_two_turns() {
    let (transcript, ended) = replay(
        include_str!("../fixtures/edit_allowed.jsonl"),
        "Fix the typo in README.md",
    )
    .await;
    assert_eq!(ended, [Ended::Closed { stopped: true }]);
    insta::assert_json_snapshot!(transcript);
}

#[tokio::test]
async fn a_denied_write_a_subagent_and_a_failed_turn() {
    let (transcript, ended) = replay(
        include_str!("../fixtures/denied_and_subagent.jsonl"),
        "Save notes next to the project",
    )
    .await;
    assert_eq!(ended, [Ended::Closed { stopped: true }]);
    insta::assert_json_snapshot!(transcript);
}

const INIT: &str = r#"{"ovra":{"type":"control_request","request_id":"init","request":{"subtype":"initialize","hooks":{"PreToolUse":[{"matcher":null,"hookCallbackIds":["ovra_policy"]}]}}}}
{"cli":{"type":"control_response","response":{"subtype":"success","request_id":"init","response":{}}}}"#;

#[tokio::test]
async fn an_interrupt_ends_the_turn_and_keeps_the_session() {
    let script = format!(
        "{INIT}\n{}",
        r#"{"ovra":{"type":"user","message":{"role":"user","content":"hi"}}}
{"cli":{"type":"system","subtype":"init","session_id":"x"}}
{"interrupt":true}
{"ovra":{"type":"control_request","request_id":"interrupt1","request":{"subtype":"interrupt"}}}
{"cli":{"type":"control_response","response":{"subtype":"success","request_id":"interrupt1","response":{}}}}
{"cli":{"type":"result","subtype":"error_during_execution","is_error":true,"usage":{}}}
{"wait":{"type":"agents.status","data":{"status":{"state":"idle"}}}}
{"core":{"method":"agents.send","params":{"session":"s1","text":"go on"}}}
{"ovra":{"type":"user","message":{"role":"user","content":"go on"}}}
{"core":{"method":"agents.stop","params":{"session":"s1"}}}
{"ovra":{"type":"control_request","request_id":"interrupt2","request":{"subtype":"interrupt"}}}"#
    );
    let (transcript, _) = replay(&script, "hi").await;
    assert_eq!(
        transcript[1..],
        [
            AgentMessage::Error {
                message: "The turn failed.".into()
            },
            AgentMessage::TurnEnd {
                usage: Default::default()
            },
            AgentMessage::UserText {
                text: "go on".into()
            },
        ]
    );
}

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
    start(&core, "/w", "hi").await;
    assert_eq!(
        end(&mut rx).await,
        AgentStatus::Failed {
            message: "claude did not start the session: bad hooks".into()
        }
    );
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
    start(&core, "/w", "hi").await;
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
        let core = core_with(Arc::new(ClaudeCli::new(Some(claude.to_owned()))));
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
