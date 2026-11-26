//! Runs sessions of agents that speak ACP (Agent Client Protocol) over
//! stdio: one generic adapter, with each agent described by data (ADR 0011).
//! A stub for now: it lists the agents it was given and starts none yet.
//! Contract: `ovra_core::services::AgentRuntime`; fake:
//! `ovra_core::fakes::ScriptedAgent`.

use ovra_core::protocol::agents::ProviderInfo;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{AgentHost, AgentRuntime, AgentSpec, Result, RunningAgent};

/// Starts sessions of the ACP agents it describes.
#[derive(Debug, Default)]
pub struct AcpAgents {
    agents: Vec<ProviderInfo>,
}

impl AcpAgents {
    pub fn new(agents: Vec<ProviderInfo>) -> Self {
        Self { agents }
    }
}

impl AgentRuntime for AcpAgents {
    fn providers(&self) -> Vec<ProviderInfo> {
        self.agents.clone()
    }

    fn start(&self, spec: AgentSpec, _host: AgentHost) -> Result<Box<dyn RunningAgent>> {
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            format!("{:?} sessions over ACP are not available yet", spec.agent),
        ))
    }
}

#[cfg(test)]
mod tests {
    use ovra_core::protocol::agents::{AgentKind, Capabilities};

    use super::*;

    #[test]
    fn lists_the_agents_it_describes() {
        assert!(AcpAgents::default().providers().is_empty());
        let agent = ProviderInfo {
            id: AgentKind::Claude,
            name: "Some agent".into(),
            capabilities: Capabilities::default(),
        };
        assert_eq!(AcpAgents::new(vec![agent.clone()]).providers(), vec![agent]);
    }
}
