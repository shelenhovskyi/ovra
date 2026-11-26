//! One runtime made of several: each provider's sessions go to the runtime
//! that names it (ADR 0011, `docs/specs/more-agents.md`).

use std::path::PathBuf;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::protocol::agents::{
    AgentKind, AgentSetup, ProjectAgentConfig, ProviderInfo, SetupState, ToolCall,
};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{AgentHost, AgentRuntime, AgentSpec, BoxFuture, Result, RunningAgent};

/// Forwards each call to the runtime of its provider. Where two runtimes
/// name the same provider, the first one listed serves it.
#[derive(Debug)]
pub struct AgentRuntimes {
    runtimes: Vec<Arc<dyn AgentRuntime>>,
}

impl AgentRuntimes {
    pub fn new(runtimes: Vec<Arc<dyn AgentRuntime>>) -> Self {
        Self { runtimes }
    }

    fn of(&self, agent: AgentKind) -> Option<&Arc<dyn AgentRuntime>> {
        self.runtimes
            .iter()
            .find(|r| r.providers().iter().any(|p| p.id == agent))
    }
}

fn unknown(agent: AgentKind) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        format!("Ovra cannot start {agent:?} agents here"),
    )
}

impl AgentRuntime for AgentRuntimes {
    fn providers(&self) -> Vec<ProviderInfo> {
        let mut all: Vec<ProviderInfo> = Vec::new();
        for info in self.runtimes.iter().flat_map(|r| r.providers()) {
            if !all.iter().any(|p| p.id == info.id) {
                all.push(info);
            }
        }
        all
    }

    fn start(&self, spec: AgentSpec, host: AgentHost) -> Result<Box<dyn RunningAgent>> {
        match self.of(spec.agent) {
            Some(runtime) => runtime.start(spec, host),
            None => Err(unknown(spec.agent)),
        }
    }

    fn setup(
        &self,
        agent: AgentKind,
        path: Option<PathBuf>,
        config_dir: Option<PathBuf>,
    ) -> BoxFuture<'_, AgentSetup> {
        match self.of(agent) {
            Some(runtime) => runtime.setup(agent, path, config_dir),
            None => Box::pin(async move {
                AgentSetup {
                    path: path.map(|p| p.to_string_lossy().into_owned()),
                    executable: None,
                    version: None,
                    state: SetupState::Broken {
                        message: unknown(agent).message,
                    },
                    billing: None,
                    api_key_in_env: false,
                }
            }),
        }
    }

    /// Every runtime's items. A project only one runtime has configuration
    /// for keeps that runtime's fingerprint, so trust given before a second
    /// provider existed still applies.
    fn project_config(&self, root: PathBuf) -> BoxFuture<'_, Result<ProjectAgentConfig>> {
        Box::pin(async move {
            let mut found = Vec::new();
            for runtime in &self.runtimes {
                let config = runtime.project_config(root.clone()).await?;
                if config != ProjectAgentConfig::default() {
                    found.push(config);
                }
            }
            Ok(match found.len() {
                0 => ProjectAgentConfig::default(),
                1 => found.remove(0),
                _ => {
                    let mut hash = Sha256::new();
                    for config in &found {
                        hash.update(config.fingerprint.as_bytes());
                        hash.update([0]);
                    }
                    ProjectAgentConfig {
                        fingerprint: format!("{:x}", hash.finalize()),
                        items: found.into_iter().flat_map(|c| c.items).collect(),
                    }
                }
            })
        })
    }

    fn tool_call(&self, agent: AgentKind, name: &str, input: &serde_json::Value) -> ToolCall {
        self.of(agent)
            .map(|r| r.tool_call(agent, name, input))
            .unwrap_or_default()
    }

    fn config_dirs(&self) -> BoxFuture<'_, Vec<PathBuf>> {
        // Accounts are Claude's only (docs/specs/more-agents.md).
        match self.of(AgentKind::Claude) {
            Some(runtime) => runtime.config_dirs(),
            None => Box::pin(async { Vec::new() }),
        }
    }
}

#[cfg(test)]
mod tests;
