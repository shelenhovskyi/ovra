//! The setup check (`setup.check`): asks every service what it needs from
//! this computer, all at once, and reports the facts. Nothing here starts
//! an agent or changes anything.

use std::path::PathBuf;
use std::sync::Arc;

use crate::Core;
use crate::protocol::agents::{AgentSetup, SetupState};
use crate::protocol::setup::{
    AgentCheck, GitTool, Probe, SandboxCheck, SetupReport, SetupRequest, SetupResponse,
};
use crate::protocol::{ProtocolError, Response};
use crate::services::{AgentRuntime, Forge, GitService, ProjectStore, Sandbox, SandboxStatus};

/// The services a check asks, apart from the core so each check can run
/// on its own task.
#[derive(Debug)]
pub(crate) struct Services {
    pub(crate) agents: Arc<dyn AgentRuntime>,
    pub(crate) projects: Arc<dyn ProjectStore>,
    pub(crate) git: Arc<dyn GitService>,
    pub(crate) forge: Arc<dyn Forge>,
    pub(crate) sandbox: Option<Arc<dyn Sandbox>>,
}

pub(crate) async fn handle(core: &Core, request: SetupRequest) -> Result<Response, ProtocolError> {
    let SetupRequest::Check { project } = request;
    let root = project.map(|id| core.root(&id)).transpose()?;
    let report = check(core.setup_services(), root).await;
    Ok(Response::Setup(SetupResponse::Report(report)))
}

async fn check(services: Services, root: Option<PathBuf>) -> SetupReport {
    // Unreadable settings check every agent where it is searched for.
    let settings = services.projects.agent_settings().unwrap_or_default();
    let agents: Vec<_> = services
        .agents
        .providers()
        .into_iter()
        .map(|provider| {
            let runtime = services.agents.clone();
            let path = settings.path(provider.id).map(PathBuf::from);
            let task = tokio::spawn(async move { runtime.setup(provider.id, path, None).await });
            (provider, task)
        })
        .collect();
    let git = services.git.clone();
    let git = tokio::spawn(async move { git.version().await });
    let gh = root.map(|root| {
        let forge = services.forge.clone();
        tokio::spawn(async move {
            forge
                .status(root, crate::pr::DEFAULT_REMOTE.to_owned())
                .await
        })
    });

    let mut checked = Vec::new();
    for (provider, task) in agents {
        let setup = task.await.unwrap_or_else(|e| AgentSetup {
            path: None,
            executable: None,
            version: None,
            state: SetupState::Broken {
                message: e.to_string(),
            },
            billing: None,
            api_key_in_env: false,
        });
        checked.push(AgentCheck {
            id: provider.id,
            name: provider.name,
            setup,
        });
    }
    let git = match git.await {
        Ok(Ok(version)) => Probe::Checked {
            found: GitTool {
                installed: version.is_some(),
                version,
            },
        },
        Ok(Err(e)) => Probe::Failed { message: e.message },
        Err(e) => Probe::Failed {
            message: e.to_string(),
        },
    };
    let gh = match gh {
        None => Probe::Skipped,
        Some(task) => match task.await {
            Ok(Ok(found)) => Probe::Checked { found },
            Ok(Err(e)) => Probe::Failed { message: e.message },
            Err(e) => Probe::Failed {
                message: e.to_string(),
            },
        },
    };
    let sandbox = match services.sandbox.map(|s| s.status()) {
        Some(SandboxStatus::Available { name, gaps }) => SandboxCheck::Available { name, gaps },
        Some(SandboxStatus::Unavailable { reason }) => SandboxCheck::Unavailable { reason },
        None => SandboxCheck::Unavailable {
            reason: "this build of Ovra has no sandbox".into(),
        },
    };
    SetupReport {
        agents: checked,
        git,
        gh,
        sandbox,
    }
}

#[cfg(test)]
mod tests;
