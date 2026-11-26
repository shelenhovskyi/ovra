//! Runs sessions of agents that speak ACP (Agent Client Protocol) over
//! stdio: one generic adapter, with each agent described by data (ADR 0011).
//! A stub for now: it lists the agents it was given and starts none yet.
//! Contract: `ovra_core::services::AgentRuntime`; fake:
//! `ovra_core::fakes::ScriptedAgent`.

#[cfg_attr(not(test), expect(dead_code, reason = "the process layer runs it"))]
mod driver;
#[cfg_attr(not(test), expect(dead_code, reason = "the session driver uses it"))]
mod tools;
#[cfg_attr(not(test), expect(dead_code, reason = "the session driver uses it"))]
mod v1;

use ovra_core::protocol::agents::ProviderInfo;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{AgentHost, AgentRuntime, AgentSpec, Result, RunningAgent};
use tokio::sync::mpsc;

use crate::driver::Command;

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

/// The handle the core keeps; dropping it stops the session.
#[cfg_attr(not(test), expect(dead_code, reason = "the process layer returns it"))]
#[derive(Debug)]
struct AcpProcess(mpsc::UnboundedSender<Command>);

impl AcpProcess {
    #[cfg_attr(not(test), expect(dead_code, reason = "the process layer returns it"))]
    fn command(&self, command: Command) -> Result<()> {
        self.0
            .send(command)
            .map_err(|_| ProtocolError::new(ErrorCode::Conflict, "the session has ended"))
    }
}

impl RunningAgent for AcpProcess {
    fn send(&self, text: String) -> Result<()> {
        self.command(Command::Send(text))
    }
    fn interrupt(&self) -> Result<()> {
        self.command(Command::Interrupt)
    }
    fn stop(&self) {
        let _ = self.command(Command::Stop);
    }
}

#[cfg(test)]
mod tests;
