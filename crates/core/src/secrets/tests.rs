use std::sync::Arc;

use serde_json::json;

use super::check;
use crate::Core;
use crate::fakes::{MemoryProjects, MemorySecrets};
use crate::protocol::projects::ProjectId;
use crate::protocol::secrets::{
    SecretInfo, SecretList, SecretStoreStatus, SecretValue, SecretsEvent, SecretsResponse,
};
use crate::protocol::{ErrorCode, Event, Request, Response};
use crate::services::Secrets;

fn info(name: &str, host: &str, header: &str, prefix: &str) -> SecretInfo {
    SecretInfo {
        name: name.into(),
        host: host.into(),
        header: header.into(),
        prefix: prefix.into(),
        variable: None,
    }
}

/// `ok`, or the field the check refused.
fn verdict(secret: SecretInfo, value: &str) -> String {
    match check(&secret, &SecretValue::new(value)) {
        Ok(()) => "ok".into(),
        Err(e) => {
            assert_eq!(e.code, ErrorCode::BadRequest);
            assert!(!e.message.contains(value) || value.is_empty(), "{e}");
            e.message
        }
    }
}

#[test]
fn only_what_the_netproxy_can_send_safely_is_kept() {
    let ok = |s: SecretInfo| verdict(s, "ghp_x");
    assert_eq!(
        ok(info("gh", "api.github.com", "Authorization", "Bearer ")),
        "ok"
    );
    assert_eq!(
        ok(info("gh.2-x_", "api.github.com:8443", "X-Api-Key", "")),
        "ok"
    );

    for name in ["", "-x", "a b", "a/b", &"n".repeat(65)] {
        let got = ok(info(name, "api.github.com", "Authorization", ""));
        assert!(got.contains("name"), "{name:?}: {got}");
    }
    for host in [
        "",
        "localhost",
        "API.github.com",
        "*.github.com",
        "a..b",
        "-a.com",
        "127.0.0.1",
        "[::1]",
        "a.com:0",
        "a.com:70000",
        "a.com:",
        "a.com:80:80",
        "a.com/path",
        "user@a.com",
        "api.anthropic.com",
        "anthropic.com",
        "claude.ai",
        "platform.claude.com",
        "bedrock-runtime.us-east-1.amazonaws.com",
        "vpce-1.bedrock-runtime.us-east-1.vpce.amazonaws.com",
        "bedrock-runtime.cn-north-1.amazonaws.com.cn",
        "us-central1-aiplatform.googleapis.com",
        "x.services.ai.azure.com",
        "generativelanguage.googleapis.com",
        "oauth2.googleapis.com",
        "accounts.google.com",
        "api.openai.com",
        "chatgpt.com",
    ] {
        let got = ok(info("gh", host, "Authorization", ""));
        assert!(got.contains("host"), "{host:?}: {got}");
    }
    for header in [
        "",
        "Host",
        "content-length",
        "Transfer-Encoding",
        "Connection",
        "Proxy-Authorization",
        "proxy-anything",
        "X Key",
        "X-Key:",
        "X-Key\r\nEvil",
    ] {
        let got = ok(info("gh", "api.github.com", header, ""));
        assert!(got.contains("header"), "{header:?}: {got}");
    }
    let got = ok(info("gh", "api.github.com", "Authorization", "Bearer\r\n"));
    assert!(got.contains("prefix"), "{got}");

    let secret = || info("gh", "api.github.com", "Authorization", "");
    for value in ["", "tok\r\nX-Evil: 1", "tok\n", "tok\0", "tab\there"] {
        let got = verdict(secret(), value);
        assert!(got.contains("value"), "{value:?}: {got}");
    }
    let long = "v".repeat(16 * 1024 + 1);
    assert!(verdict(secret(), &long).contains("value"));
}

#[test]
fn a_placeholder_never_replaces_an_agents_own_login() {
    let with = |variable: &str| {
        let secret = SecretInfo {
            variable: Some(variable.into()),
            ..info("gh", "api.github.com", "Authorization", "Bearer ")
        };
        verdict(secret, "ghp_x")
    };
    for ok in [
        "GH_TOKEN",
        "STRIPE_API_KEY",
        "HF_TOKEN",
        "DEPLOY_SECRET",
        "GITLAB_PAT",
    ] {
        assert_eq!(with(ok), "ok", "{ok}");
    }
    for refused in [
        "",
        "_TOKEN",
        "gh_token",
        "PATH",
        "LD_PRELOAD",
        "GH TOKEN",
        "GH-TOKEN",
        "ANTHROPIC_API_KEY",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "OPENAI_API_KEY",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "AWS_SECRET",
        "AZURE_KEY",
        "CODEX_TOKEN",
        "OVRA_TOKEN",
        &format!("{}_KEY", "A".repeat(61)),
    ] {
        assert!(with(refused).contains("variable"), "{refused:?}");
    }
}

#[tokio::test]
async fn two_keys_never_set_the_same_variable() {
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .secrets(Arc::new(MemorySecrets::default()))
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
    let set = |name: &str, host: &str| {
        json!({"method": "secrets.set", "params": {"project": "p1",
            "secret": {"name": name, "host": host, "header": "Authorization",
                "prefix": "Bearer ", "variable": "GH_TOKEN"},
            "value": "v"}})
    };
    assert!(call(set("gh", "api.github.com")).await.is_ok());
    // The same key again keeps its variable.
    assert!(call(set("gh", "api.github.com")).await.is_ok());
    let taken = call(set("other", "api.example.com")).await;
    assert_eq!(taken.err().map(|e| e.code), Some(ErrorCode::Conflict));
}

#[test]
fn a_value_never_shows_in_debug_output() {
    let request: Request = serde_json::from_value(json!({"method": "secrets.set", "params": {
        "project": "p1",
        "secret": {"name": "gh", "host": "api.github.com", "header": "Authorization"},
        "value": "ghp_secret"}}))
    .unwrap_or_else(|e| panic!("{e}"));
    let printed = format!("{request:?}");
    assert!(!printed.contains("ghp_secret"), "{printed}");
    assert!(printed.contains("<secret>"), "{printed}");

    // A frame the parser refuses never quotes the value back.
    let bad = serde_json::from_value::<Request>(json!({"method": "secrets.set", "params": {
        "project": "p1", "secret": {"name": "gh"}, "value": "ghp_secret"}}));
    let message = bad.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(
        !message.is_empty() && !message.contains("ghp_secret"),
        "{message}"
    );
    let number = serde_json::from_value::<SecretValue>(json!(123_456_789));
    let message = number.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(
        !message.is_empty() && !message.contains("123456789"),
        "{message}"
    );
}

#[tokio::test]
async fn the_person_sets_lists_and_removes_a_projects_secrets() {
    let secrets = Arc::new(MemorySecrets::default());
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .secrets(secrets.clone())
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
    let p1 = ProjectId("p1".into());

    let mut rx = core.subscribe();
    let set = |name: &str, header: &str, value: &str| {
        json!({"method": "secrets.set", "params": {"project": "p1",
            "secret": {"name": name, "host": "api.github.com", "header": header, "prefix": "Bearer "},
            "value": value}})
    };
    assert!(call(set("gh", "Authorization", "one")).await.is_ok());
    assert!(matches!(
        rx.try_recv(),
        Ok(Event::Secrets(SecretsEvent::Changed { .. }))
    ));
    // Setting the same name again replaces it.
    assert!(call(set("gh", "Authorization", "two")).await.is_ok());
    assert_eq!(
        secrets
            .value(&p1, "gh")
            .ok()
            .as_ref()
            .map(SecretValue::expose),
        Some("two")
    );

    let taken = call(set("gh2", "authorization", "three")).await;
    assert_eq!(taken.err().map(|e| e.code), Some(ErrorCode::Conflict));
    let bad = call(set("gh2", "Host", "three")).await;
    assert_eq!(bad.err().map(|e| e.code), Some(ErrorCode::BadRequest));

    let listed = call(json!({"method": "secrets.list", "params": {"project": "p1"}})).await;
    assert_eq!(
        listed.ok(),
        Some(Response::Secrets(SecretsResponse::List(SecretList {
            store: SecretStoreStatus::Available {
                name: "memory".into()
            },
            secrets: vec![info("gh", "api.github.com", "Authorization", "Bearer ")],
        })))
    );

    let remove = json!({"method": "secrets.remove", "params": {"project": "p1", "name": "gh"}});
    assert!(call(remove.clone()).await.is_ok());
    assert_eq!(
        call(remove).await.err().map(|e| e.code),
        Some(ErrorCode::NotFound)
    );
    let unknown = json!({"method": "secrets.list", "params": {"project": "p9"}});
    assert_eq!(
        call(unknown).await.err().map(|e| e.code),
        Some(ErrorCode::NotFound)
    );

    // Removing the project removes its secrets: they could never be listed again.
    assert!(call(set("gh", "Authorization", "one")).await.is_ok());
    let removed = call(json!({"method": "projects.remove", "params": {"id": "p1"}})).await;
    assert!(removed.is_ok(), "{removed:?}");
    assert_eq!(secrets.list(&p1).ok(), Some(Vec::new()));
}

#[tokio::test]
async fn without_a_keychain_setting_fails_and_listing_says_why() {
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .build();
    let root = crate::test_path!("/w");
    let add: Request =
        serde_json::from_value(json!({"method": "projects.add", "params": {"root": root}}))
            .unwrap_or_else(|e| panic!("{e}"));
    assert!(core.handle(add).await.is_ok());
    let set: Request = serde_json::from_value(json!({"method": "secrets.set", "params": {"project": "p1",
        "secret": {"name": "gh", "host": "api.github.com", "header": "Authorization"}, "value": "v"}}))
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        core.handle(set).await.err().map(|e| e.code),
        Some(ErrorCode::Unavailable)
    );
    let list: Request =
        serde_json::from_value(json!({"method": "secrets.list", "params": {"project": "p1"}}))
            .unwrap_or_else(|e| panic!("{e}"));
    let Ok(Response::Secrets(SecretsResponse::List(listed))) = core.handle(list).await else {
        panic!("no list");
    };
    assert!(matches!(
        listed.store,
        SecretStoreStatus::Unavailable { .. }
    ));
}

/// Fails to remove anything, as a locked keychain would.
#[derive(Debug)]
struct LockedKeychain;

impl Secrets for LockedKeychain {
    fn status(&self) -> SecretStoreStatus {
        SecretStoreStatus::Unavailable {
            reason: "locked".into(),
        }
    }
    fn list(&self, _: &ProjectId) -> Result<Vec<SecretInfo>, crate::protocol::ProtocolError> {
        Ok(Vec::new())
    }
    fn set(
        &self,
        _: &ProjectId,
        _: SecretInfo,
        _: SecretValue,
    ) -> Result<(), crate::protocol::ProtocolError> {
        Err(locked())
    }
    fn remove(&self, _: &ProjectId, _: &str) -> Result<(), crate::protocol::ProtocolError> {
        Err(locked())
    }
    fn remove_project(&self, _: &ProjectId) -> Result<(), crate::protocol::ProtocolError> {
        Err(locked())
    }
    fn value(&self, _: &ProjectId, _: &str) -> Result<SecretValue, crate::protocol::ProtocolError> {
        Err(locked())
    }
}

fn locked() -> crate::protocol::ProtocolError {
    crate::protocol::ProtocolError::new(ErrorCode::Unavailable, "locked")
}

#[tokio::test]
async fn a_project_whose_secrets_cannot_be_removed_stays() {
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .secrets(Arc::new(LockedKeychain))
        .build();
    let req = |json: serde_json::Value| -> Request {
        serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"))
    };
    let root = crate::test_path!("/w");
    assert!(
        core.handle(req(
            json!({"method": "projects.add", "params": {"root": root}})
        ))
        .await
        .is_ok()
    );
    let removed = core
        .handle(req(
            json!({"method": "projects.remove", "params": {"id": "p1"}}),
        ))
        .await;
    assert_eq!(removed.err().map(|e| e.code), Some(ErrorCode::Unavailable));
    let listed = core
        .handle(req(
            json!({"method": "secrets.list", "params": {"project": "p1"}}),
        ))
        .await;
    assert!(listed.is_ok(), "the project is still there: {listed:?}");
}

#[test]
fn every_provider_has_its_own_hosts_refused() {
    use crate::protocol::agents::AgentKind;
    // Every variant in turn: the match is exhaustive, so a new provider
    // does not compile here until it is chained in, and then fails below
    // until it is in `PROVIDERS` (its own hosts never take a secret by omission).
    let next = |agent: AgentKind| match agent {
        AgentKind::Claude => Some(AgentKind::Gemini),
        AgentKind::Gemini => Some(AgentKind::Codex),
        AgentKind::Codex => None,
    };
    let mut agent = Some(AgentKind::default());
    while let Some(this) = agent {
        assert!(super::PROVIDERS.contains(&this), "{this:?}");
        agent = next(this);
    }
}
