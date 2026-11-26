use std::sync::Arc;

use serde_json::json;

use crate::Core;
use crate::fakes::{FakeSandbox, MemoryForge, MemoryGit, MemoryProjects, ScriptedAgent};
use crate::protocol::agents::{AgentKind, AgentSettings, Capabilities, ProviderInfo};
use crate::protocol::setup::{SetupReport, SetupResponse};
use crate::protocol::{ErrorCode, Request, Response};
use crate::runtimes::AgentRuntimes;
use crate::services::ProjectStore;

async fn check(core: &Core, params: serde_json::Value) -> Result<SetupReport, ErrorCode> {
    let request = json!({"method": "setup.check", "params": params});
    let request: Request = serde_json::from_value(request).unwrap_or_else(|e| panic!("{e}"));
    match core.handle(request).await {
        Ok(Response::Setup(SetupResponse::Report(report))) => Ok(report),
        Ok(other) => panic!("unexpected {other:?}"),
        Err(e) => Err(e.code),
    }
}

#[tokio::test]
async fn checks_every_provider_git_gh_for_origin_and_the_sandbox() {
    let projects = Arc::new(MemoryProjects::default());
    let mut settings = AgentSettings::default();
    settings
        .paths
        .insert(AgentKind::Gemini, "/opt/gemini".into());
    assert!(projects.set_agent_settings(settings).is_ok());
    let gemini = ScriptedAgent::default().as_provider(ProviderInfo {
        id: AgentKind::Gemini,
        name: "Gemini".into(),
        capabilities: Capabilities::default(),
    });
    let git = Arc::new(MemoryGit::default());
    *git.version.lock().unwrap_or_else(|e| e.into_inner()) = Some("2.47.1".into());
    let forge = Arc::new(MemoryForge::default());
    let core = Core::builder("test")
        .projects(projects)
        .agents(Arc::new(AgentRuntimes::new(vec![
            Arc::new(ScriptedAgent::default()),
            Arc::new(gemini),
        ])))
        .git(git)
        .forge(forge.clone())
        .sandbox(Arc::new(FakeSandbox::available()))
        .build();
    let root = crate::test_path!("/work/a");
    let add = json!({"method": "projects.add", "params": {"root": root}});
    let add: Request = serde_json::from_value(add).unwrap_or_else(|e| panic!("{e}"));
    assert!(core.handle(add).await.is_ok());

    let report = check(&core, json!({"project": "p1"})).await;
    let report = report.unwrap_or_else(|e| panic!("{e:?}"));
    insta::assert_json_snapshot!(report);
    assert_eq!(
        *forge.asked.lock().unwrap_or_else(|e| e.into_inner()),
        [(root.into(), "origin".to_owned())]
    );
}

#[tokio::test]
async fn without_a_project_gh_is_skipped_and_missing_services_say_why() {
    let forge = Arc::new(MemoryForge::default());
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(ScriptedAgent::default()))
        .forge(forge.clone())
        .build();

    let report = check(&core, json!({})).await;
    insta::assert_json_snapshot!(report.unwrap_or_else(|e| panic!("{e:?}")));
    assert!(
        forge
            .asked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    );
    assert_eq!(
        check(&core, json!({"project": "nope"})).await.err(),
        Some(ErrorCode::NotFound)
    );
}
