//! The driver through the core, with the agent's side played over
//! in-memory pipes.

use std::sync::{Mutex, PoisonError};

use ovra_core::Core;
use ovra_core::fakes::{FakeSandbox, MemoryProjects};
use ovra_core::protocol::agents::{
    AgentKind, AgentMessage, AgentsEvent, AgentsResponse, Capabilities,
};
use ovra_core::protocol::permissions::PermissionsResponse;
use ovra_core::protocol::{Event, Request, Response};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};
use tokio::sync::broadcast;

use super::*;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

async fn call(core: &Core, request: Value) -> Response {
    let request: Request = serde_json::from_value(request).expect("a request");
    core.handle(request).await.expect("the request succeeds")
}

/// The agent's side of the pipes.
struct Agent {
    stdin: Lines<BufReader<DuplexStream>>,
    stdout: DuplexStream,
}

impl Agent {
    /// Ovra's next line; `None` once stdin closed.
    async fn read(&mut self) -> Option<Value> {
        let line = self.stdin.next_line().await.expect("reads stdin")?;
        Some(serde_json::from_str(&line).expect("Ovra writes JSON"))
    }

    async fn expect(&mut self, expected: Value) {
        assert_eq!(self.read().await, Some(expected));
    }

    async fn write(&mut self, line: &str) {
        let line = format!("{line}\n");
        self.stdout
            .write_all(line.as_bytes())
            .await
            .expect("writes stdout");
    }

    async fn update(&mut self, update: Value) {
        let line = json!({"jsonrpc": "2.0", "method": "session/update",
            "params": {"sessionId": "acp-1", "update": update}});
        self.write(&line.to_string()).await;
    }

    async fn answer(&mut self, id: u64, result: Value) {
        self.write(&json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string())
            .await;
    }

    /// Answers `initialize` and `session/new`, then reads the first prompt.
    async fn start(&mut self, prompt: &str) {
        self.expect(v1::initialize(0)).await;
        self.answer(0, json!({"protocolVersion": 1})).await;
        self.expect(v1::new_session(1, std::path::Path::new(ROOT)))
            .await;
        self.answer(1, json!({"sessionId": "acp-1"})).await;
        self.expect(v1::prompt(2, "acp-1", prompt)).await;
    }

    async fn end_turn(&mut self, id: u64, reason: &str) {
        self.answer(id, json!({"stopReason": reason})).await;
    }
}

/// A runtime that connects the driver to the test instead of a process.
#[derive(Debug, Default)]
struct Piped {
    agent: Mutex<Option<(DuplexStream, DuplexStream)>>,
    ended: Mutex<Option<tokio::sync::oneshot::Sender<Ended>>>,
}

impl AgentRuntime for Piped {
    fn start(&self, spec: AgentSpec, host: AgentHost) -> Result<Box<dyn RunningAgent>> {
        let (agent_stdout, ovra_stdout) = tokio::io::duplex(1 << 16);
        let (ovra_stdin, agent_stdin) = tokio::io::duplex(1 << 16);
        *lock(&self.agent) = Some((agent_stdin, agent_stdout));
        let ended = lock(&self.ended).take();
        let (commands, rx) = mpsc::unbounded_channel();
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
            if let Some(ended) = ended {
                let _ = ended.send(end);
            }
            host.status(status);
        });
        Ok(Box::new(AcpProcess(commands)))
    }
}

const ROOT: &str = if cfg!(windows) { r"C:\w" } else { "/w" };

struct Session {
    core: Core,
    events: broadcast::Receiver<Event>,
    agent: Agent,
    ended: tokio::sync::oneshot::Receiver<Ended>,
}

async fn session(prompt: &str) -> Session {
    let runtime = Arc::new(Piped::default());
    let (tx, ended) = tokio::sync::oneshot::channel();
    *lock(&runtime.ended) = Some(tx);
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(runtime.clone())
        .build();
    let events = core.subscribe();
    call(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    call(
        &core,
        json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": prompt}}),
    )
    .await;
    let (stdin, stdout) = lock(&runtime.agent).take().expect("the session started");
    let agent = Agent {
        stdin: BufReader::new(stdin).lines(),
        stdout,
    };
    Session {
        core,
        events,
        agent,
        ended,
    }
}

impl Session {
    async fn request(&self, method: &str, params: Value) {
        call(&self.core, json!({"method": method, "params": params})).await;
    }

    /// Waits for the session's next state `status` (by its `state`).
    async fn wait_status(&mut self, state: &str) {
        loop {
            let event = Core::next_event(&mut self.events).await.expect("an event");
            if let Event::Agents(AgentsEvent::Status { status, .. }) = event
                && serde_json::to_value(&status).expect("json")["state"] == state
            {
                return;
            }
        }
    }

    /// Waits for a permission request and returns its id.
    async fn wait_asked(&mut self) -> String {
        loop {
            let event = Core::next_event(&mut self.events).await.expect("an event");
            let event = serde_json::to_value(&event).expect("json");
            if event["type"] == "permissions.requested" {
                return event["data"]["id"].as_str().expect("an id").to_owned();
            }
        }
    }

    /// How many permission requests wait for the person.
    async fn pending(&self) -> usize {
        match call(&self.core, json!({"method": "permissions.pending"})).await {
            Response::Permissions(PermissionsResponse::Pending(pending)) => pending.len(),
            other => panic!("unexpected {other:?}"),
        }
    }

    /// Stops the session, which closes stdin, and closes stdout as the
    /// agent then does.
    async fn stop(mut self) -> (Vec<AgentMessage>, Ended) {
        self.request("agents.stop", json!({"session": "s1"})).await;
        let mut rest = Vec::new();
        while let Some(line) = self.agent.read().await {
            rest.push(line);
        }
        assert_eq!(rest, Vec::<Value>::new(), "Ovra wrote more");
        let Self {
            core, agent, ended, ..
        } = self;
        drop(agent);
        let ended = ended.await.expect("the driver ended");
        let session = json!({"method": "agents.transcript", "params": {"session": "s1"}});
        match call(&core, session).await {
            Response::Agents(AgentsResponse::Transcript(t)) => (t.messages, ended),
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[tokio::test]
async fn a_turn_shows_text_and_tool_calls_in_order() {
    let mut s = session("Read a.rs").await;
    s.agent.start("Read a.rs").await;
    s.agent
        .update(json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "Let me "}}))
        .await;
    s.agent
        .update(json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "look."}}))
        .await;
    s.agent
        .update(json!({"sessionUpdate": "agent_thought_chunk",
            "content": {"type": "text", "text": "Hmm"}}))
        .await;
    s.agent
        .update(
            json!({"sessionUpdate": "tool_call", "toolCallId": "c1", "title": "Read a.rs",
            "kind": "read", "status": "pending", "locations": [{"path": "/w/a.rs"}],
            "rawInput": {"path": "/w/a.rs"}}),
        )
        .await;
    s.agent
        .update(json!({"sessionUpdate": "plan", "entries": []}))
        .await;
    s.agent
        .update(
            json!({"sessionUpdate": "tool_call_update", "toolCallId": "c1",
            "status": "completed",
            "content": [{"type": "content", "content": {"type": "text", "text": "fn main() {}"}}]}),
        )
        .await;
    // A late update of a finished call adds no second result.
    s.agent
        .update(
            json!({"sessionUpdate": "tool_call_update", "toolCallId": "c1",
            "status": "completed"}),
        )
        .await;
    s.agent
        .update(json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "It is empty."}}))
        .await;
    s.agent.end_turn(2, "end_turn").await;
    s.wait_status("idle").await;
    let (transcript, ended) = s.stop().await;
    assert_eq!(ended, Ended::Closed { stopped: true });
    insta::assert_json_snapshot!(transcript);
}

#[tokio::test]
async fn stop_reasons_end_turns_as_claudes_results_do() {
    let mut s = session("one").await;
    s.agent.start("one").await;
    // Messages wait for the turn that runs.
    for text in ["two", "three", "four", "five", "six"] {
        s.request("agents.send", json!({"session": "s1", "text": text}))
            .await;
    }
    s.agent.end_turn(2, "max_tokens").await;
    s.agent.expect(v1::prompt(3, "acp-1", "two")).await;
    s.agent.end_turn(3, "max_turn_requests").await;
    s.agent.expect(v1::prompt(4, "acp-1", "three")).await;
    s.agent.end_turn(4, "refusal").await;
    s.agent.expect(v1::prompt(5, "acp-1", "four")).await;
    s.agent.end_turn(5, "cancelled").await;
    s.agent.expect(v1::prompt(6, "acp-1", "five")).await;
    s.agent
        .write(r#"{"jsonrpc":"2.0","id":6,"error":{"code":-32603,"message":"Quota exceeded"}}"#)
        .await;
    s.agent.expect(v1::prompt(7, "acp-1", "six")).await;
    s.agent.answer(7, json!({})).await;
    // Each of the six turns ended.
    for _ in 0..6 {
        s.wait_status("idle").await;
    }
    let (transcript, ended) = s.stop().await;
    assert_eq!(ended, Ended::Closed { stopped: true });
    let ends: Vec<&AgentMessage> = transcript
        .iter()
        .filter(|m| !matches!(m, AgentMessage::UserText { .. }))
        .collect();
    insta::assert_json_snapshot!(ends);
}

fn asks(id: u64, call: &str, title: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "method": "session/request_permission", "params": {
    "sessionId": "acp-1",
    "toolCall": {"toolCallId": call, "title": title, "kind": "execute",
        "rawInput": {"command": "ls"}},
    "options": [
        {"optionId": "always", "name": "Always", "kind": "allow_always"},
        {"optionId": "once", "name": "Allow", "kind": "allow_once"},
        {"optionId": "no", "name": "Reject", "kind": "reject_once"},
    ]}})
    .to_string()
}

fn selected(id: u64, option: &str) -> Value {
    v1::permission(&json!(id), &v1::Outcome::Selected(option.into()))
}

#[tokio::test]
async fn permission_requests_are_answered_once_as_the_person_decides() {
    let mut s = session("go").await;
    s.agent.start("go").await;
    s.agent.write(&asks(10, "c1", "ls")).await;
    let asked = s.wait_asked().await;
    assert_eq!(s.pending().await, 1);
    s.request(
        "permissions.answer",
        json!({"id": asked, "decision": "allow"}),
    )
    .await;
    s.agent.expect(selected(10, "once")).await;
    s.agent.write(&asks(11, "c2", "ls -a")).await;
    let asked = s.wait_asked().await;
    s.request(
        "permissions.answer",
        json!({"id": asked, "decision": "deny"}),
    )
    .await;
    s.agent.expect(selected(11, "no")).await;
    s.agent.end_turn(2, "end_turn").await;
    s.wait_status("idle").await;
    let (transcript, _) = s.stop().await;
    insta::assert_json_snapshot!(transcript);
}

#[tokio::test]
async fn the_projects_policy_refuses_before_the_person_is_asked() {
    let mut s = session("go").await;
    s.agent.start("go").await;
    let policy = json!({"rules": [{"tool": "acp:execute", "action": "deny"}]});
    s.request("policy.set", json!({"project": "p1", "policy": policy}))
        .await;
    s.agent.write(&asks(10, "c1", "rm")).await;
    s.agent.expect(selected(10, "no")).await;
    s.agent.end_turn(2, "end_turn").await;
    s.wait_status("idle").await;
    assert_eq!(s.pending().await, 0);
    s.stop().await;
}

/// The title is the agent's own text: naming one of Claude's tools, one
/// of Ovra's or a policy rule's tool in it changes nothing, so the person
/// is asked about each call.
#[tokio::test]
async fn a_title_never_passes_for_a_tool_name() {
    let mut s = session("go").await;
    s.agent.start("go").await;
    let policy = json!({"rules": [
        {"tool": "rm", "action": "deny"},
        {"tool": "WebSearch", "action": "auto"},
    ]});
    s.request("policy.set", json!({"project": "p1", "policy": policy}))
        .await;
    for (id, title) in [
        (10, "mcp__ovra__spawn_agent"),
        (11, "WebSearch"),
        (12, "rm"),
    ] {
        s.agent.write(&asks(id, &format!("c{id}"), title)).await;
        let asked = s.wait_asked().await;
        assert_eq!(s.pending().await, 1, "{title}");
        s.request(
            "permissions.answer",
            json!({"id": asked, "decision": "allow"}),
        )
        .await;
        s.agent.expect(selected(id, "once")).await;
    }
    s.agent.end_turn(2, "end_turn").await;
    s.wait_status("idle").await;
    s.stop().await;
}

#[tokio::test]
async fn a_turn_that_ends_withdraws_what_it_asked() {
    let mut s = session("go").await;
    s.agent.start("go").await;
    s.agent.write(&asks(10, "c1", "ls")).await;
    s.wait_asked().await;
    s.agent.end_turn(2, "end_turn").await;
    s.wait_status("idle").await;
    assert_eq!(s.pending().await, 0);
    s.stop().await;
}

#[tokio::test]
async fn an_interrupt_cancels_the_turn_and_what_it_asked() {
    let mut s = session("go").await;
    s.agent.start("go").await;
    s.agent.write(&asks(10, "c1", "ls")).await;
    s.wait_asked().await;
    s.request("agents.interrupt", json!({"session": "s1"}))
        .await;
    s.agent.expect(v1::cancel("acp-1")).await;
    // The core refuses the request as it interrupts: whichever comes first,
    // the agent hears of it once and never as allowed.
    let answer = s.agent.read().await.expect("an answer");
    assert!(
        answer == v1::permission(&json!(10), &v1::Outcome::Cancelled)
            || answer == selected(10, "no"),
        "{answer}"
    );
    s.agent.end_turn(2, "cancelled").await;
    s.wait_status("idle").await;
    // The session goes on.
    s.request("agents.send", json!({"session": "s1", "text": "again"}))
        .await;
    s.agent.expect(v1::prompt(3, "acp-1", "again")).await;
    s.agent.end_turn(3, "end_turn").await;
    s.wait_status("idle").await;
    let (transcript, ended) = s.stop().await;
    assert_eq!(ended, Ended::Closed { stopped: true });
    assert!(transcript.contains(&AgentMessage::Interrupted));
    assert!(
        !transcript
            .iter()
            .any(|m| matches!(m, AgentMessage::Error { .. }))
    );
}

#[tokio::test]
async fn a_stop_cancels_the_running_turn_first() {
    let mut s = session("go").await;
    s.agent.start("go").await;
    s.request("agents.stop", json!({"session": "s1"})).await;
    s.agent.expect(v1::cancel("acp-1")).await;
    assert_eq!(s.agent.read().await, None);
    drop(s.agent);
    assert_eq!(
        s.ended.await.expect("ended"),
        Ended::Closed { stopped: true }
    );
}

#[tokio::test(start_paused = true)]
async fn an_agent_that_ignores_a_stop_is_given_up_on() {
    let mut s = session("go").await;
    s.agent.start("go").await;
    s.agent.end_turn(2, "end_turn").await;
    s.wait_status("idle").await;
    s.request("agents.stop", json!({"session": "s1"})).await;
    assert_eq!(s.agent.read().await, None);
    // The agent keeps stdout open.
    assert_eq!(s.ended.await.expect("ended"), Ended::Unresponsive);
}

#[tokio::test]
async fn requests_ovra_does_not_serve_are_refused() {
    let mut s = session("go").await;
    s.agent.start("go").await;
    s.agent
        .write(r#"{"jsonrpc":"2.0","id":"f1","method":"fs/read_text_file","params":{"path":"/etc/hosts"}}"#)
        .await;
    let refused = s.agent.read().await.expect("an answer");
    assert_eq!(refused["id"], "f1");
    assert_eq!(refused["error"]["code"], -32601);
    s.agent.end_turn(2, "end_turn").await;
    s.wait_status("idle").await;
    s.stop().await;
}

async fn fails_with(lines: &[&str]) -> Ended {
    let mut s = session("go").await;
    s.agent.expect(v1::initialize(0)).await;
    for line in lines {
        s.agent.write(line).await;
    }
    s.wait_status("failed").await;
    s.ended.await.expect("ended")
}

#[tokio::test]
async fn a_broken_start_or_line_fails_the_session() {
    let failures = [
        vec![r#"{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":2}}"#],
        vec![r#"{"jsonrpc":"2.0","id":0,"error":{"code":-32603,"message":"no"}}"#],
        vec![
            r#"{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1}}"#,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"Authentication required"}}"#,
        ],
        vec!["Loading..."],
    ];
    let mut messages = Vec::new();
    for lines in failures {
        match fails_with(&lines).await {
            Ended::Failed(message) => messages.push(message),
            other => panic!("unexpected {other:?}"),
        }
    }
    insta::assert_snapshot!(messages.join("\n"));
}

static LISTS: ProviderLists = ProviderLists {
    name: "Fake",
    home_dir: ".fake",
    state_file: ".fake.json",
    config_var: "FAKE_HOME",
    private: &[],
    in_place: &[],
    files: &[],
    layered: false,
    files_made: &[],
    projects: "projects",
    project_memory: "MEMORY.md",
    env: &[],
    windows_env: &[],
    fixture_var: "OVRA_FAKE_ACP_FIXTURE",
};

fn described(program: &str, args: &[&str]) -> AcpAgent {
    AcpAgent {
        info: ProviderInfo {
            id: AgentKind::Claude,
            name: "Fake".into(),
            capabilities: Capabilities::default(),
        },
        program: program.into(),
        args: args.iter().map(Into::into).collect(),
        lists: &LISTS,
        known: None,
    }
}

/// How a session through `sandbox` ends; the agent never runs.
async fn through(sandbox: FakeSandbox) -> AgentStatus {
    let agent = described("/fake-acp", &[]);
    ended_in(AcpAgents::new(vec![agent], Arc::new(sandbox)), ROOT).await
}

/// How a session of `runtime` in the project `root` ends.
async fn ended_in(runtime: AcpAgents, root: &str) -> AgentStatus {
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(runtime))
        .build();
    let mut events = core.subscribe();
    call(
        &core,
        json!({"method": "projects.add", "params": {"root": root}}),
    )
    .await;
    call(
        &core,
        json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "hi"}}),
    )
    .await;
    loop {
        let event = Core::next_event(&mut events).await.expect("an event");
        if let Event::Agents(AgentsEvent::Status { status, .. }) = event
            && status.is_ended()
        {
            return status;
        }
    }
}

#[tokio::test]
async fn no_agent_runs_without_the_sandbox() {
    let unavailable = FakeSandbox {
        status: SandboxStatus::Unavailable {
            reason: "none here".into(),
        },
        ..FakeSandbox::available()
    };
    let failing = FakeSandbox {
        fails: true,
        ..FakeSandbox::available()
    };
    let failed = |message: &str| AgentStatus::Failed {
        message: message.into(),
    };
    assert_eq!(
        through(unavailable).await,
        failed("Fake runs only inside the sandbox, which is not available here: none here")
    );
    assert_eq!(
        through(failing).await,
        failed("the sandbox could not wrap Fake: the fake sandbox fails")
    );
}

#[tokio::test]
async fn a_panic_in_a_session_still_ends_it() {
    let status = guarded(async { panic!("a bug") }).await;
    let AgentStatus::Failed { message } = status else {
        panic!("unexpected {status:?}");
    };
    assert!(
        message.starts_with("the session ended unexpectedly"),
        "{message}"
    );
    assert!(message.contains("a bug"), "{message}");
}

#[tokio::test]
async fn a_line_that_is_no_utf8_fails_the_session() {
    let mut s = session("go").await;
    s.agent.expect(v1::initialize(0)).await;
    s.agent
        .stdout
        .write_all(b"\xff\xfe{}\n")
        .await
        .expect("writes");
    s.wait_status("failed").await;
    match s.ended.await.expect("ended") {
        Ended::Failed(message) => assert!(
            message.starts_with("the agent wrote a line Ovra cannot read"),
            "{message}"
        ),
        other => panic!("unexpected {other:?}"),
    }
}

/// An agent that closes stdout but keeps running is killed after the
/// grace period instead of being waited on for ever.
#[cfg(unix)]
#[tokio::test(start_paused = true)]
async fn an_agent_that_closes_stdout_and_lives_on_is_killed() {
    let agent = described("sh", &["-c", "exec >&-; exec sleep 30"]);
    let runtime = AcpAgents::new(vec![agent], Arc::new(FakeSandbox::available()));
    let status = ended_in(runtime, env!("CARGO_MANIFEST_DIR")).await;
    let AgentStatus::Failed { message } = status else {
        panic!("unexpected {status:?}");
    };
    assert!(message.starts_with("Fake exited with signal"), "{message}");
}

#[test]
fn no_acp_agent_claims_an_enforced_policy() {
    let mut agent = described("fake-acp", &[]);
    agent.info.capabilities = Capabilities {
        enforced_policy: true,
        approvals: true,
        ..Capabilities::default()
    };
    let runtime = AcpAgents::new(vec![agent.clone()], Arc::new(FakeSandbox::available()));
    let mut expected = agent.info;
    expected.capabilities.enforced_policy = false;
    assert_eq!(runtime.providers(), vec![expected]);
}

fn spec() -> AgentSpec {
    use ovra_core::protocol::agents::{SessionId, SessionMode};
    use ovra_core::services::{ProfileKind, ProfileNetwork};
    AgentSpec {
        add_dirs: Vec::new(),
        session: SessionId("s1".into()),
        agent: AgentKind::Claude,
        mode: SessionMode::Isolated,
        cwd: ROOT.into(),
        prompt: "hi".into(),
        executable: None,
        config_dir: None,
        tools: None,
        trusted: true,
        resume: None,
        sandbox: SandboxProfile {
            more_work_dirs: Vec::new(),
            work_dir: ROOT.into(),
            work_dir_read_only: false,
            read: Vec::new(),
            write: Vec::new(),
            expose: Vec::new(),
            forward: Vec::new(),
            network: ProfileNetwork::Blocked,
            kind: ProfileKind::Agent,
            agent_state: None,
            project: None,
            work_env: None,
        },
        no_web: false,
        model: None,
        images: Vec::new(),
        servers: Vec::new(),
    }
}

#[test]
fn what_acp_sessions_cannot_do_is_refused() {
    use ovra_core::protocol::agents::SessionMode;
    use ovra_core::services::ToolRole;
    assert_eq!(unsupported(&spec()), None);
    let research = AgentSpec {
        mode: SessionMode::Research,
        ..spec()
    };
    let tools = AgentSpec {
        tools: Some(ToolRole::Worker),
        ..spec()
    };
    let resume = AgentSpec {
        resume: Some("acp-1".into()),
        ..spec()
    };
    let account = AgentSpec {
        config_dir: Some("/home/me/.fake-work".into()),
        ..spec()
    };
    let refused: Vec<_> = [research, tools, resume, account]
        .iter()
        .map(unsupported)
        .collect();
    assert_eq!(
        refused,
        [
            Some("run research sessions"),
            Some("use Ovra's IDE tools"),
            Some("resume a session"),
            Some("run with another account"),
        ]
    );
}

#[tokio::test]
async fn a_research_session_does_not_start() {
    let agent = described("fake-acp", &[]);
    let runtime = AcpAgents::new(vec![agent], Arc::new(FakeSandbox::available()));
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(runtime))
        .build();
    call(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    let start: Request = serde_json::from_value(json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "research", "prompt": "hi"}}))
    .expect("a request");
    let refused = core.handle(start).await.expect_err("refused");
    assert_eq!(refused.code, ErrorCode::Unavailable);
    assert_eq!(
        refused.message,
        "Fake over ACP cannot run research sessions yet"
    );
}

#[tokio::test]
async fn a_line_past_the_limit_fails_the_session() {
    let mut s = session("go").await;
    s.agent.expect(v1::initialize(0)).await;
    let long = vec![b' '; driver::MAX_LINE + 1];
    s.agent.stdout.write_all(&long).await.expect("writes");
    s.wait_status("failed").await;
    assert_eq!(
        s.ended.await.expect("ended"),
        Ended::Failed("the agent wrote a line longer than 4 MiB".into())
    );
}

#[tokio::test]
async fn a_long_line_within_the_limit_is_read_in_pieces() {
    let mut s = session("go").await;
    s.agent.expect(v1::initialize(0)).await;
    // Padding before the JSON, written in pieces the driver reads apart.
    let padding = " ".repeat(driver::MAX_LINE - 100);
    for piece in padding.as_bytes().chunks(1 << 16) {
        s.agent.stdout.write_all(piece).await.expect("writes");
    }
    s.agent
        .write(r#"{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1}}"#)
        .await;
    s.agent
        .expect(v1::new_session(1, std::path::Path::new(ROOT)))
        .await;
    s.stop().await;
}

#[tokio::test]
async fn stderr_keeps_a_bounded_tail_of_any_bytes() {
    let mut noise = vec![0xff_u8; 1 << 20];
    noise.extend_from_slice("\u{e9}".repeat(STDERR_TAIL).as_bytes());
    noise.extend_from_slice(b"\xfe the end\n");
    let tail = stderr_tail(noise.as_slice()).await;
    assert!(tail.starts_with('\u{2026}'), "{}", &tail[..20]);
    assert!(tail.ends_with("\u{fffd} the end"));
    assert_eq!(tail.chars().count(), STDERR_TAIL + 2);
}

/// A request in a cancelled turn, or after the turn, has nothing left to
/// decide: the agent hears `cancelled` and the person is not asked.
#[tokio::test]
async fn a_request_outside_a_running_turn_is_cancelled_at_once() {
    let mut s = session("go").await;
    s.agent.start("go").await;
    let cancelled = |id: u64| v1::permission(&json!(id), &v1::Outcome::Cancelled);
    s.request("agents.interrupt", json!({"session": "s1"}))
        .await;
    s.agent.expect(v1::cancel("acp-1")).await;
    s.agent.write(&asks(10, "c1", "ls")).await;
    s.agent.expect(cancelled(10)).await;
    s.agent.end_turn(2, "cancelled").await;
    s.wait_status("idle").await;
    s.agent.write(&asks(11, "c2", "ls")).await;
    s.agent.expect(cancelled(11)).await;
    assert_eq!(s.pending().await, 0);
    // The next turn asks again.
    s.request("agents.send", json!({"session": "s1", "text": "again"}))
        .await;
    s.agent.expect(v1::prompt(3, "acp-1", "again")).await;
    s.agent.write(&asks(12, "c3", "ls")).await;
    s.wait_asked().await;
    assert_eq!(s.pending().await, 1);
    s.agent.end_turn(3, "end_turn").await;
    s.wait_status("idle").await;
    s.stop().await;
}

#[tokio::test]
async fn an_interrupt_before_the_session_drops_its_first_prompt() {
    let mut s = session("go").await;
    s.agent.expect(v1::initialize(0)).await;
    s.request("agents.interrupt", json!({"session": "s1"}))
        .await;
    s.wait_status("idle").await;
    s.agent.answer(0, json!({"protocolVersion": 1})).await;
    s.agent
        .expect(v1::new_session(1, std::path::Path::new(ROOT)))
        .await;
    s.agent.answer(1, json!({"sessionId": "acp-1"})).await;
    s.request("agents.send", json!({"session": "s1", "text": "again"}))
        .await;
    // The dropped prompt was never sent.
    s.agent.expect(v1::prompt(2, "acp-1", "again")).await;
    s.agent.end_turn(2, "end_turn").await;
    s.wait_status("idle").await;
    let (transcript, ended) = s.stop().await;
    assert_eq!(ended, Ended::Closed { stopped: true });
    assert!(transcript.contains(&AgentMessage::Interrupted));
}

#[test]
fn a_version_is_the_first_word_that_reads_like_one() {
    assert_eq!(version("0.71.1\n").as_deref(), Some("0.71.1"));
    assert_eq!(version("codex-acp v0.16.0").as_deref(), Some("0.16.0"));
    assert_eq!(version("Usage: codex-acp [OPTIONS]"), None);
}

#[cfg(unix)]
#[tokio::test]
async fn setup_runs_the_check_and_remembers_a_missing_sign_in() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("ovra-acp-setup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let script = |name: &str, body: &str| {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("a script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("mode");
        path
    };
    let ok = script("ok", "echo 1.2.3");
    let bad = script("bad", "exit 4");
    let agents = AcpAgents::new(vec![crate::gemini()], Arc::new(FakeSandbox::available()));
    let check = |path: PathBuf| agents.setup(AgentKind::Gemini, Some(path), None);
    let ready = check(ok.clone()).await;
    assert_eq!(
        (ready.state, ready.version.as_deref()),
        (SetupState::Ready, Some("1.2.3"))
    );
    assert!(matches!(check(bad).await.state, SetupState::Broken { .. }));
    assert_eq!(check(dir.join("none")).await.state, SetupState::NotFound);
    assert_eq!(
        agents.setup(AgentKind::Codex, None, None).await.state,
        SetupState::NotFound
    );
    lock(&agents.signed_out).insert(AgentKind::Gemini);
    assert_eq!(check(ok.clone()).await.state, SetupState::SignedOut);
    // Installed, but with no sandbox it cannot start: not ready.
    let unsandboxed = AcpAgents::new(
        vec![crate::gemini()],
        Arc::new(FakeSandbox {
            status: SandboxStatus::Unavailable {
                reason: "none here".into(),
            },
            ..FakeSandbox::available()
        }),
    );
    let state = unsandboxed
        .setup(AgentKind::Gemini, Some(ok), None)
        .await
        .state;
    assert!(
        matches!(&state, SetupState::Broken { message } if message.contains("sandbox")),
        "{state:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_bare_name_is_found_in_an_absolute_path_folder_only() {
    use std::os::unix::fs::PermissionsExt;

    // `find` reads the login shell's `PATH` first; no test here sets one.
    let dir = std::env::temp_dir().join(format!("ovra-acp-find-{}", std::process::id()));
    let executable = |path: &Path| {
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a folder");
        std::fs::write(path, "#!/bin/sh\n").expect("a script");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("mode");
    };
    // One reached by a relative entry, listed first, and one by an absolute.
    let cwd = std::env::current_dir().expect("a working folder");
    let relative = pathdiff(&dir.join("rel"), &cwd);
    executable(&dir.join("rel/agent"));
    executable(&dir.join("abs/agent"));
    std::fs::write(dir.join("abs/plain"), "").expect("a file");
    let path = std::env::join_paths([PathBuf::from("."), relative, dir.join("abs")]).ok();
    assert_eq!(
        find(Path::new("agent"), path.clone()),
        Some(dir.join("abs/agent"))
    );
    assert_eq!(find(Path::new("plain"), path.clone()), None);
    assert_eq!(find(Path::new("none"), path.clone()), None);
    assert_eq!(
        find(Path::new("./x/agent"), path),
        Some(PathBuf::from("./x/agent"))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `to` relative to `from`, both absolute.
fn pathdiff(to: &Path, from: &Path) -> PathBuf {
    let ups = from.components().count() - 1;
    let mut out: PathBuf = std::iter::repeat_n("..", ups).collect();
    out.push(to.strip_prefix("/").expect("absolute"));
    out
}

#[tokio::test]
async fn an_agent_not_on_path_is_not_installed() {
    let agents = AcpAgents::new(
        vec![described("ovra-no-such-agent", &[])],
        Arc::new(FakeSandbox::available()),
    );
    assert_eq!(
        ended_in(agents, ROOT).await,
        AgentStatus::Failed {
            message: "Fake is not installed: Ovra did not find ovra-no-such-agent on PATH".into()
        }
    );
    let setup = AcpAgents::new(vec![crate::gemini()], Arc::new(FakeSandbox::available()))
        .setup(AgentKind::Gemini, Some("ovra-no-such-agent".into()), None)
        .await;
    assert_eq!(setup.state, SetupState::NotFound);
}
