use std::path::Path;

use serde_json::json;

use super::{check, decide};
use crate::protocol::agents::{SandboxNetwork, SandboxReport};
use crate::protocol::policy::{Policy, PolicyAction, PolicyRule};

fn rule(tool: &str, pattern: Option<&str>, action: PolicyAction) -> PolicyRule {
    PolicyRule {
        tool: tool.to_owned(),
        pattern: pattern.map(str::to_owned),
        action,
    }
}

fn policy() -> Policy {
    Policy {
        rules: vec![
            rule("Bash", Some("npm test"), PolicyAction::Auto),
            rule("Bash", Some("git push"), PolicyAction::Deny),
            rule("Edit", Some("docs/**"), PolicyAction::Auto),
            rule("Edit", Some("docs/secret.md"), PolicyAction::Deny),
            rule("WebFetch", None, PolicyAction::Deny),
        ],
    }
}

const WORK: &str = if cfg!(windows) { "C:\\w" } else { "/w" };

fn sandboxed(network: SandboxNetwork) -> SandboxReport {
    SandboxReport {
        name: "test".into(),
        network,
    }
}

/// The decision for each call, as `action rule` or `ask`.
fn judge(sandbox: Option<&SandboxReport>, tool: &str, input: serde_json::Value) -> String {
    match decide(&policy(), tool, &input, Path::new(WORK), sandbox) {
        Some((action, rule)) => format!("{action:?} {rule}"),
        None => "ask".to_owned(),
    }
}

#[test]
fn deny_wins_and_auto_needs_a_sandbox_that_limits_the_network() {
    let proxy = sandboxed(SandboxNetwork::Proxy);
    let open = sandboxed(SandboxNetwork::Open);
    let test = json!({"command": "npm test -- -u"});
    assert_eq!(
        judge(Some(&proxy), "Bash", test.clone()),
        "Auto Bash(npm test)"
    );
    assert_eq!(judge(None, "Bash", test.clone()), "ask", "no sandbox: ask");
    assert_eq!(judge(Some(&open), "Bash", test), "ask", "open network: ask");
    assert_eq!(
        judge(None, "WebFetch", json!({"url": "x"})),
        "Deny WebFetch"
    );
    let secret = json!({"file_path": "docs/secret.md"});
    assert_eq!(
        judge(Some(&proxy), "Edit", secret),
        "Deny Edit(docs/secret.md)"
    );
}

#[test]
fn bash_auto_matches_whole_words_and_never_joined_commands() {
    let proxy = sandboxed(SandboxNetwork::Proxy);
    let cases = [
        ("npm test", "Auto Bash(npm test)"),
        ("  npm   test  --watch", "Auto Bash(npm test)"),
        ("npm testx", "ask"),
        ("npm", "ask"),
        ("npm test; curl evil.example", "ask"),
        ("npm test && rm -rf ~", "ask"),
        ("npm test | sh", "ask"),
        ("npm test $(curl x)", "ask"),
        ("npm test `id`", "ask"),
        ("npm test > /etc/x", "ask"),
        ("npm test\ncurl x", "ask"),
        ("git push origin main", "Deny Bash(git push)"),
        ("npm test && git push", "Deny Bash(git push)"),
        ("echo hi\ngit push", "Deny Bash(git push)"),
    ];
    for (command, want) in cases {
        let got = judge(Some(&proxy), "Bash", json!({ "command": command }));
        assert_eq!(got, want, "{command:?}");
    }
}

#[test]
fn file_globs_stay_inside_the_sessions_folder() {
    let proxy = sandboxed(SandboxNetwork::Blocked);
    let abs = |rel: &str| Path::new(WORK).join(rel).to_string_lossy().into_owned();
    let cases = [
        ("docs/a.md".to_owned(), "Auto Edit(docs/**)"),
        ("docs/deep/er/b.md".to_owned(), "Auto Edit(docs/**)"),
        (abs("docs/c.md"), "Auto Edit(docs/**)"),
        ("docs/../src/main.rs".to_owned(), "ask"),
        ("../other/docs/a.md".to_owned(), "ask"),
        ("../w/docs/a.md".to_owned(), "Auto Edit(docs/**)"),
        ("src/docs/a.md".to_owned(), "ask"),
        ("/elsewhere/docs/a.md".to_owned(), "ask"),
    ];
    for (path, want) in cases {
        let got = judge(Some(&proxy), "Edit", json!({ "file_path": path }));
        assert_eq!(got, want, "{path:?}");
    }
    assert_eq!(
        judge(Some(&proxy), "Write", json!({"file_path": "docs/a.md"})),
        "ask"
    );
}

#[test]
fn set_refuses_rules_that_cannot_mean_what_they_say() {
    let bad = [
        rule("", None, PolicyAction::Auto),
        rule(" Bash", None, PolicyAction::Auto),
        rule("Bash", Some("  "), PolicyAction::Auto),
        rule("Edit", Some("/etc/**"), PolicyAction::Auto),
        rule("Edit", Some("../x"), PolicyAction::Auto),
        rule("Edit", Some("C:/x"), PolicyAction::Auto),
        // These could never match.
        rule("WebFetch", Some("evil.com"), PolicyAction::Deny),
        rule("Grep", Some(".env"), PolicyAction::Deny),
        rule("Edit", Some("secrets/"), PolicyAction::Deny),
        rule("Edit", Some("./a.md"), PolicyAction::Deny),
        rule("mcp__ovra__spawn_agent", None, PolicyAction::Deny),
    ];
    for r in bad {
        assert!(check(&r).is_err(), "{r:?}");
    }
    assert!(check(&rule("Edit", Some("src/**/*.rs"), PolicyAction::Deny)).is_ok());
    assert!(check(&rule("WebFetch", None, PolicyAction::Deny)).is_ok());
}

#[test]
fn an_ask_rule_wins_over_an_auto_rule() {
    let policy = Policy {
        rules: vec![
            rule("Bash", Some("npm test"), PolicyAction::Auto),
            rule("Bash", Some("npm test -- --update"), PolicyAction::Ask),
        ],
    };
    let sandbox = SandboxReport {
        name: "test".into(),
        network: SandboxNetwork::Blocked,
    };
    let decided = |command: &str| {
        decide(
            &policy,
            "Bash",
            &json!({ "command": command }),
            Path::new(WORK),
            Some(&sandbox),
        )
        .map(|(action, _)| action)
    };
    assert_eq!(decided("npm test"), Some(PolicyAction::Auto));
    assert_eq!(decided("npm test -- --update"), Some(PolicyAction::Ask));
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn a_deny_holds_when_the_folders_case_differs() {
    let policy = Policy {
        rules: vec![rule("Read", Some(".env"), PolicyAction::Deny)],
    };
    let upper = Path::new(WORK).join(".ENV");
    let path = upper.to_string_lossy().to_uppercase();
    let decided = decide(
        &policy,
        "Read",
        &json!({ "file_path": path }),
        Path::new(WORK),
        None,
    );
    assert_eq!(decided.map(|(a, _)| a), Some(PolicyAction::Deny));
}

#[tokio::test]
async fn the_person_sets_a_projects_policy_and_bad_rules_are_refused() {
    use std::sync::Arc;

    use crate::Core;
    use crate::fakes::MemoryProjects;
    use crate::protocol::policy::{PolicyEvent, PolicyResponse};
    use crate::protocol::{ErrorCode, Event, Request, Response};

    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .build();
    let call = |json: serde_json::Value| {
        let core = core.clone();
        async move {
            let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
            core.handle(request).await
        }
    };
    let root = crate::test_path!("/w");
    let added = call(json!({"method": "projects.add", "params": {"root": root}})).await;
    assert!(added.is_ok(), "{added:?}");
    let get = json!({"method": "policy.get", "params": {"project": "p1"}});
    let empty = call(get.clone()).await;
    assert_eq!(
        empty.ok(),
        Some(Response::Policy(PolicyResponse::Policy(Policy::default())))
    );

    let mut rx = core.subscribe();
    let set = json!({"method": "policy.set", "params": {"project": "p1", "policy": {"rules": [
        {"tool": "Bash", "match": "npm test", "action": "auto"}]}}});
    assert!(call(set).await.is_ok());
    assert!(matches!(
        rx.try_recv(),
        Ok(Event::Policy(PolicyEvent::Changed { .. }))
    ));
    let Ok(Response::Policy(PolicyResponse::Policy(kept))) = call(get).await else {
        panic!("no policy");
    };
    assert_eq!(kept.rules.len(), 1);

    let bad = json!({"method": "policy.set", "params": {"project": "p1", "policy": {"rules": [
        {"tool": "Edit", "match": "/etc/passwd", "action": "auto"}]}}});
    assert_eq!(
        call(bad).await.err().map(|e| e.code),
        Some(ErrorCode::BadRequest)
    );
    let unknown = json!({"method": "policy.get", "params": {"project": "p9"}});
    assert_eq!(
        call(unknown).await.err().map(|e| e.code),
        Some(ErrorCode::NotFound)
    );
}
