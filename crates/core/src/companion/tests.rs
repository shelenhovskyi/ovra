use std::sync::Arc;

use serde_json::json;

use super::{allows, shows};
use crate::Core;
use crate::fakes::{MemoryAudit, MemoryProjects, ScriptedAgent, Step};
use crate::protocol::agents::{AgentStatus, AgentsEvent, SessionId};
use crate::protocol::audit::{AuditEvent, DecidedBy};
use crate::protocol::fs::FsEvent;
use crate::protocol::permissions::PermissionsResponse;
use crate::protocol::projects::ProjectId;
use crate::protocol::review::{ReviewEvent, ReviewId};
use crate::protocol::{ErrorCode, Event, Request, Response};
use crate::services::AuditLog;

fn request(value: serde_json::Value) -> Request {
    serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{value}: {e}"))
}

#[test]
fn a_phone_reads_answers_replies_and_stops_but_changes_nothing_else() {
    let s = json!({"session": "s1"});
    let p = json!({"project": "p1"});
    let allowed = [
        json!({"method": "info.get"}),
        json!({"method": "projects.list"}),
        json!({"method": "agents.list", "params": p}),
        json!({"method": "agents.transcript", "params": s}),
        json!({"method": "agents.capacity", "params": p}),
        json!({"method": "agents.limit"}),
        json!({"method": "agents.send", "params": {"session": "s1", "text": "go on"}}),
        json!({"method": "agents.queue", "params": {"session": "s1", "text": "then this"}}),
        json!({"method": "agents.unqueue", "params": {"session": "s1", "id": 1}}),
        json!({"method": "agents.interrupt", "params": s}),
        json!({"method": "agents.stop", "params": s}),
        json!({"method": "permissions.pending"}),
        json!({"method": "permissions.answer", "params": {"id": "r1", "decision": "allow"}}),
        json!({"method": "permissions.answer", "params": {"id": "r1", "decision": "deny", "message": "no"}}),
        json!({"method": "orchestration.list", "params": p}),
        json!({"method": "review.list", "params": p}),
        json!({"method": "power.get"}),
        json!({"method": "power.set_mode", "params": {"mode": "always"}}),
    ];
    for value in allowed {
        assert!(allows(&request(value.clone())), "{value} should be allowed");
    }
    let refused = [
        json!({"method": "permissions.answer", "params": {"id": "r1", "decision": "allow", "always": true}}),
        json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "x"}}),
        json!({"method": "agents.resume", "params": {"session": "s1", "prompt": "x"}}),
        json!({"method": "agents.send", "params": {"session": "s1", "text": "go on",
            "context": [{"kind": "terminal", "title": "zsh", "text": "x", "confirmed": true}]}}),
        json!({"method": "agents.send", "params": {"session": "s1", "text": "go on",
            "context": [{"kind": "file", "path": ".env", "confirmed": true}]}}),
        json!({"method": "agents.queue", "params": {"session": "s1", "text": "then",
            "context": [{"kind": "file", "path": ".env", "confirmed": true}]}}),
        // It lets the agent change files, like Keep.
        json!({"method": "agents.approve_plan", "params": s}),
        json!({"method": "agents.set_limit", "params": {"limit": 8}}),
        json!({"method": "projects.add", "params": {"root": "/"}}),
        json!({"method": "projects.trust", "params": {"id": "p1", "trusted": true}}),
        json!({"method": "fs.read_file", "params": {"project": "p1", "path": "a"}}),
        json!({"method": "terminals.open", "params": {"project": "p1", "size": {"cols": 80, "rows": 24}}}),
        json!({"method": "policy.get", "params": p}),
        json!({"method": "secrets.list", "params": p}),
        json!({"method": "netproxy.get_allowlist", "params": p}),
        json!({"method": "review.diff", "params": {"id": "v1", "path": "a"}}),
        json!({"method": "review.reject", "params": {"id": "v1"}}),
        json!({"method": "review.run_check", "params": {"id": "v1"}}),
        json!({"method": "review.warnings", "params": {"id": "v1"}}),
        json!({"method": "review.update", "params": {"id": "v1"}}),
        json!({"method": "orchestration.start", "params": {"project": "p1", "prompt": "x"}}),
        json!({"method": "audit.list", "params": p}),
    ];
    for value in refused {
        assert!(
            !allows(&request(value.clone())),
            "{value} should be refused"
        );
    }
}

#[test]
fn a_phone_gets_status_and_approvals_but_no_files_or_streamed_text() {
    let status = Event::from(AgentsEvent::Status {
        session: SessionId("s1".into()),
        project: ProjectId("p1".into()),
        status: AgentStatus::Working,
    });
    assert!(shows(&status));
    assert!(!shows(&Event::from(AgentsEvent::TextDelta {
        session: SessionId("s1".into()),
        text: "secret".into(),
    })));
    // A phone hears of a review request by push and re-reads on review.changed.
    assert!(!shows(&Event::Review(ReviewEvent::Requested {
        project: ProjectId("p1".into()),
        id: ReviewId("r1".into()),
    })));
    assert!(!shows(&Event::from(FsEvent::GitChanged {
        project: ProjectId("p1".into()),
    })));
}

#[test]
fn a_phone_client_filters_events_and_a_window_client_does_not() {
    let core = Core::new("test");
    let text = Event::from(AgentsEvent::TextDelta {
        session: SessionId("s1".into()),
        text: "x".into(),
    });
    assert!(!core.connect_phone("Pixel").shows(&text));
    assert!(core.connect().shows(&text));
}

/// A core whose agent asks to run `ls` in project `p1`, and a phone
/// "Pixel"; returns once the request waits. A phone may not add projects.
async fn waiting() -> (Core, crate::Client, Arc<MemoryAudit>) {
    let log = Arc::new(MemoryAudit::default());
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(ScriptedAgent::new(vec![vec![Step::Use {
            tool: "Bash",
            input: json!({"command": "ls"}),
        }]])))
        .audit(log.clone())
        .build();
    let phone = core.connect_phone("Pixel");
    let add = request(
        json!({"method": "projects.add", "params": {"root": crate::test_path!("/work/a")}}),
    );
    let refused = phone.handle(add.clone()).await.err().map(|e| e.code);
    assert_eq!(refused, Some(ErrorCode::Forbidden));
    core.handle(add).await.unwrap_or_else(|e| panic!("{e}"));
    let mut rx = core.subscribe();
    core.handle(request(json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "direct", "prompt": "x"}})))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    loop {
        match Core::next_event(&mut rx).await {
            Some(Event::Agents(AgentsEvent::Status {
                status: AgentStatus::WaitingForApproval,
                ..
            })) => break,
            Some(_) => {}
            None => panic!("core closed"),
        }
    }
    (core, phone, log)
}

/// Waits until the log has a tool decision and returns who made it.
async fn decided_by(log: &MemoryAudit) -> DecidedBy {
    loop {
        let decided = log
            .list(&ProjectId("p1".into()), Default::default(), 100)
            .unwrap_or_else(|e| panic!("{e}"))
            .into_iter()
            .find_map(|entry| match entry.event {
                AuditEvent::ToolDecided { by, .. } => Some(by),
                _ => None,
            });
        if let Some(by) = decided {
            return by;
        }
        tokio::task::yield_now().await;
    }
}

fn pixel() -> DecidedBy {
    DecidedBy::Phone {
        device: "Pixel".into(),
    }
}

#[tokio::test]
async fn a_phone_answers_without_always_and_its_answers_are_recorded_as_its_own() {
    let (_core, phone, log) = waiting().await;
    let id = match phone
        .handle(request(json!({"method": "permissions.pending"})))
        .await
    {
        Ok(Response::Permissions(PermissionsResponse::Pending(p))) => p[0].id.0.clone(),
        other => panic!("unexpected {other:?}"),
    };
    let always = request(json!({"method": "permissions.answer", "params": {
        "id": id, "decision": "allow", "always": true}}));
    assert_eq!(
        phone.handle(always).await.err().map(|e| e.code),
        Some(ErrorCode::Forbidden)
    );
    phone
        .handle(request(
            json!({"method": "permissions.answer", "params": {"id": id, "decision": "allow"}}),
        ))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(decided_by(&log).await, pixel());
}

#[tokio::test]
async fn what_a_phone_interrupt_refuses_is_recorded_as_the_phones() {
    let (_core, phone, log) = waiting().await;
    phone
        .handle(request(
            json!({"method": "agents.interrupt", "params": {"session": "s1"}}),
        ))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(decided_by(&log).await, pixel());
}
