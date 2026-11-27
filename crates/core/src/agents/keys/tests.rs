use std::sync::Arc;

use serde_json::json;

use crate::Core;
use crate::fakes::{FakeNetProxy, MemoryProjects, MemorySecrets, ScriptedAgent, Step};
use crate::protocol::agents::{AgentStatus, SandboxNetwork, SandboxReport, SessionId};
use crate::protocol::permissions::{Decision, PermissionRequest, PermissionsEvent};
use crate::protocol::policy::{Policy, PolicyAction, PolicyRule};
use crate::protocol::projects::ProjectId;
use crate::protocol::secrets::{SecretInfo, SecretValue};
use crate::protocol::{Event, Request, Response};
use crate::services::{NetReporter, ProfileNetwork, ProjectStore, Secrets};

struct World {
    core: Core,
    net: Arc<FakeNetProxy>,
    agent: Arc<ScriptedAgent>,
    secrets: Arc<MemorySecrets>,
    projects: Arc<MemoryProjects>,
}

fn p1() -> ProjectId {
    ProjectId("p1".into())
}

fn gh() -> SecretInfo {
    SecretInfo {
        name: "github".into(),
        host: "api.github.com".into(),
        header: "Authorization".into(),
        prefix: "Bearer ".into(),
        variable: Some("GH_TOKEN".into()),
    }
}

async fn call(
    core: &Core,
    json: serde_json::Value,
) -> Result<Response, crate::protocol::ProtocolError> {
    let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
    core.handle(request).await
}

/// A project with the GitHub key and a session `s1` in a sandbox with a
/// limited network (`None`: no sandbox).
async fn world(network: Option<SandboxNetwork>) -> World {
    let net = Arc::new(FakeNetProxy::default());
    let agent = Arc::new(ScriptedAgent::new(vec![vec![Step::Say("working")]]));
    *agent.sandbox.lock().unwrap_or_else(|e| e.into_inner()) =
        network.map(|network| SandboxReport {
            name: "test".into(),
            network,
            gaps: Vec::new(),
        });
    let secrets = Arc::new(MemorySecrets::default());
    let projects = Arc::new(MemoryProjects::default());
    let core = Core::builder("test")
        .projects(projects.clone())
        .agents(agent.clone())
        .netproxy(net.clone())
        .secrets(secrets.clone())
        .build();
    call(
        &core,
        json!({"method": "projects.add", "params": {"root": crate::test_path!("/work/a")}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    secrets
        .set(&p1(), gh(), SecretValue::new("ghp_value"))
        .unwrap_or_else(|e| panic!("{e}"));
    call(
        &core,
        json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "direct", "prompt": "go"}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    World {
        core,
        net,
        agent,
        secrets,
        projects,
    }
}

impl World {
    fn reporter(&self) -> NetReporter {
        self.net
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&SessionId("s1".into()))
            .map(|(_, r, _)| r.clone())
            .unwrap_or_else(|| panic!("no proxy"))
    }

    fn rule(&self, action: PolicyAction) {
        let rule = PolicyRule {
            tool: "UseKey".into(),
            pattern: Some("github".into()),
            action,
            exact: false,
            from_approval: false,
        };
        self.projects
            .set_policy(
                &p1(),
                &Policy {
                    rules: vec![rule],
                    quiet: false,
                },
            )
            .unwrap_or_else(|e| panic!("{e}"));
    }

    async fn answer(&self, request: &PermissionRequest, decision: &str, always: bool) {
        call(
            &self.core,
            json!({"method": "permissions.answer", "params": {
            "id": request.id, "decision": decision, "always": always}}),
        )
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    }
}

async fn requested(rx: &mut tokio::sync::broadcast::Receiver<Event>) -> PermissionRequest {
    loop {
        if let Ok(Event::Permissions(PermissionsEvent::Requested(r))) = rx.recv().await {
            return *r;
        }
    }
}

fn pending(core: &Core) -> usize {
    core.sessions().permissions.pending().len()
}

#[tokio::test]
async fn the_sandbox_gets_the_placeholder_never_the_value() {
    let w = world(Some(SandboxNetwork::Proxy)).await;
    let spec = w.agent.started.lock().unwrap_or_else(|e| e.into_inner())[0].clone();
    let ProfileNetwork::Proxy { variables, .. } = spec.sandbox.network else {
        panic!("no proxy");
    };
    assert_eq!(
        variables,
        vec![("GH_TOKEN".into(), "ovra-key-github".into())]
    );
}

#[tokio::test]
async fn the_person_is_asked_once_a_session_and_the_answer_holds() {
    let w = world(Some(SandboxNetwork::Proxy)).await;
    let mut rx = w.core.subscribe();
    let reporter = w.reporter();
    let (first, second) = (reporter.key_consent(&gh()), reporter.key_consent(&gh()));
    let both = tokio::spawn(async move { (first.await, second.await) });
    let request = requested(&mut rx).await;
    assert_eq!(request.tool, "UseKey");
    assert_eq!(
        request.input,
        json!({"key": "github", "host": "api.github.com"})
    );
    assert_eq!(
        request.title.as_deref(),
        Some("Use your github key on api.github.com")
    );
    assert_eq!(
        request
            .always
            .as_ref()
            .map(|r| (r.tool.as_str(), r.pattern.as_deref())),
        Some(("UseKey", Some("github")))
    );
    assert_eq!(pending(&w.core), 1, "one question for both requests");
    w.answer(&request, "allow", false).await;
    assert_eq!(both.await.ok(), Some((true, true)));
    assert!(reporter.key_consent(&gh()).await);
    assert_eq!(pending(&w.core), 0);
}

#[tokio::test]
async fn a_deny_holds_for_the_session() {
    let w = world(Some(SandboxNetwork::Proxy)).await;
    let mut rx = w.core.subscribe();
    let reporter = w.reporter();
    let asked = tokio::spawn(reporter.key_consent(&gh()));
    let request = requested(&mut rx).await;
    w.answer(&request, "deny", false).await;
    assert_eq!(asked.await.ok(), Some(false));
    assert!(!reporter.key_consent(&gh()).await);
    assert_eq!(pending(&w.core), 0);
}

#[tokio::test]
async fn always_allow_adds_a_use_key_rule() {
    let w = world(Some(SandboxNetwork::Proxy)).await;
    let mut rx = w.core.subscribe();
    let asked = tokio::spawn(w.reporter().key_consent(&gh()));
    let request = requested(&mut rx).await;
    w.answer(&request, "allow", true).await;
    assert_eq!(asked.await.ok(), Some(true));
    let policy = w.projects.policy(&p1()).unwrap_or_else(|e| panic!("{e}"));
    let rule = &policy.rules[0];
    assert_eq!(
        (
            rule.tool.as_str(),
            rule.pattern.as_deref(),
            rule.action,
            rule.from_approval
        ),
        ("UseKey", Some("github"), PolicyAction::Auto, true)
    );
}

#[tokio::test]
async fn rules_decide_without_asking() {
    let w = world(Some(SandboxNetwork::Proxy)).await;
    w.rule(PolicyAction::Auto);
    assert!(w.reporter().key_consent(&gh()).await);
    // Off in Settings applies at once, even after an allow.
    w.rule(PolicyAction::Deny);
    assert!(!w.reporter().key_consent(&gh()).await);
    assert_eq!(pending(&w.core), 0);
}

#[tokio::test]
async fn always_allow_counts_only_in_a_limited_sandbox() {
    let w = world(Some(SandboxNetwork::Open)).await;
    w.rule(PolicyAction::Auto);
    let mut rx = w.core.subscribe();
    let asked = tokio::spawn(w.reporter().key_consent(&gh()));
    let request = requested(&mut rx).await;
    assert!(
        request.always.is_none(),
        "no Always allow where auto never counts"
    );
    w.answer(&request, "deny", false).await;
    assert_eq!(asked.await.ok(), Some(false));
}

#[tokio::test]
async fn a_key_removed_or_rebound_since_the_start_is_refused() {
    let w = world(Some(SandboxNetwork::Proxy)).await;
    w.rule(PolicyAction::Auto);
    let reporter = w.reporter();
    let elsewhere = SecretInfo {
        host: "evil.example.com".into(),
        ..gh()
    };
    assert!(!reporter.key_consent(&elsewhere).await);
    w.secrets
        .remove(&p1(), "github")
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!reporter.key_consent(&gh()).await);
}

#[tokio::test(start_paused = true)]
async fn an_unanswered_question_goes_without_the_key() {
    let w = world(Some(SandboxNetwork::Proxy)).await;
    let mut rx = w.core.subscribe();
    let asked = tokio::spawn(w.reporter().key_consent(&gh()));
    let request = requested(&mut rx).await;
    assert_eq!(asked.await.ok(), Some(false));
    assert_eq!(pending(&w.core), 0);
    let resolved = loop {
        if let Ok(Event::Permissions(PermissionsEvent::Resolved { id, decision })) = rx.recv().await
        {
            break (id, decision);
        }
    };
    assert_eq!(resolved, (request.id, Decision::Deny));
}

#[tokio::test]
async fn without_a_core_no_key_is_added() {
    let core = Core::builder("test").build();
    let reporter = NetReporter::new(p1(), SessionId("s1".into()), core.sink());
    assert!(!reporter.key_consent(&gh()).await);
}

#[tokio::test]
async fn an_interrupt_refuses_the_key_once_and_the_next_request_asks_again() {
    let w = world(Some(SandboxNetwork::Proxy)).await;
    let mut rx = w.core.subscribe();
    let reporter = w.reporter();
    let asked = tokio::spawn(reporter.key_consent(&gh()));
    requested(&mut rx).await;
    call(
        &w.core,
        json!({"method": "agents.interrupt", "params": {"session": "s1"}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(asked.await.ok(), Some(false));
    let again = tokio::spawn(reporter.key_consent(&gh()));
    let request = requested(&mut rx).await;
    w.answer(&request, "allow", false).await;
    assert_eq!(again.await.ok(), Some(true));
}

#[tokio::test]
async fn an_idle_session_stays_idle_while_a_key_is_asked_about() {
    let w = world(Some(SandboxNetwork::Proxy)).await;
    let status = || {
        w.core
            .sessions()
            .find(&SessionId("s1".into()), |s| s.status().clone())
            .unwrap_or_else(|e| panic!("{e}"))
    };
    // The scripted turn ends: the session waits for a message.
    for _ in 0..100 {
        if status() == AgentStatus::Idle {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(status(), AgentStatus::Idle);
    let mut rx = w.core.subscribe();
    let asked = tokio::spawn(w.reporter().key_consent(&gh()));
    let request = requested(&mut rx).await;
    assert_eq!(status(), AgentStatus::Idle);
    w.answer(&request, "allow", false).await;
    assert_eq!(asked.await.ok(), Some(true));
    assert_eq!(status(), AgentStatus::Idle);
}

#[tokio::test]
async fn pointing_a_key_elsewhere_drops_its_rules_and_asks_again() {
    let w = world(Some(SandboxNetwork::Proxy)).await;
    w.rule(PolicyAction::Auto);
    let set = |host: &str| {
        json!({"method": "secrets.set", "params": {"project": "p1",
            "secret": {"name": "github", "host": host, "header": "Authorization", "prefix": "Bearer "},
            "value": "ghp_value2"}})
    };
    // A new value for the same binding keeps the rule.
    call(&w.core, set("api.github.com"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        w.projects.policy(&p1()).map(|p| p.rules.len()).ok(),
        Some(1)
    );
    call(&w.core, set("uploads.github.com"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        w.projects.policy(&p1()).map(|p| p.rules.len()).ok(),
        Some(0)
    );
    w.rule(PolicyAction::Auto);
    call(
        &w.core,
        json!({"method": "secrets.remove", "params": {"project": "p1", "name": "github"}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        w.projects.policy(&p1()).map(|p| p.rules.len()).ok(),
        Some(0)
    );
}

#[tokio::test(start_paused = true)]
async fn requests_that_waited_for_an_unanswered_question_do_not_ask_in_turn() {
    let w = world(Some(SandboxNetwork::Proxy)).await;
    let mut rx = w.core.subscribe();
    let reporter = w.reporter();
    let (first, second) = (reporter.key_consent(&gh()), reporter.key_consent(&gh()));
    let both = tokio::spawn(async move { tokio::join!(first, second) });
    requested(&mut rx).await;
    assert_eq!(both.await.ok(), Some((false, false)));
    assert_eq!(pending(&w.core), 0);
    while let Ok(event) = rx.try_recv() {
        let asked = matches!(event, Event::Permissions(PermissionsEvent::Requested(_)));
        assert!(!asked, "the second request asked again");
    }
}

#[tokio::test]
async fn a_turn_started_while_a_key_is_asked_about_shows_working() {
    let w = world(Some(SandboxNetwork::Proxy)).await;
    let s1 = SessionId("s1".into());
    let status = || {
        w.core
            .sessions()
            .find(&s1, |s| s.status().clone())
            .unwrap_or_else(|e| panic!("{e}"))
    };
    for _ in 0..100 {
        if status() == AgentStatus::Idle {
            break;
        }
        tokio::task::yield_now().await;
    }
    let mut rx = w.core.subscribe();
    let asked = tokio::spawn(w.reporter().key_consent(&gh()));
    let request = requested(&mut rx).await;
    // The person's message starts a turn although the card still waits.
    let host = super::AgentHost::new(s1.clone(), p1(), w.core.sessions().clone(), w.core.sink());
    assert!(host.set_status(AgentStatus::Working));
    w.answer(&request, "allow", false).await;
    assert_eq!(asked.await.ok(), Some(true));
    assert_eq!(status(), AgentStatus::Working);
    assert!(host.set_status(AgentStatus::Idle), "the turn's end is seen");
}
