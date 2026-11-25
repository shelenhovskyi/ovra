//! One session's conversation with a `claude` process over stream-json,
//! independent of how the process was started, so tests drive it through
//! in-memory pipes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use ovra_core::protocol::agents::{AgentMessage, AgentStatus, Billing};
use ovra_core::protocol::permissions::{Decision, PermissionReason};
use ovra_core::services::{AgentHost, BeforeTool, ToolRole};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::policy;
use crate::wire::{self, Incoming};

/// How long a stopped session may take to exit before it is killed.
pub(crate) const STOP_GRACE: Duration = Duration::from_secs(5);

/// What the core asks of a running session.
#[derive(Debug)]
pub(crate) enum Command {
    Send(String),
    Interrupt,
    /// Interrupts the turn and closes stdin, which ends the process.
    Stop,
}

/// How the conversation ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Ended {
    /// The process closed stdout. `stopped` if the core asked it to stop.
    Closed { stopped: bool },
    /// The process did not close stdout within [`STOP_GRACE`] of a stop.
    Unresponsive,
    /// The session failed in a way the process cannot recover from.
    Failed(String),
}

/// Writes JSON lines to stdin until the channel closes, then closes stdin.
async fn write_lines(
    mut stdin: impl AsyncWrite + Unpin,
    mut lines: mpsc::UnboundedReceiver<Value>,
) {
    while let Some(line) = lines.recv().await {
        let mut bytes = line.to_string().into_bytes();
        bytes.push(b'\n');
        if stdin.write_all(&bytes).await.is_err() || stdin.flush().await.is_err() {
            return;
        }
    }
    let _ = stdin.shutdown().await;
}

struct Driver {
    cwd: PathBuf,
    host: AgentHost,
    /// The session's IDE tools role; `None` passes no `ovra` MCP server.
    ide: Option<ToolRole>,
    out: Option<mpsc::UnboundedSender<Value>>,
    /// Messages held back until the CLI has answered `initialize`.
    queued: Option<Vec<String>>,
    /// Permission requests waiting for a person, by control request id.
    asking: HashMap<String, JoinHandle<()>>,
    interrupts: u32,
    stopping: bool,
    /// The session's cost so far: `result.total_cost_usd` counts the whole
    /// session, `TurnEnd` one turn.
    cost_micro_usd: u64,
}

impl Driver {
    fn write(&self, line: Value) {
        if let Some(out) = &self.out {
            let _ = out.send(line);
        }
    }

    fn send(&mut self, text: String) {
        match &mut self.queued {
            Some(queued) => queued.push(text),
            None => self.write(wire::user(&text)),
        }
    }

    fn interrupt(&mut self) {
        self.interrupts += 1;
        self.write(wire::interrupt(&format!("interrupt{}", self.interrupts)));
    }

    fn command(&mut self, command: Option<Command>) {
        match command {
            Some(Command::Send(text)) => self.send(text),
            Some(Command::Interrupt) => self.interrupt(),
            // A dropped handle means the core forgot the session.
            Some(Command::Stop) | None => {
                if !self.stopping {
                    self.stopping = true;
                    self.interrupt();
                    // Answer tasks hold writers too; stdin closes once all are gone.
                    for (_, task) in self.asking.drain() {
                        task.abort();
                    }
                    self.out = None;
                }
            }
        }
    }

    /// Handles one stdout line; `Err` ends the session.
    async fn line(&mut self, line: &str) -> Result<(), String> {
        match wire::parse(line) {
            Incoming::Started { billing } => {
                if let Some(billing) = billing {
                    self.host.billing(with_provider(billing));
                }
                self.host.status(AgentStatus::Working);
            }
            Incoming::Messages(messages) => {
                let results = messages
                    .iter()
                    .any(|m| matches!(m, AgentMessage::ToolResult { .. }));
                messages.into_iter().for_each(|m| self.host.message(m));
                if results {
                    // A direct session snapshots the tree as the tool ends.
                    self.host.tools_settled().await;
                }
            }
            Incoming::TextDelta(text) => self.host.text_delta(text),
            Incoming::TurnEnd { mut usage, error } => {
                if let Some(total) = usage.cost_micro_usd {
                    usage.cost_micro_usd = Some(total.saturating_sub(self.cost_micro_usd));
                    self.cost_micro_usd = self.cost_micro_usd.max(total);
                }
                if let Some(message) = error {
                    self.host.message(AgentMessage::Error { message });
                }
                self.host.message(AgentMessage::TurnEnd { usage });
                self.host.status(AgentStatus::Idle);
            }
            Incoming::CanUseTool {
                request_id,
                tool,
                input,
                title,
                reason,
            } => self.ask(request_id, tool, input, title, reason),
            Incoming::Hook {
                request_id,
                callback_id,
                input,
            } => {
                let line = if callback_id == wire::POLICY_HOOK {
                    // The CLI waits for the hook, so the tool has not run yet:
                    // a direct session snapshots the tree first (ADR 0005).
                    let verdict = match self.host.before_tool(&input).await {
                        BeforeTool::Deny(reason) => policy::Verdict::Deny(reason),
                        BeforeTool::Ask(reason) => policy::Verdict::Ask(reason),
                        BeforeTool::Proceed => policy::pre_tool_use(&self.cwd, &input),
                    };
                    wire::success(&request_id, verdict.to_hook_output())
                } else {
                    wire::error(&request_id, &format!("unknown hook {callback_id}"))
                };
                self.write(line);
            }
            Incoming::Mcp {
                request_id,
                server,
                message,
            } => self.mcp(request_id, &server, message),
            Incoming::UnknownControl {
                request_id,
                subtype,
            } => self.write(wire::error(
                &request_id,
                &format!("Ovra does not support {subtype}"),
            )),
            Incoming::ControlResponse { request_id, error } if request_id == wire::INIT_ID => {
                if let Some(error) = error {
                    return Err(format!("claude did not start the session: {error}"));
                }
                for text in self.queued.take().unwrap_or_default() {
                    self.write(wire::user(&text));
                }
            }
            Incoming::Cancel { request_id } => {
                // Waiting until the task is gone withdraws the request from
                // the core before any later line is handled.
                if let Some(task) = self.asking.remove(&request_id) {
                    task.abort();
                    let _ = task.await;
                }
            }
            Incoming::ControlResponse { .. } | Incoming::Ignored => {}
        }
        Ok(())
    }

    /// Passes a message for the `ovra` MCP server to the core's IDE tools.
    /// A tool call can wait on the core (starting a worker), so it runs on
    /// its own while the conversation goes on.
    fn mcp(&mut self, request_id: String, server: &str, message: Value) {
        let Some(role) = self.ide.filter(|_| server == wire::IDE_SERVER) else {
            self.write(wire::error(
                &request_id,
                &format!("Ovra has no MCP server {server} for this session"),
            ));
            return;
        };
        let Some(out) = self.out.clone() else {
            return;
        };
        let host = self.host.clone();
        tokio::spawn(async move {
            let reply = host.call_ide(role, message).await;
            let _ = out.send(wire::success(&request_id, wire::mcp_response(reply)));
        });
    }

    /// Asks the person through the core and answers the CLI when they do.
    fn ask(
        &mut self,
        request_id: String,
        tool: String,
        input: Value,
        title: Option<String>,
        reason: Option<PermissionReason>,
    ) {
        let Some(out) = self.out.clone() else {
            return;
        };
        let host = self.host.clone();
        self.asking.retain(|_, task| !task.is_finished());
        let id = request_id.clone();
        let task = tokio::spawn(async move {
            let answer = host.ask(tool, input.clone(), title, reason).await;
            let response = match answer.decision {
                Decision::Allow => wire::allow(input),
                Decision::Deny => wire::deny(
                    answer
                        .message
                        .as_deref()
                        .unwrap_or("The person denied this tool call."),
                ),
            };
            let _ = out.send(wire::success(&id, response));
        });
        self.asking.insert(request_id, task);
    }
}

/// Talks to the process until it closes stdout or ignores a stop. `prompt`
/// is the first message.
pub(crate) async fn run(
    stdout: impl AsyncBufRead + Unpin,
    stdin: impl AsyncWrite + Unpin + Send + 'static,
    cwd: PathBuf,
    host: AgentHost,
    ide: Option<ToolRole>,
    prompt: String,
    mut commands: mpsc::UnboundedReceiver<Command>,
) -> Ended {
    let (out, lines) = mpsc::unbounded_channel();
    tokio::spawn(write_lines(stdin, lines));
    let mut driver = Driver {
        cwd,
        host,
        ide,
        out: Some(out),
        queued: Some(vec![prompt]),
        asking: HashMap::new(),
        interrupts: 0,
        stopping: false,
        cost_micro_usd: 0,
    };
    driver.write(wire::initialize());
    let mut stdout = stdout.lines();
    let mut deadline = None;
    let ended = loop {
        let grace = async {
            match deadline {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            line = stdout.next_line() => match line {
                Ok(Some(line)) => {
                    if let Err(message) = driver.line(&line).await {
                        break Ended::Failed(message);
                    }
                }
                Ok(None) | Err(_) => break Ended::Closed { stopped: driver.stopping },
            },
            command = commands.recv(), if !driver.stopping => {
                driver.command(command);
                if driver.stopping {
                    deadline = Some(tokio::time::Instant::now() + STOP_GRACE);
                }
            }
            () = grace => break Ended::Unresponsive,
        }
    };
    for (_, task) in driver.asking.drain() {
        task.abort();
    }
    ended
}

/// Names the cloud provider from the variables Claude Code reads to choose
/// one; `init` does not say which.
fn with_provider(billing: Billing) -> Billing {
    match billing {
        Billing::Cloud { provider } if provider.is_empty() => {
            let set =
                |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty() && v != "0");
            let provider = [
                ("CLAUDE_CODE_USE_BEDROCK", "bedrock"),
                ("CLAUDE_CODE_USE_VERTEX", "vertex"),
                ("CLAUDE_CODE_USE_FOUNDRY", "foundry"),
            ]
            .into_iter()
            .find(|(name, _)| set(name))
            .map(|(_, provider)| provider.to_owned())
            .unwrap_or_default();
            Billing::Cloud { provider }
        }
        other => other,
    }
}
