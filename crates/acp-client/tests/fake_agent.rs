//! The real process path against the fake ACP agent (`fake-acp`), through
//! the fake sandbox.

use std::path::Path;
use std::sync::Arc;

use ovra_acp_client::{AcpAgent, AcpAgents};
use ovra_core::Core;
use ovra_core::fakes::{FakeSandbox, MemoryProjects};
use ovra_core::protocol::agents::{
    AgentKind, AgentMessage, AgentStatus, AgentsEvent, AgentsResponse, Capabilities, ProviderInfo,
};
use ovra_core::protocol::{Event, Request, Response};
use ovra_core::services::ProviderLists;
use serde_json::{Value, json};
use tokio::sync::broadcast;

const FAKE: &str = env!("CARGO_BIN_EXE_fake-acp");

static LISTS: ProviderLists = ProviderLists {
    name: "Fake",
    home_dir: ".fake",
    state_file: ".fake.json",
    config_var: "FAKE_HOME",
    private: &[],
    in_place: &[],
    files: &[],
    files_made: &[],
    projects: "projects",
    project_memory: "MEMORY.md",
    env: &["FAKE_API_BASE"],
    windows_env: &[],
    fixture_var: "OVRA_FAKE_ACP_FIXTURE",
};

fn agent() -> AcpAgent {
    AcpAgent {
        info: ProviderInfo {
            // The only provider id there is until the router comes.
            id: AgentKind::Claude,
            name: "Fake".into(),
            capabilities: Capabilities {
                approvals: true,
                interrupt: true,
                text_deltas: true,
                ..Capabilities::default()
            },
        },
        program: FAKE.into(),
        args: vec!["--acp".into()],
        lists: &LISTS,
        known: None,
    }
}

/// The project: a real folder, which the agent runs in.
fn root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

async fn call(core: &Core, method: &str, params: Value) -> Response {
    let request: Request =
        serde_json::from_value(json!({"method": method, "params": params})).expect("a request");
    core.handle(request).await.expect("the request succeeds")
}

struct Session {
    core: Core,
    events: broadcast::Receiver<Event>,
}

async fn start(sandbox: Arc<FakeSandbox>, prompt: &str) -> Session {
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(AcpAgents::new(vec![agent()], sandbox)))
        .build();
    let events = core.subscribe();
    call(&core, "projects.add", json!({"root": root()})).await;
    call(
        &core,
        "agents.start",
        json!({"project": "p1", "agent": "claude", "mode": "direct", "prompt": prompt}),
    )
    .await;
    Session { core, events }
}

impl Session {
    async fn send(&self, method: &str, params: Value) {
        call(&self.core, method, params).await;
    }

    /// Waits for the next state that `pick` accepts.
    async fn status(&mut self, pick: impl Fn(&AgentStatus) -> bool) -> AgentStatus {
        loop {
            let event = Core::next_event(&mut self.events).await.expect("an event");
            if let Event::Agents(AgentsEvent::Status { status, .. }) = event
                && pick(&status)
            {
                return status;
            }
        }
    }

    async fn idle(&mut self) {
        self.status(|s| *s == AgentStatus::Idle).await;
    }

    async fn answer_next(&mut self, decision: &str) {
        let id = loop {
            let event = Core::next_event(&mut self.events).await.expect("an event");
            let event = serde_json::to_value(&event).expect("json");
            if event["type"] == "permissions.requested" {
                break event["data"]["id"].clone();
            }
        };
        self.send(
            "permissions.answer",
            json!({"id": id, "decision": decision}),
        )
        .await;
    }

    async fn transcript(&self) -> Vec<AgentMessage> {
        match call(&self.core, "agents.transcript", json!({"session": "s1"})).await {
            Response::Agents(AgentsResponse::Transcript(t)) => t,
            other => panic!("unexpected {other:?}"),
        }
    }

    /// Stops the session and returns its transcript; the agent exits.
    async fn stop(mut self) -> Vec<AgentMessage> {
        self.send("agents.stop", json!({"session": "s1"})).await;
        let end = self.status(AgentStatus::is_ended).await;
        assert_eq!(end, AgentStatus::Stopped);
        self.transcript().await
    }
}

fn texts(transcript: &[AgentMessage]) -> Vec<&str> {
    transcript
        .iter()
        .filter_map(|m| match m {
            AgentMessage::AssistantText { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn results(transcript: &[AgentMessage]) -> Vec<(&str, bool)> {
    transcript
        .iter()
        .filter_map(|m| match m {
            AgentMessage::ToolResult {
                output, is_error, ..
            } => Some((output.as_str(), *is_error)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_turn_with_text_runs_inside_the_sandbox() {
    let sandbox = Arc::new(FakeSandbox::available());
    let mut s = start(sandbox.clone(), "hello").await;
    s.idle().await;
    s.send("agents.send", json!({"session": "s1", "text": "whoami"}))
        .await;
    s.idle().await;
    let transcript = s.stop().await;
    let cwd = Path::new(root()).canonicalize().expect("the root");
    let expected = format!(r#"args ["--acp"], in {}, sandboxed true"#, cwd.display());
    assert_eq!(texts(&transcript), ["Hello, world.", expected.as_str()]);
    let wrapped = sandbox.wrapped.lock().expect("lock");
    let state = wrapped[0].agent_state.as_ref().expect("the agent's state");
    assert_eq!((state.lists.name, state.config_dir.clone()), ("Fake", None));
}

#[tokio::test]
async fn a_tool_call_the_person_allows_runs() {
    let mut s = start(Arc::new(FakeSandbox::available()), "run ls").await;
    s.answer_next("allow").await;
    s.idle().await;
    let transcript = s.stop().await;
    assert_eq!(results(&transcript), [("a.rs\nb.rs", false)]);
    assert_eq!(texts(&transcript), ["Done."]);
}

#[tokio::test]
async fn a_tool_call_the_person_denies_fails() {
    let mut s = start(Arc::new(FakeSandbox::available()), "run ls").await;
    s.answer_next("deny").await;
    s.idle().await;
    let transcript = s.stop().await;
    assert_eq!(results(&transcript), [("The person refused.", true)]);
    assert_eq!(texts(&transcript), ["Not allowed."]);
}

#[tokio::test]
async fn an_interrupt_ends_the_turn_and_keeps_the_session() {
    let mut s = start(Arc::new(FakeSandbox::available()), "wait").await;
    s.status(|s| *s == AgentStatus::Working).await;
    s.send("agents.interrupt", json!({"session": "s1"})).await;
    s.idle().await;
    s.send("agents.send", json!({"session": "s1", "text": "hello"}))
        .await;
    s.idle().await;
    let transcript = s.stop().await;
    assert!(transcript.contains(&AgentMessage::Interrupted));
    assert_eq!(texts(&transcript), ["Working...", "Hello, world."]);
}

#[tokio::test]
async fn an_agent_that_crashes_fails_with_its_stderr() {
    let mut s = start(Arc::new(FakeSandbox::available()), "crash").await;
    let end = s.status(AgentStatus::is_ended).await;
    let AgentStatus::Failed { message } = end else {
        panic!("unexpected {end:?}");
    };
    assert!(message.starts_with("Fake exited with"), "{message}");
    assert!(message.ends_with(": fake-acp: out of memory"), "{message}");
}
