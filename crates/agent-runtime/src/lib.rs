//! Runs agent sessions. Claude sessions are `claude -p` processes from the
//! user's own Claude Code installation, driven over stream-json on stdio
//! (ADR 0006, `docs/specs/claude-cli-protocol.md`).
//! Contract: `ovra_core::services::AgentRuntime`; fake:
//! `ovra_core::fakes::ScriptedAgent`.

mod claude_lists;
mod detect;
mod driver;
mod env;
mod launch;
mod policy;
mod project;
mod tools;
mod wire;

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use ovra_core::protocol::agents::{
    AgentKind, AgentSetup, AgentStatus, Capabilities, ProjectAgentConfig, ProviderInfo,
    SandboxNetwork, SandboxReport, ToolCall,
};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{
    AgentHost, AgentImage, AgentRuntime, AgentSpec, BoxFuture, ProfileNetwork, Result,
    RunningAgent, Sandbox, SandboxStatus,
};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::sync::mpsc;

use crate::driver::{Command, Ended};
use crate::launch::Conversation;

/// How much of the process's stderr a failure message keeps.
const STDERR_TAIL: usize = 2000;

/// Starts Claude sessions with the user's `claude` executable.
#[derive(Debug)]
pub struct ClaudeCli {
    /// Used when the user chose no path; `None` searches for `claude`.
    executable: Option<PathBuf>,
    /// Wraps every session's process (`docs/specs/security.md`).
    sandbox: Arc<dyn Sandbox>,
}

impl ClaudeCli {
    pub fn new(executable: Option<PathBuf>, sandbox: Arc<dyn Sandbox>) -> Self {
        Self {
            executable,
            sandbox,
        }
    }

    /// Continues the Claude Code session `id` (its own session id, not
    /// Ovra's) with `spec.prompt`; `fork` branches it into a new session.
    pub fn resume(
        &self,
        spec: AgentSpec,
        host: AgentHost,
        id: String,
        fork: bool,
    ) -> Result<Box<dyn RunningAgent>> {
        self.launch(spec, host, Conversation::Resume { id, fork })
    }

    fn launch(
        &self,
        spec: AgentSpec,
        host: AgentHost,
        conversation: Conversation,
    ) -> Result<Box<dyn RunningAgent>> {
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(ProtocolError::new(
                ErrorCode::Internal,
                "agent sessions need a Tokio runtime",
            ));
        }
        let (commands, rx) = mpsc::unbounded_channel();
        let executable = spec.executable.clone().or_else(|| self.executable.clone());
        let sandbox = self.sandbox.clone();
        tokio::spawn(async move {
            let status = session(executable, &*sandbox, spec, conversation, host.clone(), rx).await;
            host.status(status);
        });
        Ok(Box::new(ClaudeProcess(commands)))
    }
}

/// What Claude sessions through the CLI can do (ADR 0006, ADR 0011):
/// everything Ovra knows how to use.
const CLAUDE: Capabilities = Capabilities {
    approvals: true,
    enforced_policy: true,
    ide_tools: true,
    read_only_role: true,
    resume: true,
    fork: true,
    interrupt: true,
    text_deltas: true,
    subagents: true,
    usage: true,
    cost: true,
    billing: true,
    accounts: true,
    images: true,
};

impl AgentRuntime for ClaudeCli {
    fn providers(&self) -> Vec<ProviderInfo> {
        vec![ProviderInfo {
            id: AgentKind::Claude,
            name: "Claude".to_owned(),
            capabilities: CLAUDE,
        }]
    }

    /// Starts a new Claude session with an id Ovra chooses, or continues
    /// `spec.resume`, which keeps its id. Either way the core learns the id.
    fn start(&self, spec: AgentSpec, host: AgentHost) -> Result<Box<dyn RunningAgent>> {
        let conversation = Conversation::for_spec(&spec);
        host.agent_session(conversation.id().to_owned());
        self.launch(spec, host, conversation)
    }

    fn setup(
        &self,
        _agent: AgentKind,
        path: Option<PathBuf>,
        config_dir: Option<PathBuf>,
    ) -> BoxFuture<'_, AgentSetup> {
        let chosen = path.or_else(|| self.executable.clone());
        Box::pin(async move {
            detect::setup(chosen, config_dir.as_deref(), &detect::Env::current()).await
        })
    }

    fn tool_call(&self, agent: AgentKind, name: &str, input: &serde_json::Value) -> ToolCall {
        match agent {
            AgentKind::Claude => tools::tool(name, input.clone()).call,
            // Another provider's sessions never ran here.
            AgentKind::Gemini | AgentKind::Codex => ToolCall::default(),
        }
    }

    fn config_dirs(&self) -> BoxFuture<'_, Vec<PathBuf>> {
        Box::pin(async move {
            let env = detect::Env::current();
            tokio::task::spawn_blocking(move || detect::config_dirs(&env))
                .await
                .unwrap_or_default()
        })
    }

    fn project_config(&self, root: PathBuf) -> BoxFuture<'_, Result<ProjectAgentConfig>> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || project::inspect(&root))
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::Internal, e.to_string()))
        })
    }
}

/// Runs one session from version check to exit; returns its final state.
async fn session(
    chosen: Option<PathBuf>,
    sandbox: &dyn Sandbox,
    spec: AgentSpec,
    conversation: Conversation,
    host: AgentHost,
    commands: mpsc::UnboundedReceiver<Command>,
) -> AgentStatus {
    let failed = |message: String| AgentStatus::Failed { message };
    let executable = match detect::find(chosen.as_deref(), &detect::Env::current()) {
        Ok(executable) => executable,
        Err(problem) => return failed(problem.to_string()),
    };
    if let Err(problem) = detect::check_version(&executable).await {
        return failed(problem.to_string());
    }
    let line = launch::line(
        &executable,
        launch::args(
            &conversation,
            spec.trusted,
            spec.tools,
            spec.mode == ovra_core::protocol::agents::SessionMode::Research,
            !spec.no_web,
        ),
        &spec.cwd,
        spec.config_dir.as_deref(),
    );
    // An available sandbox that cannot wrap the session fails it: it never
    // runs unsandboxed instead.
    let line = match sandbox.status() {
        SandboxStatus::Available { name, gaps } => {
            match sandbox.wrap(&launch::profile(&spec), line) {
                Ok(line) => {
                    host.sandboxed(Some(SandboxReport {
                        name,
                        network: network(&spec.sandbox.network),
                        gaps,
                    }));
                    line
                }
                // Not reported: the session never ran, and a resume already
                // dropped the last run's report.
                Err(e) => {
                    return failed(format!("the sandbox could not wrap claude: {}", e.message));
                }
            }
        }
        SandboxStatus::Unavailable { .. } => {
            host.sandboxed(None);
            let outer = std::env::vars_os().map(|(name, _)| name);
            env::unsandboxed(line, outer, cfg!(windows))
        }
    };
    let child = launch::spawnable(&line)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => return failed(format!("could not start {}: {e}", executable.display())),
    };
    // Its memory and CPU are counted with every command it runs.
    if let Some(pid) = child.id() {
        host.process(pid);
    }
    let (Some(stdin), Some(stdout), Some(mut stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return failed("claude started without pipes".to_owned());
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
        spec.tools,
        (spec.prompt, spec.images),
        commands,
    )
    .await;
    // A process that ignored a stop or broke the protocol is not waited on.
    if !matches!(ended, Ended::Closed { .. }) {
        let _ = child.start_kill();
    }
    let exit = child.wait().await;
    let stderr = stderr.await.unwrap_or_default();
    match (ended, exit) {
        (Ended::Closed { stopped: true } | Ended::Unresponsive, _) => AgentStatus::Stopped,
        (Ended::Failed(message), _) => failed(message),
        (Ended::Closed { stopped: false }, Ok(exit)) if exit.success() => AgentStatus::Stopped,
        (Ended::Closed { stopped: false }, Ok(exit)) => {
            failed(join(format!("claude exited with {exit}"), &stderr))
        }
        (Ended::Closed { stopped: false }, Err(e)) => {
            failed(format!("claude did not exit cleanly: {e}"))
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
struct ClaudeProcess(mpsc::UnboundedSender<Command>);

impl ClaudeProcess {
    fn command(&self, command: Command) -> Result<()> {
        self.0
            .send(command)
            .map_err(|_| ProtocolError::new(ErrorCode::Conflict, "the session has ended"))
    }
}

impl RunningAgent for ClaudeProcess {
    fn send(&self, text: String) -> Result<()> {
        self.command(Command::Send(text, Vec::new()))
    }
    fn send_with_images(&self, text: String, images: Vec<AgentImage>) -> Result<()> {
        self.command(Command::Send(text, images))
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
