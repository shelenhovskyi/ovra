//! Runs sessions of agents that speak ACP (Agent Client Protocol) v1 over
//! stdio: one generic adapter, with each agent described by data (ADR 0011,
//! `docs/specs/acp-adapter.md`). Every agent runs inside the sandbox.
//! Contract: `ovra_core::services::AgentRuntime`; fake:
//! `ovra_core::fakes::ScriptedAgent`.

mod driver;
mod tools;
mod v1;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use ovra_core::protocol::agents::{
    AgentStatus, ProviderInfo, SandboxNetwork, SandboxReport, SessionMode,
};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{
    AgentHost, AgentRuntime, AgentSpec, AgentState, CommandLine, ProfileNetwork, ProviderLists,
    Result, RunningAgent, Sandbox, SandboxProfile, SandboxStatus,
};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::Command as Process;
use tokio::sync::mpsc;

use crate::driver::{Command, Ended, STOP_GRACE};

/// How much of the process's stderr a failure message keeps.
const STDERR_TAIL: usize = 2000;

/// One agent that speaks ACP, described by data.
#[derive(Debug, Clone)]
pub struct AcpAgent {
    pub info: ProviderInfo,
    /// Its program, unless the person chose one: a path, or a name found
    /// on `PATH`.
    pub program: PathBuf,
    /// The arguments that make it speak ACP on stdio, such as `--acp`.
    pub args: Vec<OsString>,
    /// Its own state and the variables it needs, for its sandbox.
    pub lists: &'static ProviderLists,
}

/// Starts sessions of the ACP agents it describes.
#[derive(Debug)]
pub struct AcpAgents {
    agents: Vec<AcpAgent>,
    /// Wraps every session's process; without one no session starts.
    sandbox: Arc<dyn Sandbox>,
}

impl AcpAgents {
    pub fn new(agents: Vec<AcpAgent>, sandbox: Arc<dyn Sandbox>) -> Self {
        Self { agents, sandbox }
    }
}

impl AgentRuntime for AcpAgents {
    /// The agents it describes. None has an enforced policy, whatever its
    /// descriptor says: ACP has no hook that holds in an agent's auto
    /// modes, so the agent alone decides when to ask (ADR 0011).
    fn providers(&self) -> Vec<ProviderInfo> {
        self.agents
            .iter()
            .map(|a| {
                let mut info = a.info.clone();
                info.capabilities.enforced_policy = false;
                info
            })
            .collect()
    }

    fn start(&self, spec: AgentSpec, host: AgentHost) -> Result<Box<dyn RunningAgent>> {
        let Some(agent) = self
            .agents
            .iter()
            .find(|a| a.info.id == spec.agent)
            .cloned()
        else {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                format!("no agent {:?} speaks ACP here", spec.agent),
            ));
        };
        if let Some(what) = unsupported(&spec) {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                format!("{} over ACP cannot {what} yet", agent.info.name),
            ));
        }
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(ProtocolError::new(
                ErrorCode::Internal,
                "agent sessions need a Tokio runtime",
            ));
        }
        let (commands, rx) = mpsc::unbounded_channel();
        let sandbox = self.sandbox.clone();
        let reporter = host.clone();
        tokio::spawn(async move {
            let status = guarded(async move { session(agent, &*sandbox, spec, host, rx).await });
            reporter.status(status.await);
        });
        Ok(Box::new(AcpProcess(commands)))
    }
}

/// What `spec` asks for that an ACP session cannot do, if anything: it is
/// refused rather than run without it (docs/specs/acp-adapter.md, not goals
/// for v1).
fn unsupported(spec: &AgentSpec) -> Option<&'static str> {
    if spec.mode == SessionMode::Research {
        Some("run research sessions")
    } else if spec.tools.is_some() {
        Some("use Ovra's IDE tools")
    } else if spec.resume.is_some() {
        Some("resume a session")
    } else if spec.config_dir.is_some() {
        Some("run with another account")
    } else {
        None
    }
}

/// Runs `session` on a task of its own, so that a panic in it still ends
/// the session as failed (which stops its netproxy and drops what it asked).
async fn guarded(session: impl Future<Output = AgentStatus> + Send + 'static) -> AgentStatus {
    tokio::spawn(session)
        .await
        .unwrap_or_else(|e| AgentStatus::Failed {
            message: format!("the session ended unexpectedly: {e}"),
        })
}

/// What the session's sandbox shows: the core's profile with the agent's
/// own state.
fn profile(agent: &AcpAgent, spec: &AgentSpec) -> SandboxProfile {
    SandboxProfile {
        agent_state: Some(AgentState {
            lists: agent.lists,
            config_dir: None,
        }),
        ..spec.sandbox.clone()
    }
}

/// Runs one session from spawn to exit; returns its final state.
async fn session(
    agent: AcpAgent,
    sandbox: &dyn Sandbox,
    spec: AgentSpec,
    host: AgentHost,
    commands: mpsc::UnboundedReceiver<Command>,
) -> AgentStatus {
    let failed = |message: String| AgentStatus::Failed { message };
    let name = agent.info.name.clone();
    let line = CommandLine {
        program: spec
            .executable
            .clone()
            .unwrap_or_else(|| agent.program.clone()),
        args: agent.args.clone(),
        env: Vec::new(),
        cwd: spec.cwd.clone(),
    };
    // An agent whose policy Ovra cannot enforce never runs unsandboxed.
    let line = match sandbox.status() {
        SandboxStatus::Available {
            name: sandbox_name,
            gaps,
        } => match sandbox.wrap(&profile(&agent, &spec), line) {
            Ok(line) => {
                host.sandboxed(Some(SandboxReport {
                    name: sandbox_name,
                    network: network(&spec.sandbox.network),
                    gaps,
                }));
                line
            }
            Err(e) => {
                return failed(format!("the sandbox could not wrap {name}: {}", e.message));
            }
        },
        SandboxStatus::Unavailable { reason } => {
            return failed(format!(
                "{name} runs only inside the sandbox, which is not available here: {reason}"
            ));
        }
    };
    let mut process = Process::new(&line.program);
    process.args(&line.args).current_dir(&line.cwd);
    // The login shell's `PATH`, then the line's: a sandbox's own wins.
    for (variable, value) in ovra_core::child_path::env(&line.env) {
        match value {
            Some(value) => process.env(variable, value),
            None => process.env_remove(variable),
        };
    }
    let child = process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => return failed(format!("could not start {}: {e}", line.program.display())),
    };
    if let Some(pid) = child.id() {
        host.process(pid);
    }
    let (Some(stdin), Some(stdout), Some(mut stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return failed(format!("{name} started without pipes"));
    };
    let stderr = tokio::spawn(async move {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text).await;
        tail(&text)
    });
    let ended = driver::run(
        BufReader::new(stdout),
        stdin,
        spec.cwd,
        host,
        spec.prompt,
        commands,
    )
    .await;
    // An agent that ignored a stop or broke the protocol is not waited on;
    // one that closed stdout gets the grace period to exit.
    if !matches!(ended, Ended::Closed { .. }) {
        let _ = child.start_kill();
    }
    let exit = match tokio::time::timeout(STOP_GRACE, child.wait()).await {
        Ok(exit) => exit,
        Err(_) => {
            let _ = child.start_kill();
            child.wait().await
        }
    };
    // What it started may keep stderr open.
    let stderr = tokio::time::timeout(STOP_GRACE, stderr)
        .await
        .ok()
        .and_then(std::result::Result::ok)
        .unwrap_or_default();
    match (ended, exit) {
        (Ended::Closed { stopped: true } | Ended::Unresponsive, _) => AgentStatus::Stopped,
        (Ended::Failed(message), _) => failed(join(message, &stderr)),
        (Ended::Closed { stopped: false }, Ok(exit)) if exit.success() => AgentStatus::Stopped,
        (Ended::Closed { stopped: false }, Ok(exit)) => {
            failed(join(format!("{name} exited with {exit}"), &stderr))
        }
        (Ended::Closed { stopped: false }, Err(e)) => {
            failed(format!("{name} did not exit cleanly: {e}"))
        }
    }
}

fn network(network: &ProfileNetwork) -> SandboxNetwork {
    match network {
        ProfileNetwork::Blocked => SandboxNetwork::Blocked,
        ProfileNetwork::Proxy { .. } => SandboxNetwork::Proxy,
        ProfileNetwork::Open => SandboxNetwork::Open,
    }
}

fn tail(text: &str) -> String {
    let text = text.trim();
    match text.char_indices().rev().nth(STDERR_TAIL) {
        Some((cut, _)) => format!("…{}", &text[cut..]),
        None => text.to_owned(),
    }
}

fn join(message: String, stderr: &str) -> String {
    if stderr.is_empty() {
        message
    } else {
        format!("{message}: {stderr}")
    }
}

/// The handle the core keeps; dropping it stops the session.
#[derive(Debug)]
struct AcpProcess(mpsc::UnboundedSender<Command>);

impl AcpProcess {
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
