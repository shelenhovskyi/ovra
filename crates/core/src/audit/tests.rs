use std::sync::Arc;

use crate::Core;
use crate::fakes::MemoryAudit;
use crate::protocol::audit::{
    AuditEntry, AuditEvent, AuditInput, AuditLogEvent, AuditRequest, AuditResponse, DecidedBy,
    MAX_AUDIT_PAGE,
};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, Event, Request, Response};
use crate::services::AuditLog;

fn project(id: &str) -> ProjectId {
    ProjectId(id.into())
}

fn trust(trusted: bool) -> AuditEvent {
    AuditEvent::TrustChanged {
        trusted,
        fingerprint: None,
    }
}

async fn list(core: &Core, after: Option<u64>, limit: Option<usize>) -> Vec<AuditEntry> {
    let request = Request::Audit(AuditRequest::List {
        project: project("p1"),
        after,
        limit,
    });
    match core.handle(request).await {
        Ok(Response::Audit(AuditResponse::Entries(entries))) => entries,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn list_pages_through_one_projects_entries() {
    let log = Arc::new(MemoryAudit::default());
    for (p, trusted) in [("p1", true), ("p2", true), ("p1", false), ("p1", true)] {
        log.append(&project(p), None, trust(trusted))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let core = Core::builder("test").audit(log).build();
    let seqs = |entries: Vec<AuditEntry>| entries.iter().map(|e| e.seq).collect::<Vec<_>>();
    assert_eq!(seqs(list(&core, None, None).await), [1, 3, 4]);
    assert_eq!(seqs(list(&core, Some(1), Some(1)).await), [3]);
    assert_eq!(seqs(list(&core, Some(4), None).await), Vec::<u64>::new());
}

#[tokio::test]
async fn list_never_returns_more_than_a_page() {
    let log = Arc::new(MemoryAudit::default());
    for _ in 0..=MAX_AUDIT_PAGE {
        log.append(&project("p1"), None, trust(true))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let core = Core::builder("test").audit(log).build();
    assert_eq!(
        list(&core, None, Some(usize::MAX)).await.len(),
        MAX_AUDIT_PAGE
    );
}

#[tokio::test]
async fn without_a_log_list_is_unavailable() {
    let request = Request::Audit(AuditRequest::List {
        project: project("p1"),
        after: None,
        limit: None,
    });
    let err = Core::new("test").handle(request).await.err();
    assert_eq!(err.map(|e| e.code), Some(ErrorCode::Unavailable));
}

#[test]
fn wire_format_is_stable() {
    let entry = AuditEntry {
        seq: 7,
        at_ms: 1_700_000_000_000,
        project: project("p1"),
        session: Some(crate::protocol::agents::SessionId("s1".into())),
        event: AuditEvent::ToolDecided {
            tool: "Bash".into(),
            input: AuditInput::Json {
                text: r#"{"command":"ls"}"#.into(),
            },
            allowed: true,
            by: DecidedBy::Person,
        },
    };
    insta::assert_json_snapshot!(serde_json::json!({
        "request": Request::Audit(AuditRequest::List { project: project("p1"), after: Some(6), limit: Some(100) }),
        "response": Response::Audit(AuditResponse::Entries(vec![entry.clone()])),
        "event": Event::from(AuditLogEvent::Appended(entry)),
    }));
}
