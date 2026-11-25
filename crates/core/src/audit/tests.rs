use std::sync::Arc;

use serde_json::json;

use crate::Core;
use crate::fakes::MemoryAudit;
use crate::protocol::audit::{
    AuditEntry, AuditEvent, AuditInput, AuditLogEvent, AuditOrder, AuditRequest, AuditResponse,
    DecidedBy, MAX_AUDIT_PAGE,
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

/// `audit.list` for `p1`, with `params` as the client sends them.
async fn list(core: &Core, mut params: serde_json::Value) -> Vec<AuditEntry> {
    params["project"] = "p1".into();
    let request =
        serde_json::from_value(serde_json::json!({"method": "audit.list", "params": params}))
            .unwrap_or_else(|e| panic!("bad request: {e}"));
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
    assert_eq!(seqs(list(&core, json!({})).await), [1, 3, 4]);
    assert_eq!(
        seqs(list(&core, json!({"after": 1, "limit": 1})).await),
        [3]
    );
    assert_eq!(
        seqs(list(&core, json!({"after": 4})).await),
        Vec::<u64>::new()
    );
}

#[tokio::test]
async fn list_pages_back_from_the_newest_entry() {
    let log = Arc::new(MemoryAudit::default());
    for (p, trusted) in [("p1", true), ("p2", true), ("p1", false), ("p1", true)] {
        log.append(&project(p), None, trust(trusted))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let core = Core::builder("test").audit(log).build();
    let seqs = |entries: Vec<AuditEntry>| entries.iter().map(|e| e.seq).collect::<Vec<_>>();
    let newest = |params: serde_json::Value| {
        let mut params = params;
        params["order"] = "newest_first".into();
        params
    };
    assert_eq!(seqs(list(&core, newest(json!({"limit": 2}))).await), [4, 3]);
    assert_eq!(seqs(list(&core, newest(json!({"before": 3}))).await), [1]);
    assert_eq!(
        seqs(list(&core, newest(json!({"before": 1}))).await),
        Vec::<u64>::new()
    );
    // `before` bounds an oldest-first page too.
    assert_eq!(seqs(list(&core, json!({"before": 4})).await), [1, 3]);
}

#[tokio::test]
async fn list_never_returns_more_than_a_page() {
    let log = Arc::new(MemoryAudit::default());
    for _ in 0..=MAX_AUDIT_PAGE {
        log.append(&project("p1"), None, trust(true))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let core = Core::builder("test").audit(log).build();
    for order in ["oldest_first", "newest_first"] {
        let page = list(&core, json!({"limit": usize::MAX, "order": order})).await;
        assert_eq!(page.len(), MAX_AUDIT_PAGE);
    }
}

#[tokio::test]
async fn without_a_log_list_is_unavailable() {
    let request = Request::Audit(AuditRequest::List {
        project: project("p1"),
        after: None,
        before: None,
        order: AuditOrder::default(),
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
        "request": Request::Audit(AuditRequest::List { project: project("p1"), after: Some(6), before: None, order: AuditOrder::default(), limit: Some(100) }),
        "newest": Request::Audit(AuditRequest::List { project: project("p1"), after: None, before: Some(9), order: AuditOrder::NewestFirst, limit: None }),
        "response": Response::Audit(AuditResponse::Entries(vec![entry.clone()])),
        "event": Event::from(AuditLogEvent::Appended(entry)),
    }));
}

#[test]
fn a_failed_append_warns_without_the_entrys_content() {
    #[derive(Clone, Default)]
    struct Captured(Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let event = AuditEvent::NetworkRefused {
        host: "evil.example".into(),
        reason: "not allowed".into(),
    };
    // Without a log wired in, every append fails.
    tracing::subscriber::with_default(subscriber, || {
        super::record(&Core::new("test"), &project("p1"), None, event);
    });
    let logged = String::from_utf8_lossy(
        &captured
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
    .into_owned();
    assert!(logged.contains("WARN"), "{logged}");
    assert!(logged.contains("network_refused"), "{logged}");
    assert!(!logged.contains("evil.example"), "{logged}");
    assert!(!logged.contains("not allowed"), "{logged}");
}

mod inputs {
    use serde_json::json;

    use super::super::input::{MAX_INPUT_BYTES, REDACTED, cut_name, input};
    use crate::protocol::audit::AuditInput;
    use crate::protocol::secrets::SecretValue;

    fn text(tool: &str, value: serde_json::Value) -> String {
        match input(tool, &value, Some(&[])) {
            AuditInput::Json { text } => text,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn file_tools_keep_the_path_and_a_hash_never_the_content() {
        let kept = input(
            "Write",
            &json!({"file_path": "src/a.rs", "content": "ghp_secretsecretsecret"}),
            Some(&[]),
        );
        assert_eq!(
            kept,
            AuditInput::File {
                path: "src/a.rs".into(),
                sha256: Some(sha("ghp_secretsecretsecret")),
            }
        );
        let edit = input(
            "Edit",
            &json!({"file_path": "a", "old_string": "x", "new_string": "y"}),
            Some(&[]),
        );
        assert_eq!(
            edit,
            AuditInput::File {
                path: "a".into(),
                sha256: Some(sha("y"))
            }
        );
        let notebook = input(
            "NotebookEdit",
            &json!({"notebook_path": "n.ipynb"}),
            Some(&[]),
        );
        assert_eq!(
            notebook,
            AuditInput::File {
                path: "n.ipynb".into(),
                sha256: None
            }
        );
    }

    fn sha(text: &str) -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(text.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn token_like_words_are_removed() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl";
        for secret in [
            "sk-ant-api03-abcdefghijkl",
            "ghp_0123456789abcdefABCDEF",
            "gho_0123456789abcdef",
            "github_pat_11AAAAAAA0123456789",
            "xoxb-123456789012-abcdef",
            "AKIAIOSFODNN7EXAMPLE",
            jwt,
        ] {
            let kept = text(
                "Bash",
                json!({"command": format!("deploy --token={secret} && echo {secret}.")}),
            );
            assert!(!kept.contains(secret), "{kept}");
            assert_eq!(kept.matches(REDACTED).count(), 2, "{kept}");
        }
        // Words that only start like a token stay.
        let kept = text(
            "Bash",
            json!({"command": "pip install sk-learn task-sk-12345678"}),
        );
        assert!(kept.contains("sk-learn task-sk-12345678"), "{kept}");
    }

    #[test]
    fn header_values_and_credential_fields_are_removed() {
        let kept = text(
            "Bash",
            json!({"command": "curl -H 'Authorization: Bearer abc.def' -H \"Cookie: id=42; s=1\" -H 'X-Other: fine' https://x"}),
        );
        assert_eq!(
            kept,
            json!({"command": format!("curl -H 'Authorization: {REDACTED}' -H \"Cookie: {REDACTED}\" -H 'X-Other: fine' https://x")}).to_string()
        );
        let kept = text(
            "WebFetch",
            json!({"url": "https://x", "headers": {"authorization": "Basic Zm9v", "Set-Cookie": "a=b", "password": 7, "client_secret": ["s"], "accept": "json"}}),
        );
        for gone in ["Basic", "Zm9v", "a=b", "7", "\"s\""] {
            assert!(!kept.contains(gone), "{gone} in {kept}");
        }
        assert!(kept.contains(r#""accept":"json""#), "{kept}");
        // Quoted names and values, as in code, httpie and JSON bodies.
        for command in [
            r#"python -c 'requests.get(u, headers={"Authorization": "Bearer abc123"})'"#,
            r#"http GET u Authorization:"Bearer abc123""#,
            r#"curl -H 'Cookie: "abc123"' u"#,
            r#"curl -H "authorization = Basic abc123" u"#,
        ] {
            let kept = text("Bash", json!({ "command": command }));
            assert!(!kept.contains("abc123"), "{kept}");
            assert!(kept.contains(REDACTED), "{kept}");
        }
        // A word that only contains a name is left alone.
        let kept = text("Bash", json!({"command": "rm -rf cookies: none"}));
        assert!(kept.contains("cookies: none"), "{kept}");
        let kept = text(
            "Task",
            json!({"accessToken": "a1", "x-api-key": "a2", "privateKey": "a3", "max_tokens": 5}),
        );
        for gone in ["a1", "a2", "a3"] {
            assert!(!kept.contains(gone), "{gone} in {kept}");
        }
        assert!(kept.contains(r#""max_tokens":5"#), "{kept}");
    }

    /// Each command keeps `abc123` (and what follows it) out of the log.
    fn header_value_is_gone(command: &str) {
        let kept = text("Bash", json!({ "command": command }));
        assert!(!kept.contains("abc123"), "{kept}");
        assert!(!kept.contains("tail"), "{kept}");
        assert!(kept.contains(REDACTED), "{kept}");
    }

    #[test]
    fn header_values_behind_escaped_quotes_are_removed() {
        header_value_is_gone(
            r#"python -c "requests.get(u, headers={\"Authorization\": \"Bearer abc123 tail\"})""#,
        );
        header_value_is_gone(r#"curl -d "{\"Cookie\":\"abc123 tail\"}" u"#);
        // An escaped quote inside a quoted value does not end it.
        header_value_is_gone(r#"curl -H 'Authorization: "Bearer ab\"c123 tail"' u"#);
        header_value_is_gone(r#"node -e 'h = {"cookie": "a\"bc123 tail"}'"#);
    }

    #[test]
    fn header_values_after_a_comma_after_the_name_are_removed() {
        header_value_is_gone(r#"node -e 'req.headers.set("Authorization", "Bearer abc123 tail")'"#);
        header_value_is_gone(r#"node -e "headers.append('Cookie','abc123 tail')""#);
        header_value_is_gone(r#"python -c 'h = [("authorization", "abc123 tail")]'"#);
        // Without a quoted name, a comma is just prose.
        let kept = text("Bash", json!({"command": "echo cookie, fine"}));
        assert!(kept.contains("cookie, fine"), "{kept}");
    }

    #[test]
    fn header_values_after_a_bracket_after_the_name_are_removed() {
        header_value_is_gone(r#"node -e 'headers["Authorization"] = "Bearer abc123 tail"'"#);
        header_value_is_gone(r#"php -r "\$h[\"Cookie\"]=\"abc123 tail\";""#);
    }

    #[test]
    fn header_values_after_a_fat_arrow_are_removed() {
        header_value_is_gone(r#"ruby -e 'get(u, {"Authorization" => "Bearer abc123 tail"})'"#);
        header_value_is_gone(r#"php -r "['Cookie'=>'abc123 tail'];""#);
        header_value_is_gone(r#"perl -e 'my %h = (Authorization => "abc123 tail");'"#);
    }

    #[test]
    fn long_inputs_are_cut_without_leaking_a_token_cut_in_half() {
        let token = "ghp_0123456789abcdefABCDEF";
        // A long token first shrinks to `[redacted]`, which pulls what follows
        // forward: a token cut at the clip must not become visible.
        let filler = format!("ghp_{}", "a".repeat(2 * MAX_INPUT_BYTES - 20));
        let value = format!("{filler} {token} {}", "é".repeat(5000));
        let kept = text("Bash", json!({ "command": value }));
        assert!(kept.len() <= MAX_INPUT_BYTES, "{}", kept.len());
        assert!(!kept.contains("ghp_0"), "{kept}");
        let long = text("Bash", json!({ "command": "é".repeat(5000) }));
        assert!(
            long.len() <= MAX_INPUT_BYTES && long.ends_with('…'),
            "{}",
            long.len()
        );
    }

    #[test]
    fn the_projects_secret_values_are_removed_wherever_they_are() {
        let known = [
            SecretValue::new("plain-value-1"),
            SecretValue::new("value-1x"),
            SecretValue::new("aa"),
        ];
        let kept = input(
            "Bash",
            &json!({"command": "echo plain-value-1x", "plain-value-1": ["aaa"]}),
            Some(&known),
        );
        // Overlapping values go whole, and so does a value overlapping itself.
        assert_eq!(
            kept,
            AuditInput::Json {
                text: json!({"command": format!("echo {REDACTED}"), REDACTED: [REDACTED]})
                    .to_string()
            }
        );
        let file = input(
            "Write",
            &json!({"file_path": "out/plain-value-1.txt", "content": "x"}),
            Some(&known),
        );
        assert!(
            matches!(&file, AuditInput::File { path, .. } if *path == format!("out/{REDACTED}.txt")),
            "{file:?}"
        );
        assert_eq!(
            cut_name("mcp__plain-value-1", Some(&known)),
            format!("mcp__{REDACTED}")
        );
    }

    #[test]
    fn encoded_secret_values_are_removed_too() {
        let known = [SecretValue::new("p@ss \"w/rd\"")];
        for encoded in [
            r#"p@ss \"w/rd\""#,
            r#"p@ss \"w\/rd\""#,
            "p%40ss%20%22w%2Frd%22",
            "p%40ss%20%22w%2frd%22",
        ] {
            let command = format!("curl -d '{{\"k\":\"{encoded}\"}}' https://x?v={encoded}");
            let kept = input("Bash", &json!({ "command": command }), Some(&known));
            let AuditInput::Json { text } = kept else {
                panic!("unexpected {kept:?}")
            };
            assert!(!text.contains("ss"), "{encoded}: {text}");
            assert_eq!(text.matches(REDACTED).count(), 2, "{text}");
        }
    }

    #[test]
    fn a_short_value_in_a_huge_input_is_removed_within_the_cut() {
        let known = [SecretValue::new("b"), SecretValue::new("value-at-the-cut")];
        // Ten megabytes of one-byte matches merge into one place.
        let kept = text_with("Bash", json!({ "command": "b".repeat(10 << 20) }), &known);
        assert_eq!(kept, json!({ "command": REDACTED }).to_string());
        // A value that starts before the cut goes whole; one cut at the
        // end of what is read shows no prefix.
        for at in [2 * MAX_INPUT_BYTES - 4, 2 * MAX_INPUT_BYTES + 4] {
            let value = format!("{}value-at-the-cut tail", "-".repeat(at));
            let kept = text_with("Bash", json!({ "command": value }), &known);
            assert!(
                !kept.contains("value-at"),
                "{at}: {}",
                &kept[kept.len() - 40..]
            );
            assert!(kept.len() <= MAX_INPUT_BYTES, "{}", kept.len());
        }
    }

    fn text_with(tool: &str, value: serde_json::Value, known: &[SecretValue]) -> String {
        match input(tool, &value, Some(known)) {
            AuditInput::Json { text } => text,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn unreadable_secrets_keep_nothing_the_agent_wrote_but_a_hash() {
        let kept = input("Bash", &json!({"command": "ls"}), None);
        assert_eq!(
            kept,
            AuditInput::Json {
                text: REDACTED.into()
            }
        );
        // Only the names of Claude's own tools stay: an MCP tool's could
        // hold a value.
        assert_eq!(cut_name("Bash", None), "Bash");
        assert_eq!(cut_name("mcp__srv__known-value", None), REDACTED);
        let file = input("Write", &json!({"file_path": "a", "content": "y"}), None);
        assert_eq!(
            file,
            AuditInput::File {
                path: REDACTED.into(),
                sha256: Some(sha("y"))
            }
        );
    }

    #[test]
    fn secret_looking_field_names_are_removed_too() {
        let kept = text("Task", json!({"ghp_0123456789abcdefABCDEF": 1}));
        assert_eq!(kept, json!({ REDACTED: 1 }).to_string());
    }
}

mod recording {
    use std::sync::Arc;

    use serde_json::json;
    use tokio::sync::broadcast;

    use super::super::input::REDACTED;
    use super::project;
    use crate::Core;
    use crate::fakes::{MemoryAudit, MemoryProjects, MemorySecrets, ScriptedAgent, Step};
    use crate::protocol::agents::{AgentStatus, AgentsEvent, AgentsResponse};
    use crate::protocol::audit::{AuditEntry, AuditEvent, AuditInput, AuditLogEvent};
    use crate::protocol::permissions::PermissionsResponse;
    use crate::protocol::projects::ProjectId;
    use crate::protocol::secrets::{SecretInfo, SecretStoreStatus, SecretValue};
    use crate::protocol::{ErrorCode, Event, ProtocolError, Request, Response};
    use crate::services::{AuditLog, Secrets};

    async fn ok(core: &Core, json: serde_json::Value) -> Response {
        let request: Request =
            serde_json::from_value(json).unwrap_or_else(|e| panic!("bad request: {e}"));
        core.handle(request)
            .await
            .unwrap_or_else(|e| panic!("request failed: {e}"))
    }

    /// Reads events until the session reaches `want`; returns the entries
    /// shown to live viewers meanwhile.
    async fn until(rx: &mut broadcast::Receiver<Event>, want: &AgentStatus) -> Vec<AuditEntry> {
        let mut shown = Vec::new();
        loop {
            match Core::next_event(rx).await {
                Some(Event::Agents(AgentsEvent::Status { status, .. })) if &status == want => {
                    return shown;
                }
                Some(Event::Audit(AuditLogEvent::Appended(entry))) => shown.push(entry),
                Some(_) => {}
                None => panic!("core closed while waiting for {want:?}"),
            }
        }
    }

    async fn pending(core: &Core) -> String {
        match ok(core, json!({"method": "permissions.pending"})).await {
            Response::Permissions(PermissionsResponse::Pending(p)) if p.len() == 1 => {
                p[0].id.0.clone()
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_core_records_who_decided_what() {
        let log = Arc::new(MemoryAudit::default());
        let agent = ScriptedAgent::new(vec![vec![
            Step::Use {
                tool: "Bash",
                input: json!({"command": "echo ghp_0123456789abcdefABCDEF"}),
            },
            Step::Use {
                tool: "WebFetch",
                input: json!({"url": "https://example.com"}),
            },
            Step::Use {
                tool: "Write",
                input: json!({"file_path": "a.txt", "content": "hello"}),
            },
        ]]);
        let core = Core::builder("test")
            .projects(Arc::new(MemoryProjects::default()))
            .agents(Arc::new(agent))
            .audit(log.clone())
            .build();
        let root = crate::test_path!("/work/a");
        ok(
            &core,
            json!({"method": "projects.add", "params": {"root": root}}),
        )
        .await;
        ok(
            &core,
            json!({"method": "policy.set", "params": {"project": "p1", "policy": {"rules": [
            {"tool": "WebFetch", "action": "deny"}]}}}),
        )
        .await;
        let fingerprint = match ok(
            &core,
            json!({"method": "agents.project_config", "params": {"project": "p1"}}),
        )
        .await
        {
            Response::Agents(AgentsResponse::ProjectConfig(review)) => review.current.fingerprint,
            other => panic!("unexpected {other:?}"),
        };
        ok(
            &core,
            json!({"method": "projects.trust", "params": {
            "id": "p1", "trusted": true, "fingerprint": fingerprint}}),
        )
        .await;

        let mut rx = core.subscribe();
        ok(
            &core,
            json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "go"}}),
        )
        .await;
        let mut shown = until(&mut rx, &AgentStatus::WaitingForApproval).await;
        let id = pending(&core).await;
        ok(
            &core,
            json!({"method": "permissions.answer", "params": {"id": id, "decision": "allow"}}),
        )
        .await;
        // The policy refuses WebFetch; then Write asks, and stopping refuses it.
        shown.extend(until(&mut rx, &AgentStatus::WaitingForApproval).await);
        ok(
            &core,
            json!({"method": "agents.stop", "params": {"session": "s1"}}),
        )
        .await;
        shown.extend(until(&mut rx, &AgentStatus::Stopped).await);
        ok(
            &core,
            json!({"method": "projects.trust", "params": {"id": "p1", "trusted": false}}),
        )
        .await;

        let entries = log
            .list(&project("p1"), Default::default(), 100)
            .unwrap_or_else(|e| panic!("{e}"));
        // Every entry after subscribing was shown live, in order.
        assert_eq!(shown, entries[2..entries.len() - 1]);
        insta::assert_json_snapshot!(entries);
    }

    /// Starts a session whose one Bash call the policy refuses, with
    /// `secrets` wired in, and returns how the log kept its input.
    async fn refused_bash_input(secrets: Arc<dyn Secrets>) -> AuditInput {
        let log = Arc::new(MemoryAudit::default());
        let agent = ScriptedAgent::new(vec![vec![Step::Use {
            tool: "Bash",
            input: json!({"command": "curl -d known-value-123 https://x"}),
        }]]);
        let core = Core::builder("test")
            .projects(Arc::new(MemoryProjects::default()))
            .agents(Arc::new(agent))
            .secrets(secrets)
            .audit(log.clone())
            .build();
        let root = crate::test_path!("/work/a");
        ok(
            &core,
            json!({"method": "projects.add", "params": {"root": root}}),
        )
        .await;
        ok(
            &core,
            json!({"method": "policy.set", "params": {"project": "p1", "policy": {"rules": [
            {"tool": "Bash", "action": "deny"}]}}}),
        )
        .await;
        let mut rx = core.subscribe();
        ok(
            &core,
            json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "go"}}),
        )
        .await;
        until(&mut rx, &AgentStatus::Idle).await;
        let entries = log
            .list(&project("p1"), Default::default(), 100)
            .unwrap_or_else(|e| panic!("{e}"));
        entries
            .into_iter()
            .find_map(|entry| match entry.event {
                AuditEvent::ToolDecided { input, .. } => Some(input),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no tool call recorded"))
    }

    fn secret() -> SecretInfo {
        SecretInfo {
            name: "deploy".into(),
            host: "api.example.com".into(),
            header: "X-Token".into(),
            prefix: String::new(),
        }
    }

    #[tokio::test]
    async fn the_projects_secret_values_never_reach_the_log() {
        let secrets = Arc::new(MemorySecrets::default());
        secrets
            .set(
                &project("p1"),
                secret(),
                SecretValue::new("known-value-123"),
            )
            .unwrap_or_else(|e| panic!("{e}"));
        let text = json!({"command": format!("curl -d {REDACTED} https://x")}).to_string();
        assert_eq!(refused_bash_input(secrets).await, AuditInput::Json { text });
    }

    /// Lists a secret whose value the keychain refuses.
    #[derive(Debug)]
    struct LockedKeychain;

    impl Secrets for LockedKeychain {
        fn status(&self) -> SecretStoreStatus {
            SecretStoreStatus::Available {
                name: "locked".into(),
            }
        }
        fn list(&self, _: &ProjectId) -> Result<Vec<SecretInfo>, ProtocolError> {
            Ok(vec![secret()])
        }
        fn set(&self, _: &ProjectId, _: SecretInfo, _: SecretValue) -> Result<(), ProtocolError> {
            Err(locked())
        }
        fn remove(&self, _: &ProjectId, _: &str) -> Result<(), ProtocolError> {
            Err(locked())
        }
        fn remove_project(&self, _: &ProjectId) -> Result<(), ProtocolError> {
            Err(locked())
        }
        fn value(&self, _: &ProjectId, _: &str) -> Result<SecretValue, ProtocolError> {
            Err(locked())
        }
    }

    fn locked() -> ProtocolError {
        ProtocolError::new(ErrorCode::Unavailable, "the keychain is locked")
    }

    #[tokio::test]
    async fn unreadable_secrets_keep_the_input_out_of_the_log() {
        let text = REDACTED.to_owned();
        assert_eq!(
            refused_bash_input(Arc::new(LockedKeychain)).await,
            AuditInput::Json { text }
        );
    }

    #[tokio::test]
    async fn a_log_that_fails_does_not_stop_the_session() {
        let agent = ScriptedAgent::new(vec![vec![Step::Say("hi")]]);
        let core = Core::builder("test")
            .projects(Arc::new(MemoryProjects::default()))
            .agents(Arc::new(agent))
            .build();
        let root = crate::test_path!("/work/a");
        ok(
            &core,
            json!({"method": "projects.add", "params": {"root": root}}),
        )
        .await;
        let mut rx = core.subscribe();
        ok(
            &core,
            json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "go"}}),
        )
        .await;
        assert_eq!(until(&mut rx, &AgentStatus::Idle).await, Vec::new());
    }
}

/// Keychain reads: cached, bounded by a timeout, and kept when the caller
/// stops waiting.
mod keychain {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};

    use serde_json::json;
    use tokio::sync::Notify;

    use super::super::input::REDACTED;
    use super::super::{KEYCHAIN_TIMEOUT, tool_decided};
    use super::project;
    use crate::Core;
    use crate::fakes::{MemoryAudit, MemoryProjects, MemorySecrets, ScriptedAgent};
    use crate::protocol::agents::{AgentStatus, AgentsEvent, AgentsResponse, SessionId};
    use crate::protocol::audit::{AuditEvent, AuditInput, AuditLogEvent, DecidedBy};
    use crate::protocol::permissions::PermissionsResponse;
    use crate::protocol::projects::ProjectId;
    use crate::protocol::secrets::{SecretInfo, SecretStoreStatus, SecretValue};
    use crate::protocol::{Event, ProtocolError, Request, Response};
    use crate::services::{AuditLog, Secrets};

    /// A keychain whose values wait until it is opened, counting reads.
    #[derive(Debug, Default)]
    struct SlowKeychain {
        open: Mutex<bool>,
        opened: Condvar,
        entered: Notify,
        reads: AtomicUsize,
        inner: MemorySecrets,
    }

    impl SlowKeychain {
        fn with(value: &str) -> Arc<Self> {
            let keychain = Arc::new(Self::default());
            let secret = SecretInfo {
                name: "deploy".into(),
                host: "api.example.com".into(),
                header: "X-Token".into(),
                prefix: String::new(),
            };
            keychain
                .inner
                .set(&project("p1"), secret, SecretValue::new(value))
                .unwrap_or_else(|e| panic!("{e}"));
            keychain
        }

        fn open(&self) {
            *self.open.lock().unwrap_or_else(|e| e.into_inner()) = true;
            self.opened.notify_all();
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
    }

    /// Opens the keychain when the test ends, so its blocked thread ends too.
    struct Opens(Arc<SlowKeychain>);

    impl Drop for Opens {
        fn drop(&mut self) {
            self.0.open();
        }
    }

    impl Secrets for SlowKeychain {
        fn status(&self) -> SecretStoreStatus {
            self.inner.status()
        }
        fn list(&self, p: &ProjectId) -> Result<Vec<SecretInfo>, ProtocolError> {
            self.inner.list(p)
        }
        fn set(&self, p: &ProjectId, s: SecretInfo, v: SecretValue) -> Result<(), ProtocolError> {
            self.inner.set(p, s, v)
        }
        fn remove(&self, p: &ProjectId, name: &str) -> Result<(), ProtocolError> {
            self.inner.remove(p, name)
        }
        fn remove_project(&self, p: &ProjectId) -> Result<(), ProtocolError> {
            self.inner.remove_project(p)
        }
        fn value(&self, p: &ProjectId, name: &str) -> Result<SecretValue, ProtocolError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            let open = self.open.lock().unwrap_or_else(|e| e.into_inner());
            drop(
                self.opened
                    .wait_while(open, |open| !*open)
                    .unwrap_or_else(|e| e.into_inner()),
            );
            self.inner.value(p, name)
        }
    }

    fn core(keychain: &Arc<SlowKeychain>, log: &Arc<MemoryAudit>) -> Core {
        Core::builder("test")
            .projects(Arc::new(MemoryProjects::default()))
            .agents(Arc::new(ScriptedAgent::default()))
            .secrets(keychain.clone())
            .audit(log.clone())
            .build()
    }

    async fn ok(core: &Core, json: serde_json::Value) -> Response {
        let request: Request =
            serde_json::from_value(json).unwrap_or_else(|e| panic!("bad request: {e}"));
        core.handle(request)
            .await
            .unwrap_or_else(|e| panic!("request failed: {e}"))
    }

    fn inputs(log: &MemoryAudit) -> Vec<AuditInput> {
        log.list(&project("p1"), Default::default(), 100)
            .unwrap_or_else(|e| panic!("{e}"))
            .into_iter()
            .filter_map(|entry| match entry.event {
                AuditEvent::ToolDecided { input, .. } => Some(input),
                _ => None,
            })
            .collect()
    }

    fn bash(command: &str) -> serde_json::Value {
        json!({ "command": command })
    }

    fn redacted() -> AuditInput {
        AuditInput::Json {
            text: REDACTED.to_owned(),
        }
    }

    fn kept(command: &str) -> AuditInput {
        AuditInput::Json {
            text: bash(command).to_string(),
        }
    }

    async fn decide(core: &Core, command: &str) {
        let input = bash(command);
        let by = DecidedBy::Person;
        let recorded = tool_decided(core, &project("p1"), None, "Bash", &input, true, by);
        recorded.await.unwrap_or_else(|e| panic!("{e}"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_keychain_that_never_answers_times_out_to_redacted() {
        let keychain = SlowKeychain::with("known-value-123");
        let _opens = Opens(keychain.clone());
        let log = Arc::new(MemoryAudit::default());
        let core = core(&keychain, &log);
        let input = bash("echo known-value-123");
        let by = DecidedBy::Person;
        let recorded = tool_decided(&core, &project("p1"), None, "Bash", &input, true, by);
        keychain.entered.notified().await;
        tokio::time::advance(KEYCHAIN_TIMEOUT).await;
        recorded.await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(inputs(&log), [redacted()]);
    }

    #[tokio::test]
    async fn secret_values_are_read_once_until_they_change() {
        let keychain = SlowKeychain::with("known-value-123");
        keychain.open();
        let log = Arc::new(MemoryAudit::default());
        let core = core(&keychain, &log);
        ok(
            &core,
            json!({"method": "projects.add", "params": {"root": crate::test_path!("/work/a")}}),
        )
        .await;
        decide(&core, "echo known-value-123").await;
        decide(&core, "echo new-value-456").await;
        assert_eq!(keychain.reads(), 1);
        ok(
            &core,
            json!({"method": "secrets.set", "params": {"project": "p1",
            "secret": {"name": "other", "host": "api.other.com", "header": "X-Key"},
            "value": "new-value-456"}}),
        )
        .await;
        decide(&core, "echo new-value-456").await;
        assert_eq!(keychain.reads(), 3);
        ok(
            &core,
            json!({"method": "secrets.remove", "params": {"project": "p1", "name": "other"}}),
        )
        .await;
        decide(&core, "echo new-value-456").await;
        assert_eq!(keychain.reads(), 4);
        let gone = format!("echo {REDACTED}");
        assert_eq!(
            inputs(&log),
            [
                kept(&gone),
                kept("echo new-value-456"),
                kept(&gone),
                kept("echo new-value-456")
            ]
        );
    }

    #[tokio::test]
    async fn an_answer_is_recorded_even_if_the_agent_stops_waiting_mid_read() {
        let keychain = SlowKeychain::with("known-value-123");
        let _opens = Opens(keychain.clone());
        let log = Arc::new(MemoryAudit::default());
        let core = core(&keychain, &log);
        ok(
            &core,
            json!({"method": "projects.add", "params": {"root": crate::test_path!("/work/a")}}),
        )
        .await;
        let mut rx = core.subscribe();
        ok(
            &core,
            json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "go"}}),
        )
        .await;
        let status = |want: AgentStatus| move |event: &Event| matches!(event, Event::Agents(AgentsEvent::Status { status, .. }) if *status == want);
        next(&mut rx, status(AgentStatus::Idle)).await;
        let host = crate::agents::AgentHost::new(
            SessionId("s1".into()),
            project("p1"),
            core.inner_sessions(),
            core.sink(),
        );
        let asking = tokio::spawn(async move {
            host.ask("Bash".into(), bash("echo known-value-123"), None, None)
                .await
        });
        next(&mut rx, status(AgentStatus::WaitingForApproval)).await;
        let id = match ok(&core, json!({"method": "permissions.pending"})).await {
            Response::Permissions(PermissionsResponse::Pending(p)) => p[0].id.0.clone(),
            other => panic!("unexpected {other:?}"),
        };
        ok(
            &core,
            json!({"method": "permissions.answer", "params": {"id": id, "decision": "allow"}}),
        )
        .await;
        keychain.entered.notified().await;
        asking.abort();
        let _ = asking.await;
        keychain.open();
        let appended = next(&mut rx, |event| {
            matches!(event, Event::Audit(AuditLogEvent::Appended(_)))
        })
        .await;
        let Event::Audit(AuditLogEvent::Appended(entry)) = appended else {
            unreachable!()
        };
        let gone = format!("echo {REDACTED}");
        assert_eq!(
            entry.event,
            AuditEvent::ToolDecided {
                tool: "Bash".into(),
                input: kept(&gone),
                allowed: true,
                by: DecidedBy::Person,
            }
        );
        match ok(
            &core,
            json!({"method": "agents.list", "params": {"project": "p1"}}),
        )
        .await
        {
            Response::Agents(AgentsResponse::List(list)) => {
                assert_eq!(list[0].status, AgentStatus::Working);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    async fn next(
        rx: &mut tokio::sync::broadcast::Receiver<Event>,
        want: impl Fn(&Event) -> bool,
    ) -> Event {
        loop {
            match Core::next_event(rx).await {
                Some(event) if want(&event) => return event,
                Some(_) => {}
                None => panic!("core closed"),
            }
        }
    }
}
