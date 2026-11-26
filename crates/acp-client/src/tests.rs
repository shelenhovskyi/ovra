//! The driver through the core, with the agent's side played over
//! in-memory pipes.

use std::sync::{Arc, Mutex, PoisonError};

use ovra_core::Core;
use ovra_core::fakes::MemoryProjects;
use ovra_core::protocol::agents::{
    AgentKind, AgentMessage, AgentStatus, AgentsEvent, AgentsResponse, Capabilities,
};
use ovra_core::protocol::permissions::PermissionsResponse;
use ovra_core::protocol::{Event, Request, Response};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};
use tokio::sync::broadcast;

use super::*;
use crate::driver::Ended;

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
            Response::Agents(AgentsResponse::Transcript(t)) => (t, ended),
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
    let policy = json!({"rules": [{"tool": "rm", "action": "deny"}]});
    s.request("policy.set", json!({"project": "p1", "policy": policy}))
        .await;
    s.agent.write(&asks(10, "c1", "rm")).await;
    s.agent.expect(selected(10, "no")).await;
    s.agent.end_turn(2, "end_turn").await;
    s.wait_status("idle").await;
    assert_eq!(s.pending().await, 0);
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

#[test]
fn lists_the_agents_it_describes() {
    assert!(AcpAgents::default().providers().is_empty());
    let agent = ProviderInfo {
        id: AgentKind::Claude,
        name: "Some agent".into(),
        capabilities: Capabilities::default(),
    };
    assert_eq!(AcpAgents::new(vec![agent.clone()]).providers(), vec![agent]);
}
