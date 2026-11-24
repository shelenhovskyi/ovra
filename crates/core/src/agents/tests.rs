use std::sync::Arc;

use serde_json::json;
use tokio::sync::broadcast;

use crate::Core;
use crate::fakes::{EchoTerminals, MemoryProjects, MemorySessions, ScriptedAgent, Step};
use crate::protocol::agents::{
    AgentMessage, AgentStatus, AgentsEvent, AgentsResponse, SessionInfo, TurnUsage,
};
use crate::protocol::agents::{ProjectAgentConfig, ProjectConfigReview, SetupState};
use crate::protocol::permissions::{PermissionAnswer, PermissionsEvent, PermissionsResponse};
use crate::protocol::projects::{Project, ProjectsEvent, ProjectsResponse};
use crate::protocol::terminals::{TerminalsEvent, TerminalsResponse};
use crate::protocol::{ErrorCode, Event, ProtocolError, Request, Response};

fn core_with(agent: ScriptedAgent) -> Core {
    Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .terminals(Arc::new(EchoTerminals::default()))
        .agents(Arc::new(agent))
        .build()
}

async fn call(core: &Core, json: serde_json::Value) -> Result<Response, ProtocolError> {
    let request: Request =
        serde_json::from_value(json).unwrap_or_else(|e| panic!("bad request: {e}"));
    core.handle(request).await
}

async fn ok(core: &Core, json: serde_json::Value) -> Response {
    call(core, json)
        .await
        .unwrap_or_else(|e| panic!("request failed: {e}"))
}

async fn code(core: &Core, json: serde_json::Value) -> ErrorCode {
    match call(core, json).await {
        Err(e) => e.code,
        Ok(r) => panic!("expected an error, got {r:?}"),
    }
}

async fn project(core: &Core) {
    ok(
        core,
        json!({"method": "projects.add", "params": {"root": crate::test_path!("/work/a")}}),
    )
    .await;
}

fn start(prompt: &str) -> serde_json::Value {
    json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "direct", "prompt": prompt}})
}

/// Reads events until a session reaches `want`, returning every status seen.
async fn until(rx: &mut broadcast::Receiver<Event>, want: AgentStatus) -> Vec<AgentStatus> {
    let mut seen = Vec::new();
    loop {
        match Core::next_event(rx).await {
            Some(Event::Agents(AgentsEvent::Status { status, .. })) => {
                let done = status == want;
                seen.push(status);
                if done {
                    return seen;
                }
            }
            Some(_) => {}
            None => panic!("core closed while waiting for {want:?}"),
        }
    }
}

async fn transcript(core: &Core) -> Vec<AgentMessage> {
    match ok(
        core,
        json!({"method": "agents.transcript", "params": {"session": "s1"}}),
    )
    .await
    {
        Response::Agents(AgentsResponse::Transcript(t)) => t,
        other => panic!("unexpected {other:?}"),
    }
}

async fn pending_id(core: &Core) -> String {
    match ok(core, json!({"method": "permissions.pending"})).await {
        Response::Permissions(PermissionsResponse::Pending(p)) if p.len() == 1 => p[0].id.0.clone(),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_session_streams_asks_and_waits_for_the_answer() {
    let core = core_with(ScriptedAgent::new(vec![vec![
        Step::Say("Looking."),
        Step::Use {
            tool: "Bash",
            input: json!({"command": "ls"}),
        },
    ]]));
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("List the files\nthen stop")).await;

    let seen = until(&mut rx, AgentStatus::WaitingForApproval).await;
    assert_eq!(
        seen,
        [
            AgentStatus::Starting,
            AgentStatus::Working,
            AgentStatus::WaitingForApproval
        ]
    );
    // The request says where it comes from and which folder paths are judged against.
    match ok(&core, json!({"method": "permissions.pending"})).await {
        Response::Permissions(PermissionsResponse::Pending(p)) => {
            assert_eq!(p[0].project.0, "p1");
            assert_eq!(p[0].cwd.as_deref(), Some(crate::test_path!("/work/a")));
        }
        other => panic!("unexpected {other:?}"),
    }
    let id = pending_id(&core).await;
    ok(
        &core,
        json!({"method": "permissions.answer", "params": {"id": id, "decision": "allow"}}),
    )
    .await;
    assert_eq!(
        until(&mut rx, AgentStatus::Idle).await,
        [AgentStatus::Working, AgentStatus::Idle]
    );

    insta::assert_json_snapshot!(transcript(&core).await);
    match ok(
        &core,
        json!({"method": "agents.list", "params": {"project": "p1"}}),
    )
    .await
    {
        Response::Agents(AgentsResponse::List(list)) => {
            assert_eq!(list[0].title, "List the files");
            assert_eq!(list[0].status, AgentStatus::Idle);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_denied_tool_reports_the_reason_to_the_agent() {
    let core = core_with(ScriptedAgent::new(vec![vec![Step::Use {
        tool: "Edit",
        input: json!({"file_path": "a.rs"}),
    }]]));
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("edit")).await;
    until(&mut rx, AgentStatus::WaitingForApproval).await;
    let id = pending_id(&core).await;
    ok(
        &core,
        json!({"method": "permissions.answer",
        "params": {"id": id, "decision": "deny", "message": "not this file"}}),
    )
    .await;
    until(&mut rx, AgentStatus::Idle).await;
    let result = transcript(&core).await.into_iter().find_map(|m| match m {
        AgentMessage::ToolResult {
            output, is_error, ..
        } => Some((output, is_error)),
        _ => None,
    });
    assert_eq!(result, Some(("not this file".to_owned(), true)));
    let again = json!({"method": "permissions.answer", "params": {"id": id, "decision": "allow"}});
    assert_eq!(code(&core, again).await, ErrorCode::NotFound);
}

#[tokio::test]
async fn each_message_plays_the_next_turn_until_stopped() {
    let core = core_with(ScriptedAgent::new(vec![
        vec![Step::Say("one")],
        vec![Step::Say("two")],
    ]));
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("go")).await;
    until(&mut rx, AgentStatus::Idle).await;
    ok(
        &core,
        json!({"method": "agents.send", "params": {"session": "s1", "text": "more"}}),
    )
    .await;
    until(&mut rx, AgentStatus::Idle).await;
    ok(
        &core,
        json!({"method": "agents.stop", "params": {"session": "s1"}}),
    )
    .await;
    until(&mut rx, AgentStatus::Stopped).await;

    let said: Vec<String> = transcript(&core)
        .await
        .into_iter()
        .filter_map(|m| match m {
            AgentMessage::UserText { text, .. } | AgentMessage::AssistantText { text, .. } => {
                Some(text)
            }
            _ => None,
        })
        .collect();
    assert_eq!(said, ["go", "one", "more", "two"]);
    let send = json!({"method": "agents.send", "params": {"session": "s1", "text": "late"}});
    assert_eq!(code(&core, send).await, ErrorCode::Conflict);
}

#[tokio::test]
async fn one_direct_session_per_project() {
    let core = core_with(ScriptedAgent::new(vec![vec![Step::Fail("no model")]]));
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("first")).await;
    assert_eq!(code(&core, start("second")).await, ErrorCode::Conflict);
    until(
        &mut rx,
        AgentStatus::Failed {
            message: "no model".into(),
        },
    )
    .await;
    // The failed session no longer holds the main checkout.
    ok(&core, start("third")).await;

    let isolated = json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "isolated", "prompt": "x"}});
    assert_eq!(code(&core, isolated).await, ErrorCode::Unavailable);
    let unknown = json!({"method": "agents.start", "params": {
        "project": "nope", "agent": "claude", "mode": "direct", "prompt": "x"}});
    assert_eq!(code(&core, unknown).await, ErrorCode::NotFound);
}

#[tokio::test]
async fn an_ended_session_drops_its_permission_requests() {
    let core = core_with(ScriptedAgent::default());
    project(&core).await;
    ok(&core, start("x")).await;
    let host = host(&core);
    let mut rx = core.subscribe();
    let asking = tokio::spawn({
        let host = host.clone();
        async move { host.ask("Bash".into(), json!({}), None, None).await }
    });
    until(&mut rx, AgentStatus::WaitingForApproval).await;
    host.status(AgentStatus::Stopped);
    let answer = asking.await.unwrap_or_else(|e| panic!("ask panicked: {e}"));
    assert_eq!(answer.message.as_deref(), Some("the session ended"));
    let mut resolved = false;
    while let Ok(event) = rx.try_recv() {
        resolved |= matches!(event, Event::Permissions(PermissionsEvent::Resolved { .. }));
    }
    assert!(resolved, "the dropped request was not resolved");
}

/// A host for session s1 of p1, to report as a runtime would.
fn host(core: &Core) -> crate::agents::AgentHost {
    crate::agents::AgentHost::new(
        crate::protocol::agents::SessionId("s1".into()),
        crate::protocol::projects::ProjectId("p1".into()),
        core.inner_sessions(),
        core.sink(),
    )
}

async fn sessions(core: &Core) -> Vec<SessionInfo> {
    match ok(
        core,
        json!({"method": "agents.list", "params": {"project": "p1"}}),
    )
    .await
    {
        Response::Agents(AgentsResponse::List(list)) => list,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn stopping_a_waiting_session_denies_its_request() {
    let core = core_with(ScriptedAgent::new(vec![vec![
        Step::Use {
            tool: "Bash",
            input: json!({"command": "rm -rf build"}),
        },
        Step::Use {
            tool: "Bash",
            input: json!({"command": "make"}),
        },
    ]]));
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("clean")).await;
    until(&mut rx, AgentStatus::WaitingForApproval).await;
    let id = pending_id(&core).await;
    let stop = json!({"method": "agents.stop", "params": {"session": "s1"}});
    ok(&core, stop.clone()).await;
    let send = json!({"method": "agents.send", "params": {"session": "s1", "text": "more"}});
    assert_eq!(code(&core, send).await, ErrorCode::Conflict);
    until(&mut rx, AgentStatus::Stopped).await;

    let results: Vec<(String, bool)> = transcript(&core)
        .await
        .into_iter()
        .filter_map(|m| match m {
            AgentMessage::ToolResult {
                output, is_error, ..
            } => Some((output, is_error)),
            _ => None,
        })
        .collect();
    // The second tool was denied without asking anyone.
    let stopped = ("the session was stopped".to_owned(), true);
    assert_eq!(results, [stopped.clone(), stopped]);
    match ok(&core, json!({"method": "permissions.pending"})).await {
        Response::Permissions(PermissionsResponse::Pending(p)) => assert!(p.is_empty()),
        other => panic!("unexpected {other:?}"),
    }
    let answer = json!({"method": "permissions.answer", "params": {"id": id, "decision": "allow"}});
    assert_eq!(code(&core, answer).await, ErrorCode::NotFound);
    // Stopping again does nothing.
    ok(&core, stop).await;
}

#[tokio::test]
async fn an_interrupt_ends_the_turn_and_denies_the_waiting_tool() {
    let core = core_with(ScriptedAgent::new(vec![
        vec![
            Step::Use {
                tool: "Bash",
                input: json!({"command": "make"}),
            },
            Step::Say("built"),
        ],
        vec![Step::Say("next")],
    ]));
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("build")).await;
    until(&mut rx, AgentStatus::WaitingForApproval).await;
    let interrupt = json!({"method": "agents.interrupt", "params": {"session": "s1"}});
    ok(&core, interrupt.clone()).await;
    until(&mut rx, AgentStatus::Idle).await;
    // The session goes on with the next message.
    ok(
        &core,
        json!({"method": "agents.send", "params": {"session": "s1", "text": "go on"}}),
    )
    .await;
    until(&mut rx, AgentStatus::Idle).await;

    let texts: Vec<String> = transcript(&core)
        .await
        .into_iter()
        .filter_map(|m| match m {
            AgentMessage::AssistantText { text, .. } => Some(text),
            AgentMessage::ToolResult { output, .. } => Some(output),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["the turn was interrupted", "next"]);
    ok(
        &core,
        json!({"method": "agents.stop", "params": {"session": "s1"}}),
    )
    .await;
    until(&mut rx, AgentStatus::Stopped).await;
    assert_eq!(code(&core, interrupt).await, ErrorCode::Conflict);
}

#[tokio::test]
async fn the_state_machine_ignores_reports_that_make_no_sense() {
    let core = core_with(ScriptedAgent::default());
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("x")).await;
    until(&mut rx, AgentStatus::Idle).await;
    let host = host(&core);
    let asking = tokio::spawn({
        let host = host.clone();
        async move { host.ask("Bash".into(), json!({}), None, None).await }
    });
    until(&mut rx, AgentStatus::WaitingForApproval).await;
    // A request still waits: neither working nor idle shows.
    host.status(AgentStatus::Working);
    host.status(AgentStatus::Idle);
    assert_eq!(
        sessions(&core).await[0].status,
        AgentStatus::WaitingForApproval
    );
    let id = pending_id(&core).await;
    ok(
        &core,
        json!({"method": "permissions.answer", "params": {"id": id, "decision": "allow"}}),
    )
    .await;
    let answer = asking.await.unwrap_or_else(|e| panic!("ask panicked: {e}"));
    assert_eq!(answer, PermissionAnswer::allow());
    assert_eq!(sessions(&core).await[0].status, AgentStatus::Working);

    host.status(AgentStatus::Failed {
        message: "crashed".into(),
    });
    host.status(AgentStatus::Idle);
    host.status(AgentStatus::Stopped);
    let late = host.ask("Bash".into(), json!({}), None, None).await;
    assert_eq!(late.message.as_deref(), Some("the session ended"));

    let mut seen = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            Event::Agents(AgentsEvent::Status { status, .. }) => seen.push(status),
            Event::Permissions(PermissionsEvent::Requested(r)) => panic!("asked again: {r:?}"),
            _ => {}
        }
    }
    let failed = AgentStatus::Failed {
        message: "crashed".into(),
    };
    assert_eq!(seen, [AgentStatus::Working, failed.clone()]);
    assert_eq!(sessions(&core).await[0].status, failed);
}

#[tokio::test]
async fn a_request_the_agent_cancels_is_withdrawn() {
    let core = core_with(ScriptedAgent::default());
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("x")).await;
    until(&mut rx, AgentStatus::Idle).await;
    let host = host(&core);
    let asking = tokio::spawn({
        let host = host.clone();
        async move { host.ask("Bash".into(), json!({}), None, None).await }
    });
    until(&mut rx, AgentStatus::WaitingForApproval).await;
    asking.abort();
    let mut resolved = false;
    loop {
        match Core::next_event(&mut rx).await {
            Some(Event::Permissions(PermissionsEvent::Resolved { .. })) => resolved = true,
            Some(Event::Agents(AgentsEvent::Status { status, .. })) => {
                assert_eq!(status, AgentStatus::Working);
                break;
            }
            Some(_) => {}
            None => panic!("core closed"),
        }
    }
    assert!(resolved, "the withdrawn request was not resolved");
    match ok(&core, json!({"method": "permissions.pending"})).await {
        Response::Permissions(PermissionsResponse::Pending(p)) => assert!(p.is_empty()),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn sessions_add_up_their_usage() {
    let core = core_with(ScriptedAgent::default());
    project(&core).await;
    ok(&core, start("x")).await;
    let host = host(&core);
    for cost in [Some(1_500), None] {
        host.message(AgentMessage::TurnEnd {
            usage: TurnUsage {
                input_tokens: 100,
                output_tokens: 20,
                cost_micro_usd: cost,
            },
        });
    }
    insta::assert_json_snapshot!(sessions(&core).await[0].usage, @r#"
    {
      "input_tokens": 200,
      "output_tokens": 40,
      "cost_micro_usd": 1500
    }
    "#);
}

#[tokio::test]
async fn prompts_must_say_something_and_fit() {
    let core = core_with(ScriptedAgent::default());
    project(&core).await;
    assert_eq!(code(&core, start(" \n ")).await, ErrorCode::BadRequest);
    let long = "a".repeat(64 * 1024 + 1);
    assert_eq!(code(&core, start(&long)).await, ErrorCode::BadRequest);
    ok(&core, start("ok")).await;
    let empty = json!({"method": "agents.send", "params": {"session": "s1", "text": ""}});
    assert_eq!(code(&core, empty).await, ErrorCode::BadRequest);
}

#[tokio::test]
async fn removing_a_project_stops_its_sessions() {
    let core = core_with(ScriptedAgent::default());
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("x")).await;
    until(&mut rx, AgentStatus::Idle).await;
    ok(
        &core,
        json!({"method": "projects.remove", "params": {"id": "p1"}}),
    )
    .await;
    until(&mut rx, AgentStatus::Stopped).await;
}

#[tokio::test]
async fn old_ended_sessions_are_forgotten() {
    // Without a runtime every start fails, so each session ends at once.
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .build();
    project(&core).await;
    for _ in 0..25 {
        assert_eq!(code(&core, start("x")).await, ErrorCode::Unavailable);
    }
    let ids: Vec<String> = sessions(&core).await.into_iter().map(|s| s.id.0).collect();
    // Twenty kept when the last one started, plus the last one.
    let kept: Vec<String> = (5..=25).map(|n| format!("s{n}")).collect();
    assert_eq!(ids, kept);
}

#[tokio::test]
async fn terminals_open_in_the_project_root() {
    let core = core_with(ScriptedAgent::default());
    project(&core).await;
    let mut rx = core.subscribe();
    let opened = ok(
        &core,
        json!({"method": "terminals.open",
        "params": {"project": "p1", "size": {"cols": 80, "rows": 24}}}),
    )
    .await;
    insta::assert_json_snapshot!(opened, @r#"
    {
      "type": "terminals.opened",
      "data": {
        "id": "t1"
      }
    }
    "#);
    ok(
        &core,
        json!({"method": "terminals.input", "params": {"id": "t1", "data": "ls\r"}}),
    )
    .await;
    let scrollback = ok(
        &core,
        json!({"method": "terminals.scrollback", "params": {"id": "t1"}}),
    )
    .await;
    assert_eq!(
        scrollback,
        Response::Terminals(TerminalsResponse::Scrollback {
            data: "ls\r".into(),
            seq: 1
        })
    );
    ok(
        &core,
        json!({"method": "terminals.close", "params": {"id": "t1"}}),
    )
    .await;
    let mut events = Vec::new();
    while let Ok(Event::Terminals(e)) = rx.try_recv() {
        events.push(e);
    }
    assert!(
        matches!(&events[..], [TerminalsEvent::Output { data, seq: 1, .. }, TerminalsEvent::Exited { .. }] if data == "ls\r")
    );
    let input = json!({"method": "terminals.input", "params": {"id": "t1", "data": "x"}});
    assert_eq!(code(&core, input).await, ErrorCode::NotFound);
}

#[tokio::test]
async fn setup_checks_the_saved_path_and_only_absolute_paths_are_saved() {
    let core = core_with(ScriptedAgent::default());
    let setup = |core: &Core| {
        let core = core.clone();
        async move {
            match ok(
                &core,
                json!({"method": "agents.setup", "params": {"agent": "claude"}}),
            )
            .await
            {
                Response::Agents(AgentsResponse::Setup(setup)) => setup,
                other => panic!("unexpected {other:?}"),
            }
        }
    };
    assert_eq!(setup(&core).await.path, None);
    let set = |path: serde_json::Value| json!({"method": "agents.set_path", "params": {"agent": "claude", "path": path}});
    assert_eq!(
        code(&core, set(json!("bin/claude"))).await,
        ErrorCode::BadRequest
    );
    ok(&core, set(json!(crate::test_path!("/opt/claude")))).await;
    let saved = setup(&core).await;
    assert_eq!(
        saved.path.as_deref(),
        Some(crate::test_path!("/opt/claude"))
    );
    assert_eq!(saved.state, SetupState::Ready);
    // An empty field clears the path.
    ok(&core, set(json!("  "))).await;
    assert_eq!(setup(&core).await.path, None);
}

fn config(fingerprint: &str) -> ProjectAgentConfig {
    ProjectAgentConfig {
        fingerprint: fingerprint.into(),
        items: Vec::new(),
    }
}

fn trust(fingerprint: Option<&str>) -> serde_json::Value {
    json!({"method": "projects.trust", "params": {
        "id": "p1", "trusted": fingerprint.is_some(), "fingerprint": fingerprint}})
}

async fn review(core: &Core) -> ProjectConfigReview {
    match ok(
        core,
        json!({"method": "agents.project_config", "params": {"project": "p1"}}),
    )
    .await
    {
        Response::Agents(AgentsResponse::ProjectConfig(review)) => review,
        other => panic!("unexpected {other:?}"),
    }
}

/// Starts a session, waits until it is idle, stops it and returns whether
/// it was started as trusted.
async fn run_once(core: &Core, agent: &ScriptedAgent, rx: &mut broadcast::Receiver<Event>) -> bool {
    ok(core, start("go")).await;
    until(rx, AgentStatus::Idle).await;
    let id = format!(
        "s{}",
        agent.started.lock().map(|s| s.len()).unwrap_or_default()
    );
    ok(
        core,
        json!({"method": "agents.stop", "params": {"session": id}}),
    )
    .await;
    until(rx, AgentStatus::Stopped).await;
    let started = agent.started.lock().map(|s| s.clone()).unwrap_or_default();
    started.last().is_some_and(|s| s.trusted)
}

#[tokio::test]
async fn sessions_get_the_chosen_executable_and_the_project_trust() {
    let agent = Arc::new(ScriptedAgent::new(vec![vec![]]));
    *agent
        .config
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = config("a");
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(agent.clone())
        .build();
    project(&core).await;
    let mut rx = core.subscribe();
    ok(
        &core,
        json!({"method": "agents.set_path", "params": {"agent": "claude", "path": crate::test_path!("/opt/claude")}}),
    )
    .await;
    assert!(!run_once(&core, &agent, &mut rx).await);
    assert_eq!(review(&core).await.trusted, None);

    // Trusting needs what was reviewed, and only that.
    assert_eq!(
        code(
            &core,
            json!({"method": "projects.trust", "params": {"id": "p1", "trusted": true}})
        )
        .await,
        ErrorCode::BadRequest
    );
    assert_eq!(code(&core, trust(Some("old"))).await, ErrorCode::Conflict);
    let trusted = ok(&core, trust(Some("a"))).await;
    assert!(matches!(
        trusted,
        Response::Projects(ProjectsResponse::Project(Project { trusted: true, .. }))
    ));
    assert!(matches!(
        Core::next_event(&mut rx).await,
        Some(Event::Projects(ProjectsEvent::Changed { .. }))
    ));
    assert!(review(&core).await.applies());
    assert!(run_once(&core, &agent, &mut rx).await);

    // Changed settings (a git pull) do not load until trusted again.
    *agent
        .config
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = config("b");
    let changed = review(&core).await;
    assert!(!changed.applies());
    assert_eq!(changed.trusted, Some(config("a")));
    assert!(!run_once(&core, &agent, &mut rx).await);
    ok(&core, trust(Some("b"))).await;
    assert!(run_once(&core, &agent, &mut rx).await);

    ok(&core, trust(None)).await;
    assert!(!run_once(&core, &agent, &mut rx).await);
    let started = agent.started.lock().map(|s| s.clone()).unwrap_or_default();
    let opt = Some(std::path::PathBuf::from(crate::test_path!("/opt/claude")));
    assert!(started.iter().all(|s| s.executable == opt));
    assert_eq!(
        code(
            &core,
            json!({"method": "projects.trust", "params": {"id": "p9", "trusted": false}})
        )
        .await,
        ErrorCode::NotFound
    );
}

/// Two cores over the same projects and sessions: a run, then a restart.
fn restartable(
    agent: ScriptedAgent,
    projects: &Arc<MemoryProjects>,
    kept: &Arc<MemorySessions>,
) -> Core {
    Core::builder("test")
        .projects(projects.clone())
        .sessions(kept.clone())
        .agents(Arc::new(agent))
        .build()
}

#[tokio::test]
async fn sessions_survive_a_restart_and_resume() {
    let projects = Arc::new(MemoryProjects::default());
    let kept = Arc::new(MemorySessions::default());
    let first = restartable(
        ScriptedAgent::new(vec![vec![Step::Say("one")]]),
        &projects,
        &kept,
    );
    project(&first).await;
    let mut rx = first.subscribe();
    ok(&first, start("go")).await;
    until(&mut rx, AgentStatus::Idle).await;
    host(&first).message(AgentMessage::TurnEnd {
        usage: TurnUsage {
            input_tokens: 7,
            output_tokens: 3,
            cost_micro_usd: Some(40),
        },
    });
    // A status change saves the summed usage, as after every real turn.
    host(&first).status(AgentStatus::Working);
    assert!(
        !sessions(&first).await[0].resumable,
        "a running session is not resumable"
    );
    drop(first);

    let agent = Arc::new(ScriptedAgent::new(vec![vec![Step::Say("two")]]));
    let second = Core::builder("test")
        .projects(projects)
        .sessions(kept)
        .agents(agent.clone())
        .build();
    // It ran when the core quit, so it shows as stopped and can resume.
    let before = sessions(&second).await;
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].status, AgentStatus::Stopped);
    assert!(before[0].resumable);
    assert_eq!(before[0].usage.cost_micro_usd, Some(40));
    let said = |t: Vec<AgentMessage>| -> Vec<String> {
        t.into_iter()
            .filter_map(|m| match m {
                AgentMessage::UserText { text, .. } | AgentMessage::AssistantText { text, .. } => {
                    Some(text)
                }
                _ => None,
            })
            .collect()
    };
    assert_eq!(said(transcript(&second).await), ["go", "one"]);

    let mut rx = second.subscribe();
    let resume = json!({"method": "agents.resume", "params": {"session": "s1", "prompt": "again"}});
    ok(&second, resume.clone()).await;
    until(&mut rx, AgentStatus::Idle).await;
    assert_eq!(
        said(transcript(&second).await),
        ["go", "one", "again", "two"]
    );
    let resumed = agent.started.lock().unwrap_or_else(|e| e.into_inner())[0]
        .resume
        .clone();
    assert_eq!(resumed.as_deref(), Some("scripted-s1"));
    // Running again: neither a second resume nor a new direct session.
    assert_eq!(code(&second, resume).await, ErrorCode::Conflict);
    ok(
        &second,
        json!({"method": "agents.stop", "params": {"session": "s1"}}),
    )
    .await;
    until(&mut rx, AgentStatus::Stopped).await;
    // New sessions do not reuse kept ids.
    match ok(&second, start("new")).await {
        Response::Agents(AgentsResponse::Session(info)) => assert_eq!(info.id.0, "s2"),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_session_without_the_agents_id_cannot_resume() {
    let projects = Arc::new(MemoryProjects::default());
    let kept = Arc::new(MemorySessions::default());
    // Without a runtime the start fails before any agent reports an id.
    let core = Core::builder("test")
        .projects(projects.clone())
        .sessions(kept.clone())
        .build();
    project(&core).await;
    assert_eq!(code(&core, start("x")).await, ErrorCode::Unavailable);
    assert!(!sessions(&core).await[0].resumable);
    let resume = json!({"method": "agents.resume", "params": {"session": "s1", "prompt": "again"}});
    assert_eq!(code(&core, resume).await, ErrorCode::Conflict);
    let unknown =
        json!({"method": "agents.resume", "params": {"session": "s9", "prompt": "again"}});
    assert_eq!(code(&core, unknown).await, ErrorCode::NotFound);
}

/// Answers every request with who called and what.
#[derive(Debug)]
struct WhoTools;

impl crate::services::IdeTools for WhoTools {
    fn handle(
        &self,
        core: Core,
        caller: crate::services::ToolCaller,
        message: serde_json::Value,
    ) -> crate::services::BoxFuture<'_, Option<serde_json::Value>> {
        Box::pin(async move {
            let id = message.get("id")?.clone();
            let mode = match core
                .handle(serde_json::from_value(json!({"method": "info.get"})).ok()?)
                .await
            {
                Ok(Response::Info(crate::protocol::info::InfoResponse::Info(info))) => info.mode,
                _ => String::new(),
            };
            Some(
                json!({"id": id, "session": caller.session.0, "role": format!("{:?}", caller.role), "mode": mode}),
            )
        })
    }
}

#[tokio::test]
async fn ide_tool_calls_reach_the_tools_with_the_caller() {
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .ide_tools(Arc::new(WhoTools))
        .build();
    let host = crate::agents::AgentHost::new(
        crate::protocol::agents::SessionId("s7".into()),
        crate::protocol::projects::ProjectId("p1".into()),
        core.inner_sessions(),
        core.sink(),
    );
    let role = crate::services::ToolRole::Worker;
    let reply = host
        .call_ide(
            role,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}),
        )
        .await;
    assert_eq!(
        reply,
        Some(json!({"id": 3, "session": "s7", "role": "Worker", "mode": "test"}))
    );
    let notification = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    assert_eq!(host.call_ide(role, notification).await, None);
    // Without wired tools, a request gets a JSON-RPC error instead of silence.
    let bare = Core::new("test");
    let host = crate::agents::AgentHost::new(
        crate::protocol::agents::SessionId("s1".into()),
        crate::protocol::projects::ProjectId("p1".into()),
        bare.inner_sessions(),
        bare.sink(),
    );
    let reply = host
        .call_ide(
            role,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        )
        .await;
    assert_eq!(
        reply.and_then(|r| r["error"]["code"].as_i64()),
        Some(-32601)
    );
}

#[tokio::test]
async fn a_session_says_who_pays_once_the_agent_tells() {
    let core = core_with(ScriptedAgent::new(vec![vec![Step::Say("hi")]]));
    project(&core).await;
    ok(&core, start("x")).await;
    assert_eq!(sessions(&core).await[0].billing, None);
    let mut rx = core.subscribe();
    let cloud = crate::protocol::agents::Billing::Cloud {
        provider: "bedrock".into(),
    };
    host(&core).billing(cloud.clone());
    host(&core).billing(cloud.clone());
    assert_eq!(sessions(&core).await[0].billing, Some(cloud));
    let mut told = 0;
    while let Ok(event) = rx.try_recv() {
        told += usize::from(matches!(event, Event::Agents(AgentsEvent::Billing { .. })));
    }
    assert_eq!(told, 1, "the same answer again is no news");
}

#[tokio::test]
async fn the_projects_policy_decides_before_the_person_is_asked() {
    use crate::protocol::agents::{SandboxNetwork, SandboxReport};
    use crate::protocol::permissions::Decision;

    let core = core_with(ScriptedAgent::new(vec![vec![Step::Say("hi")]]));
    project(&core).await;
    ok(&core, start("x")).await;
    ok(
        &core,
        json!({"method": "policy.set", "params": {"project": "p1", "policy": {"rules": [
            {"tool": "WebFetch", "action": "deny"},
            {"tool": "Bash", "match": "npm test", "action": "auto"}]}}}),
    )
    .await;
    let host = host(&core);
    let fetch = host
        .ask("WebFetch".into(), json!({"url": "x"}), None, None)
        .await;
    assert_eq!(fetch.decision, Decision::Deny);
    assert!(fetch.message.is_some_and(|m| m.contains("(WebFetch)")));
    // The hook refuses it too, for calls the user's settings let through.
    let hook = json!({"tool_use_id": "t1", "tool_name": "WebFetch", "tool_input": {"url": "x"}});
    assert!(host.before_tool(&hook).await.is_some());
    let bash = json!({"tool_use_id": "t2", "tool_name": "Bash", "tool_input": {"command": "ls"}});
    assert_eq!(host.before_tool(&bash).await, None);

    // Auto counts only once the runtime said it ran a sandbox.
    let mut rx = core.subscribe();
    let asking = tokio::spawn({
        let host = host.clone();
        async move {
            host.ask("Bash".into(), json!({"command": "npm test"}), None, None)
                .await
        }
    });
    until(&mut rx, AgentStatus::WaitingForApproval).await;
    host.status(AgentStatus::Stopped);
    let unsandboxed = asking.await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(unsandboxed.message.as_deref(), Some("the session ended"));

    ok(&core, start("y")).await;
    let second = crate::agents::AgentHost::new(
        crate::protocol::agents::SessionId("s2".into()),
        crate::protocol::projects::ProjectId("p1".into()),
        core.inner_sessions(),
        core.sink(),
    );
    second.sandboxed(Some(SandboxReport {
        name: "test".into(),
        network: SandboxNetwork::Proxy,
    }));
    assert_eq!(
        sessions(&core).await[1]
            .sandbox
            .as_ref()
            .map(|s| s.name.as_str()),
        Some("test")
    );
    let test = second.ask("Bash".into(), json!({"command": "npm test"}), None, None);
    assert_eq!(test.await.decision, Decision::Allow);
}
