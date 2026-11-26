//! Runs sessions of agents that speak ACP (Agent Client Protocol) v1 over
//! stdio: one generic adapter, with each agent described by data (ADR 0011,
//! `docs/specs/acp-adapter.md`). Every agent runs inside the sandbox.
//! Contract: `ovra_core::services::AgentRuntime`; fake:
//! `ovra_core::fakes::ScriptedAgent`.

mod agents;
mod driver;
mod tools;
mod v1;

pub use agents::{Known, codex, gemini, known};

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use std::collections::BTreeSet;
use std::sync::{Mutex, PoisonError};

use ovra_core::protocol::agents::{
    AgentKind, AgentSetup, AgentStatus, ProviderInfo, SandboxNetwork, SandboxReport, SessionMode,
    SetupState,
};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{
    AgentHost, AgentRuntime, AgentSpec, AgentState, BoxFuture, CommandLine, ProfileNetwork,
    ProviderLists, Result, RunningAgent, Sandbox, SandboxProfile, SandboxStatus,
};
use tokio::io::{AsyncRead, AsyncReadExt, BufReader};
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
    /// How Ovra checks it and tells its sign-in, for an agent Ovra knows.
    pub known: Option<Known>,
}

/// Starts sessions of the ACP agents it describes.
#[derive(Debug)]
pub struct AcpAgents {
    agents: Vec<AcpAgent>,
    /// Wraps every session's process; without one no session starts.
    sandbox: Arc<dyn Sandbox>,
    /// The agents whose last session found no sign-in, until one starts.
    signed_out: Arc<Mutex<BTreeSet<AgentKind>>>,
}

impl AcpAgents {
    pub fn new(agents: Vec<AcpAgent>, sandbox: Arc<dyn Sandbox>) -> Self {
        Self {
            agents,
            sandbox,
            signed_out: Arc::default(),
        }
    }

    fn agent(&self, id: AgentKind) -> Option<&AcpAgent> {
        self.agents.iter().find(|a| a.info.id == id)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
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

    /// Runs the program with its check arguments, outside the sandbox
    /// (as `claude --version` runs): found, broken or ready, or signed out
    /// when its last session said so.
    fn setup(
        &self,
        agent: AgentKind,
        path: Option<PathBuf>,
        _config_dir: Option<PathBuf>,
    ) -> BoxFuture<'_, AgentSetup> {
        let described = self.agent(agent).cloned();
        let signed_out = lock(&self.signed_out).contains(&agent);
        Box::pin(async move {
            let shown = path.as_ref().map(|p| p.to_string_lossy().into_owned());
            let Some(agent) = described else {
                return setup_of(shown, None, None, SetupState::NotFound);
            };
            let program = path.unwrap_or_else(|| agent.program.clone());
            let check = agent.known.map_or(&["--version"][..], |k| k.check);
            let run = Process::new(&program)
                .args(check)
                .envs(
                    ovra_core::child_path::env(&[])
                        .into_iter()
                        .filter_map(|(k, v)| v.map(|v| (k, v))),
                )
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .output();
            let executable = Some(program.to_string_lossy().into_owned());
            match tokio::time::timeout(CHECK_TIMEOUT, run).await {
                Err(_) => setup_of(
                    shown,
                    executable,
                    None,
                    SetupState::Broken {
                        message: format!("{} did not answer within 10 seconds", agent.info.name),
                    },
                ),
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    setup_of(shown, None, None, SetupState::NotFound)
                }
                Ok(Err(e)) => setup_of(
                    shown,
                    executable,
                    None,
                    SetupState::Broken {
                        message: format!("could not run {}: {e}", program.display()),
                    },
                ),
                Ok(Ok(out)) if !out.status.success() => setup_of(
                    shown,
                    executable,
                    None,
                    SetupState::Broken {
                        message: format!("{} exited with {}", program.display(), out.status),
                    },
                ),
                Ok(Ok(out)) => {
                    let version = version(&String::from_utf8_lossy(&out.stdout));
                    let state = if signed_out {
                        SetupState::SignedOut
                    } else {
                        SetupState::Ready
                    };
                    setup_of(shown, executable, version, state)
                }
            }
        })
    }

    fn start(&self, spec: AgentSpec, host: AgentHost) -> Result<Box<dyn RunningAgent>> {
        let Some(agent) = self.agent(spec.agent).cloned() else {
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
        let signed_out = self.signed_out.clone();
        let id = agent.info.id;
        let known = agent.known;
        let name = agent.info.name.clone();
        tokio::spawn(async move {
            let status = guarded(async move { session(agent, &*sandbox, spec, host, rx).await });
            let status = match (status.await, known) {
                (AgentStatus::Failed { message }, Some(known)) => {
                    match agents::signed_out(&known, &message) {
                        Some(hint) => {
                            lock(&signed_out).insert(id);
                            AgentStatus::Failed {
                                message: format!("{name} is not signed in. {hint}"),
                            }
                        }
                        None => AgentStatus::Failed { message },
                    }
                }
                (status, _) => {
                    lock(&signed_out).remove(&id);
                    status
                }
            };
            reporter.status(status);
        });
        Ok(Box::new(AcpProcess(commands)))
    }
}

/// How long a setup check may take.
const CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn setup_of(
    path: Option<String>,
    executable: Option<String>,
    version: Option<String>,
    state: SetupState,
) -> AgentSetup {
    AgentSetup {
        path,
        executable,
        version,
        state,
        billing: None,
        api_key_in_env: false,
    }
}

/// The first word of `out` that reads like a version (`0.71.1`).
fn version(out: &str) -> Option<String> {
    out.split_whitespace()
        .map(|w| w.trim_start_matches('v'))
        .find(|w| {
            let parts: Vec<_> = w.split('.').collect();
            parts.len() >= 2
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(str::to_owned)
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
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return failed(format!("{name} started without pipes"));
    };
    let stderr = tokio::spawn(stderr_tail(stderr));
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

/// Reads `stderr` to its end, whatever it holds, and returns the tail of
/// it as text. Only the last bytes are kept, so the agent's stderr cannot
/// take Ovra's memory.
async fn stderr_tail(mut stderr: impl AsyncRead + Unpin) -> String {
    // A char is at most four bytes.
    let keep = STDERR_TAIL * 4;
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    while let Ok(n @ 1..) = stderr.read(&mut chunk).await {
        kept.extend_from_slice(&chunk[..n]);
        if kept.len() > 2 * keep {
            kept.drain(..kept.len() - keep);
        }
    }
    let start = kept.len().saturating_sub(keep);
    tail(&String::from_utf8_lossy(&kept[start..]))
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
