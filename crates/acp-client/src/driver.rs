//! One session's conversation with an ACP agent, independent of how the
//! process was started, so tests drive it through in-memory pipes.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ovra_core::protocol::agents::{AgentMessage, AgentStatus, TurnUsage};
use ovra_core::protocol::permissions::{Decision, PermissionReason};
use ovra_core::services::{AgentHost, BeforeTool};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::tools;
use crate::v1::{self, Incoming, Outcome, StopReason, ToolCallFields, Update};

/// How long a stopped session may take to exit before it is killed: the
/// Claude adapter's grace period.
pub(crate) const STOP_GRACE: Duration = Duration::from_secs(5);

/// The ids of Ovra's first two requests; prompts follow.
const INITIALIZE: u64 = 0;
const NEW_SESSION: u64 = 1;

/// What the core asks of a running session.
#[derive(Debug)]
pub(crate) enum Command {
    Send(String),
    Interrupt,
    /// Cancels the turn and closes stdin, which ends the process.
    Stop,
}

/// How the conversation ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Ended {
    /// The agent closed stdout. `stopped` if the core asked it to stop.
    Closed { stopped: bool },
    /// The agent did not close stdout within [`STOP_GRACE`] of a stop.
    Unresponsive,
    /// The session failed in a way the agent cannot recover from.
    Failed(String),
}

/// The longest line Ovra reads from an agent, so that its output cannot
/// take Ovra's memory.
pub(crate) const MAX_LINE: usize = 4 << 20;

/// Reads from `stdout` into `buf` up to the next newline, the end of the
/// output, or [`MAX_LINE`] bytes. Cancel safe: what it read stays in
/// `buf`, and the next call goes on from there.
async fn read_line(
    stdout: &mut (impl AsyncBufRead + Unpin),
    buf: &mut Vec<u8>,
) -> std::io::Result<()> {
    let room = (MAX_LINE + 1).saturating_sub(buf.len());
    (&mut *stdout)
        .take(room as u64)
        .read_until(b'\n', buf)
        .await
        .map(drop)
}

/// The line [`read_line`] read, without its line ending: `None` at the end
/// of the output, `Err` for a line too long or not UTF-8.
fn take_line(buf: &mut Vec<u8>) -> Result<Option<String>, String> {
    if buf.is_empty() {
        return Ok(None);
    }
    let mut line = std::mem::take(buf);
    if line.last() == Some(&b'\n') {
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
    } else if line.len() > MAX_LINE {
        return Err(format!(
            "the agent wrote a line longer than {} MiB",
            MAX_LINE >> 20
        ));
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|_| "the agent wrote a line Ovra cannot read: it is not UTF-8".to_owned())
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

/// A permission request waiting for the core.
struct Asking {
    task: JoinHandle<()>,
    /// Set when the turn is cancelled: the answer is then `cancelled`.
    cancelled: Arc<AtomicBool>,
}

struct Driver {
    cwd: PathBuf,
    host: AgentHost,
    out: Option<mpsc::UnboundedSender<Value>>,
    /// The agent's id for the session, once `session/new` answered.
    session: Option<String>,
    /// Messages waiting for the session or for the turn to end.
    queued: VecDeque<String>,
    /// The id of the prompt whose turn runs.
    turn: Option<u64>,
    next_id: u64,
    /// The turn's assistant text since the last tool call.
    text: String,
    /// Tool calls the transcript shows, and whether they have a result.
    tools: HashMap<String, (ToolCallFields, bool)>,
    /// Permission requests waiting for the core.
    asking: Vec<Asking>,
    stopping: bool,
}

impl Driver {
    fn write(&self, line: Value) {
        if let Some(out) = &self.out {
            let _ = out.send(line);
        }
    }

    /// Starts the next turn if the session is free.
    fn prompt(&mut self) {
        let (Some(session), None) = (&self.session, self.turn) else {
            return;
        };
        let Some(text) = self.queued.pop_front() else {
            return;
        };
        let id = self.next_id;
        self.next_id += 1;
        self.write(v1::prompt(id, session, &text));
        self.turn = Some(id);
        self.host.status(AgentStatus::Working);
    }

    /// Cancels the turn. The core refuses what it asked as it interrupts,
    /// and the agent hears `cancelled` for each.
    fn interrupt(&mut self) {
        if let (Some(session), Some(_)) = (&self.session, self.turn) {
            self.write(v1::cancel(session));
            for asking in &self.asking {
                asking.cancelled.store(true, Ordering::Relaxed);
            }
        }
    }

    fn command(&mut self, command: Option<Command>) {
        match command {
            Some(Command::Send(text)) => {
                self.queued.push_back(text);
                self.prompt();
            }
            Some(Command::Interrupt) => self.interrupt(),
            // A dropped handle means the core forgot the session.
            Some(Command::Stop) | None => {
                self.stopping = true;
                self.interrupt();
                // Answer tasks hold writers too; stdin closes once all are gone.
                for asking in self.asking.drain(..) {
                    asking.task.abort();
                }
                self.out = None;
            }
        }
    }

    /// Shows the text written since the last tool call as one message.
    fn flush_text(&mut self) {
        if !self.text.is_empty() {
            let text = std::mem::take(&mut self.text);
            self.host
                .message(AgentMessage::AssistantText { text, parent: None });
        }
    }

    /// Takes what an update says about a call, shows the call when it is
    /// new and its result when it has one.
    async fn tool(&mut self, fields: ToolCallFields) {
        let id = fields.tool_call_id.clone();
        let (call, ended) = match self.tools.remove(&id) {
            Some((mut call, ended)) => {
                call.merge(fields);
                (call, ended)
            }
            None => {
                self.flush_text();
                let request = tools::request(&fields);
                self.host.message(AgentMessage::ToolUse {
                    id: id.clone(),
                    name: request.name,
                    input: request.input,
                    call: request.call,
                    parent: None,
                });
                (fields, false)
            }
        };
        let result = (!ended).then(|| tools::result(&call)).flatten();
        self.tools
            .insert(id.clone(), (call, ended || result.is_some()));
        if let Some((output, is_error)) = result {
            self.host.message(AgentMessage::ToolResult {
                tool_use_id: id,
                output,
                is_error,
                parent: None,
            });
            self.host.tools_settled().await;
        }
    }

    async fn turn_end(&mut self, result: Result<Value, String>) {
        self.turn = None;
        // The agent no longer waits for what it asked: waiting until the
        // tasks are gone withdraws the requests before the session is idle.
        for asking in self.asking.drain(..) {
            asking.task.abort();
            let _ = asking.task.await;
        }
        self.flush_text();
        let reason = result.and_then(|result| {
            serde_json::from_value::<v1::PromptDone>(result)
                .map(|done| done.stop_reason)
                .map_err(|e| format!("The agent ended the turn without a stop reason: {e}"))
        });
        let error = match reason {
            Ok(StopReason::EndTurn) => None,
            Ok(StopReason::Cancelled) => {
                self.host.message(AgentMessage::Interrupted);
                None
            }
            Ok(StopReason::MaxTokens) => {
                Some("The turn stopped: it reached its token limit.".into())
            }
            Ok(StopReason::MaxTurnRequests) => {
                Some("The turn stopped: it reached its turn limit.".into())
            }
            Ok(StopReason::Refusal) => Some("The agent refused to go on.".into()),
            Err(message) => Some(message),
        };
        if let Some(message) = error {
            self.host.message(AgentMessage::Error { message });
        }
        self.host.message(AgentMessage::TurnEnd {
            usage: TurnUsage::default(),
        });
        self.host.status(AgentStatus::Idle);
        self.prompt();
    }

    /// Handles one stdout line; `Err` ends the session.
    async fn line(&mut self, line: &str) -> Result<(), String> {
        if line.trim().is_empty() {
            return Ok(());
        }
        match v1::parse(line)? {
            Incoming::Response {
                id: INITIALIZE,
                result,
            } => {
                let version = result
                    .and_then(|r| {
                        serde_json::from_value::<v1::Initialized>(r).map_err(|e| e.to_string())
                    })
                    .map_err(|e| format!("the agent did not initialize: {e}"))?
                    .protocol_version;
                if version != v1::PROTOCOL_VERSION {
                    return Err(format!(
                        "the agent speaks ACP version {version}; Ovra speaks version {}",
                        v1::PROTOCOL_VERSION
                    ));
                }
                self.write(v1::new_session(NEW_SESSION, &self.cwd));
            }
            Incoming::Response {
                id: NEW_SESSION,
                result,
            } => {
                let session = result
                    .and_then(|r| {
                        serde_json::from_value::<v1::NewSession>(r).map_err(|e| e.to_string())
                    })
                    .map_err(|e| format!("the agent did not start the session: {e}"))?
                    .session_id;
                self.host.agent_session(session.clone());
                self.session = Some(session);
                self.prompt();
            }
            Incoming::Response { id, result } if Some(id) == self.turn => {
                self.turn_end(result).await;
            }
            Incoming::Update(update) => match *update {
                Update::AgentMessageChunk {
                    content: v1::Content::Text { text },
                } => {
                    self.host.text_delta(text.clone());
                    self.text.push_str(&text);
                }
                Update::Tool(fields) => self.tool(fields).await,
                Update::AgentMessageChunk { .. } | Update::AgentThoughtChunk | Update::Other => {}
            },
            Incoming::Permission { id, request } => {
                self.tool(request.tool_call.clone()).await;
                self.ask(id, *request);
            }
            Incoming::Refused { id, refusal } => self.write(v1::refuse(&id, &refusal)),
            Incoming::Response { .. } | Incoming::Ignored => {}
        }
        Ok(())
    }

    /// Asks the core about a call and answers the agent when it decides:
    /// the project's policy first, then the person.
    fn ask(&mut self, id: Value, request: v1::PermissionRequest) {
        let Some(out) = self.out.clone() else {
            return;
        };
        // The merged call: the request may name only its id.
        let call = self
            .tools
            .get(&request.tool_call.tool_call_id)
            .map_or_else(|| request.tool_call.clone(), |(call, _)| call.clone());
        let host = self.host.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = cancelled.clone();
        let task = tokio::spawn(async move {
            let tool = tools::request(&call);
            let allowed = match host.before_tool(Some(&call.tool_call_id), &tool).await {
                BeforeTool::Deny(_) => false,
                verdict => {
                    let reason = match verdict {
                        BeforeTool::Ask(text) => Some(PermissionReason { text, kind: None }),
                        _ => None,
                    };
                    let title = call.title.clone();
                    host.ask(tool, title, reason).await.decision == Decision::Allow
                }
            };
            let outcome = if cancel.load(Ordering::Relaxed) {
                Outcome::Cancelled
            } else {
                request.outcome(allowed)
            };
            let _ = out.send(v1::permission(&id, &outcome));
        });
        self.asking.retain(|asking| !asking.task.is_finished());
        self.asking.push(Asking { task, cancelled });
    }
}

/// Talks to the agent until it closes stdout or ignores a stop. `prompt`
/// is the first message.
pub(crate) async fn run(
    stdout: impl AsyncBufRead + Unpin,
    stdin: impl AsyncWrite + Unpin + Send + 'static,
    cwd: PathBuf,
    host: AgentHost,
    prompt: String,
    mut commands: mpsc::UnboundedReceiver<Command>,
) -> Ended {
    let (out, lines) = mpsc::unbounded_channel();
    tokio::spawn(write_lines(stdin, lines));
    let mut driver = Driver {
        cwd,
        host,
        out: Some(out),
        session: None,
        queued: VecDeque::from([prompt]),
        turn: None,
        next_id: NEW_SESSION + 1,
        text: String::new(),
        tools: HashMap::new(),
        asking: Vec::new(),
        stopping: false,
    };
    driver.write(v1::initialize(INITIALIZE));
    let mut stdout = stdout;
    // The line being read; it survives a `select!` that a command wins.
    let mut buf = Vec::new();
    let mut deadline = None;
    let ended = loop {
        let grace = async {
            match deadline {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            read = read_line(&mut stdout, &mut buf) => match read.map(|()| take_line(&mut buf)) {
                Ok(Ok(Some(line))) => {
                    if let Err(message) = driver.line(&line).await {
                        break Ended::Failed(message);
                    }
                }
                Ok(Ok(None)) => break Ended::Closed { stopped: driver.stopping },
                Ok(Err(message)) => break Ended::Failed(message),
                Err(e) => break Ended::Failed(format!("could not read the agent's output: {e}")),
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
    for asking in driver.asking.drain(..) {
        asking.task.abort();
    }
    ended
}
