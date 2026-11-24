//! In-memory fakes of every service, for tests in this and other crates
//! (enable the `fakes` feature in `[dev-dependencies]`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

mod worktrees;

/// An absolute path for tests on every OS, as a `&'static str`: the Unix
/// path itself, or with a drive in front on Windows (`C:/work/a`), where
/// `/work/a` is not absolute. Compare built paths as `Path`s, not strings.
#[macro_export]
macro_rules! test_path {
    ($path:literal) => {
        if cfg!(windows) {
            concat!("C:", $path)
        } else {
            $path
        }
    };
}

pub use worktrees::MemoryWorktrees;

use crate::path::RelPath;
use crate::protocol::agents::{
    AgentMessage, AgentSettings, AgentStatus, ProjectAgentConfig, SessionId, TurnUsage,
};
use crate::protocol::fs::{DirEntry, DirListing, EntryKind, FileBody, FileContent, FileList};
use crate::protocol::permissions::Decision;
use crate::protocol::projects::{Project, ProjectId, ProjectState, ProjectStatePatch};
use crate::protocol::search::{FileMatches, LineMatch, SearchEvent, SearchId, SearchQuery};
use crate::protocol::terminals::{TermSize, TerminalId, TerminalsEvent};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{
    AgentHost, AgentRuntime, AgentSpec, BoxFuture, EventSink, FsService, Opener, ProjectStore,
    Result, RunningAgent, SearchCancel, SessionStore, StoredSession, TerminalService, ToolRole,
};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn not_found(what: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::NotFound, format!("{what} not found"))
}

/// Projects kept in memory; ids are `p1`, `p2`, ... and time is a counter.
#[derive(Debug, Default)]
pub struct MemoryProjects {
    state: Mutex<(Vec<Project>, u64)>,
    saved: Mutex<BTreeMap<ProjectId, ProjectState>>,
    settings: Mutex<AgentSettings>,
    trusted: Mutex<BTreeMap<ProjectId, ProjectAgentConfig>>,
    /// Projects ever added, so ids are never reused.
    added: Mutex<u64>,
}

impl ProjectStore for MemoryProjects {
    fn list(&self) -> Result<Vec<Project>> {
        let mut list = lock(&self.state).0.clone();
        list.sort_by_key(|p| std::cmp::Reverse(p.last_opened_ms));
        Ok(list)
    }

    fn add(&self, root: &Path) -> Result<Project> {
        let mut state = lock(&self.state);
        let root = root.to_string_lossy().into_owned();
        if let Some(existing) = state.0.iter().find(|p| p.root == root) {
            return Ok(existing.clone());
        }
        let mut added = lock(&self.added);
        *added += 1;
        let project = Project {
            id: ProjectId(format!("p{added}")),
            name: root.rsplit('/').next().unwrap_or(&root).to_owned(),
            root,
            last_opened_ms: None,
            trusted: false,
        };
        state.0.push(project.clone());
        Ok(project)
    }

    fn remove(&self, id: &ProjectId) -> Result<()> {
        let mut state = lock(&self.state);
        let before = state.0.len();
        state.0.retain(|p| &p.id != id);
        if state.0.len() == before {
            return Err(not_found(&id.0));
        }
        lock(&self.saved).remove(id);
        lock(&self.trusted).remove(id);
        Ok(())
    }

    fn get(&self, id: &ProjectId) -> Result<Project> {
        lock(&self.state)
            .0
            .iter()
            .find(|p| &p.id == id)
            .cloned()
            .ok_or_else(|| not_found(&id.0))
    }

    fn touch(&self, id: &ProjectId) -> Result<Project> {
        let mut state = lock(&self.state);
        state.1 += 1;
        let now = state.1;
        let project = state
            .0
            .iter_mut()
            .find(|p| &p.id == id)
            .ok_or_else(|| not_found(&id.0))?;
        project.last_opened_ms = Some(now);
        Ok(project.clone())
    }

    fn state(&self, id: &ProjectId) -> Result<ProjectState> {
        self.get(id)?;
        Ok(lock(&self.saved).get(id).cloned().unwrap_or_default())
    }

    fn update_state(&self, id: &ProjectId, patch: ProjectStatePatch) -> Result<()> {
        self.get(id)?;
        patch.apply(lock(&self.saved).entry(id.clone()).or_default());
        Ok(())
    }

    fn set_trusted(&self, id: &ProjectId, trusted: Option<ProjectAgentConfig>) -> Result<Project> {
        let mut state = lock(&self.state);
        let project = state
            .0
            .iter_mut()
            .find(|p| &p.id == id)
            .ok_or_else(|| not_found(&id.0))?;
        project.trusted = trusted.is_some();
        let project = project.clone();
        let mut configs = lock(&self.trusted);
        match trusted {
            Some(config) => configs.insert(id.clone(), config),
            None => configs.remove(id),
        };
        Ok(project)
    }

    fn trusted_config(&self, id: &ProjectId) -> Result<Option<ProjectAgentConfig>> {
        self.get(id)?;
        Ok(lock(&self.trusted).get(id).cloned())
    }

    fn agent_settings(&self) -> Result<AgentSettings> {
        Ok(lock(&self.settings).clone())
    }

    fn set_agent_settings(&self, settings: AgentSettings) -> Result<()> {
        *lock(&self.settings) = settings;
        Ok(())
    }
}

/// Text files in memory, keyed by root and relative path. Directories are
/// implied by file paths. Records watch calls.
#[derive(Debug, Default)]
pub struct MemoryFs {
    files: Mutex<BTreeMap<(PathBuf, String), (String, u64)>>,
    clock: Mutex<u64>,
    /// Projects currently watched, with the sink they emit to.
    pub watched: Mutex<Vec<(ProjectId, EventSink)>>,
    /// The cancel flag of every search started, for tests that cancel.
    pub searches: Mutex<Vec<SearchCancel>>,
}

impl MemoryFs {
    /// Adds or replaces a file.
    pub fn put(&self, root: &Path, path: &str, text: &str) {
        let mtime = self.tick();
        lock(&self.files).insert(
            (root.to_path_buf(), path.to_owned()),
            (text.to_owned(), mtime),
        );
    }

    fn tick(&self) -> u64 {
        let mut clock = lock(&self.clock);
        *clock += 1;
        *clock
    }

    pub fn watched_ids(&self) -> Vec<ProjectId> {
        lock(&self.watched)
            .iter()
            .map(|(id, _)| id.clone())
            .collect()
    }
}

impl FsService for MemoryFs {
    fn list_dir(&self, root: PathBuf, path: RelPath) -> BoxFuture<'_, Result<DirListing>> {
        Box::pin(async move {
            let prefix = if path.is_root() {
                String::new()
            } else {
                format!("{path}/")
            };
            let mut entries: BTreeMap<String, EntryKind> = BTreeMap::new();
            for (r, p) in lock(&self.files).keys() {
                if *r != root {
                    continue;
                }
                if let Some(rest) = p.strip_prefix(&prefix) {
                    if let Some((dir, _)) = rest.split_once('/') {
                        entries.insert(dir.to_owned(), EntryKind::Dir);
                    } else {
                        entries.entry(rest.to_owned()).or_insert(EntryKind::File);
                    }
                }
            }
            if entries.is_empty() && !path.is_root() {
                return Err(not_found(&path));
            }
            let mut entries: Vec<DirEntry> = entries
                .into_iter()
                .map(|(name, kind)| DirEntry {
                    name,
                    kind,
                    ignored: false,
                    git: None,
                    renamed_from: None,
                })
                .collect();
            entries.sort_by_key(|e| (e.kind != EntryKind::Dir, e.name.clone()));
            Ok(DirListing {
                path: path.to_string(),
                entries,
            })
        })
    }

    fn read_file(&self, root: PathBuf, path: RelPath) -> BoxFuture<'_, Result<FileContent>> {
        Box::pin(async move {
            let files = lock(&self.files);
            let (text, mtime) = files
                .get(&(root, path.to_string()))
                .ok_or_else(|| not_found(&path))?;
            Ok(FileContent {
                path: path.to_string(),
                size: text.len() as u64,
                mtime_ms: *mtime,
                body: FileBody::Text { text: text.clone() },
            })
        })
    }

    fn list_files(
        &self,
        root: PathBuf,
        offset: u32,
        limit: u32,
    ) -> BoxFuture<'_, Result<FileList>> {
        Box::pin(async move {
            let files = lock(&self.files);
            let all: Vec<&String> = files
                .keys()
                .filter(|(r, _)| *r == root)
                .map(|(_, p)| p)
                .collect();
            Ok(FileList {
                total: u32::try_from(all.len()).unwrap_or(u32::MAX),
                paths: all
                    .into_iter()
                    .skip(offset as usize)
                    .take(limit as usize)
                    .cloned()
                    .collect(),
                version: *lock(&self.clock),
            })
        })
    }

    fn write_file(
        &self,
        root: PathBuf,
        path: RelPath,
        text: String,
        expected_mtime_ms: Option<u64>,
    ) -> BoxFuture<'_, Result<u64>> {
        Box::pin(async move {
            let key = (root, path.to_string());
            let current = lock(&self.files).get(&key).map(|(_, mtime)| *mtime);
            // A file deleted since the client read it is a conflict too.
            if expected_mtime_ms.is_some_and(|expected| current != Some(expected)) {
                return Err(ProtocolError::new(
                    ErrorCode::Conflict,
                    format!("{path} changed on disk"),
                ));
            }
            let mtime = self.tick();
            lock(&self.files).insert(key, (text, mtime));
            Ok(mtime)
        })
    }

    /// Literal text only (case-insensitive for ASCII); `include` and
    /// `exclude` must be empty. Emits every result in one event, then
    /// `search.done`, before it returns.
    fn search(
        &self,
        id: SearchId,
        root: PathBuf,
        query: SearchQuery,
        sink: EventSink,
        cancel: SearchCancel,
    ) -> Result<()> {
        if query.regex || query.whole_word || !query.include.is_empty() || !query.exclude.is_empty()
        {
            return Err(ProtocolError::new(
                ErrorCode::BadRequest,
                "the fake file service searches for literal text only",
            ));
        }
        let fold = |text: &str| {
            if query.case_sensitive {
                text.to_owned()
            } else {
                text.to_ascii_lowercase()
            }
        };
        lock(&self.searches).push(cancel.clone());
        let pattern = fold(&query.pattern);
        let utf16 = |text: &str, byte: usize| u32::try_from(text[..byte].encode_utf16().count());
        let mut files = Vec::new();
        for ((r, path), (text, _)) in lock(&self.files).iter() {
            if *r != root || cancel.is_cancelled() {
                continue;
            }
            let mut lines = Vec::new();
            for (n, line) in text.lines().enumerate() {
                let folded = fold(line);
                let ranges: Vec<[u32; 2]> = folded
                    .match_indices(&pattern)
                    .filter_map(|(at, m)| {
                        Some([utf16(line, at).ok()?, utf16(line, at + m.len()).ok()?])
                    })
                    .collect();
                if !ranges.is_empty() {
                    lines.push(LineMatch {
                        line: u32::try_from(n + 1).unwrap_or(u32::MAX),
                        text: line.to_owned(),
                        column: 0,
                        ranges,
                    });
                }
            }
            if !lines.is_empty() {
                files.push(FileMatches {
                    path: path.clone(),
                    lines,
                });
            }
        }
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let (file_count, line_count) = (
            count(files.len()),
            count(files.iter().map(|f| f.lines.len()).sum()),
        );
        if !files.is_empty() {
            sink.emit(SearchEvent::Results { id, files });
        }
        sink.emit(SearchEvent::Done {
            id,
            files: file_count,
            lines: line_count,
            truncated: false,
            cancelled: cancel.is_cancelled(),
        });
        Ok(())
    }

    fn watch(&self, project: ProjectId, _root: PathBuf, sink: EventSink) -> Result<()> {
        lock(&self.watched).push((project, sink));
        Ok(())
    }

    fn unwatch(&self, project: &ProjectId) {
        lock(&self.watched).retain(|(id, _)| id != project);
    }
}

/// Records every file (as `path` under `root`, symlinks unresolved) and URL
/// it is asked to open.
#[derive(Debug, Default)]
pub struct RecordingOpener {
    pub opened: Mutex<Vec<(PathBuf, Option<u32>)>>,
    pub urls: Mutex<Vec<String>>,
}

impl Opener for RecordingOpener {
    fn open_in_editor(&self, root: &Path, path: &RelPath, line: Option<u32>) -> Result<()> {
        lock(&self.opened).push((path.under(root), line));
        Ok(())
    }
    fn open_url(&self, url: &str) -> Result<()> {
        lock(&self.urls).push(url.to_owned());
        Ok(())
    }
}

/// Terminals that echo their input back as output; ids are `t1`, `t2`, ...
/// The scrollback is everything echoed so far.
#[derive(Debug, Default)]
pub struct EchoTerminals {
    open: Mutex<Vec<EchoTerminal>>,
    opened: Mutex<u64>,
}

#[derive(Debug)]
struct EchoTerminal {
    id: TerminalId,
    cwd: PathBuf,
    size: TermSize,
    sink: EventSink,
    output: String,
    seq: u64,
}

impl EchoTerminals {
    /// Working directory and size of each open terminal.
    pub fn open_terminals(&self) -> Vec<(TerminalId, PathBuf, TermSize)> {
        lock(&self.open)
            .iter()
            .map(|t| (t.id.clone(), t.cwd.clone(), t.size))
            .collect()
    }

    fn with<T>(&self, id: &TerminalId, f: impl FnOnce(&mut EchoTerminal) -> T) -> Result<T> {
        let mut open = lock(&self.open);
        let terminal = open
            .iter_mut()
            .find(|t| &t.id == id)
            .ok_or_else(|| not_found(format!("terminal {}", id.0)))?;
        Ok(f(terminal))
    }
}

impl TerminalService for EchoTerminals {
    fn open(&self, cwd: PathBuf, size: TermSize, sink: EventSink) -> Result<TerminalId> {
        let mut opened = lock(&self.opened);
        *opened += 1;
        let id = TerminalId(format!("t{opened}"));
        lock(&self.open).push(EchoTerminal {
            id: id.clone(),
            cwd,
            size,
            sink,
            output: String::new(),
            seq: 0,
        });
        Ok(id)
    }

    fn input(&self, id: &TerminalId, data: &str) -> Result<()> {
        self.with(id, |t| {
            t.seq += 1;
            t.output.push_str(data);
            t.sink.emit(TerminalsEvent::Output {
                id: id.clone(),
                data: data.to_owned(),
                seq: t.seq,
            });
        })
    }

    fn resize(&self, id: &TerminalId, new: TermSize) -> Result<()> {
        self.with(id, |t| t.size = new)
    }

    fn close(&self, id: &TerminalId) -> Result<()> {
        let sink = self.with(id, |t| t.sink.clone())?;
        lock(&self.open).retain(|t| &t.id != id);
        sink.emit(TerminalsEvent::Exited {
            id: id.clone(),
            code: Some(0),
        });
        Ok(())
    }

    fn scrollback(&self, id: &TerminalId) -> Result<(String, u64)> {
        self.with(id, |t| (t.output.clone(), t.seq))
    }
}

/// Sessions kept in memory; share one between two cores to test a restart.
#[derive(Debug, Default)]
pub struct MemorySessions {
    sessions: Mutex<Vec<(StoredSession, Vec<AgentMessage>)>>,
    orchestration: Mutex<Option<String>>,
}

impl SessionStore for MemorySessions {
    fn load(&self, max_messages: usize) -> Result<Vec<(StoredSession, Vec<AgentMessage>)>> {
        Ok(lock(&self.sessions)
            .iter()
            .map(|(session, messages)| {
                let skip = messages.len().saturating_sub(max_messages);
                (session.clone(), messages[skip..].to_vec())
            })
            .collect())
    }

    fn save(&self, session: &StoredSession) -> Result<()> {
        let mut sessions = lock(&self.sessions);
        match sessions
            .iter_mut()
            .find(|(s, _)| s.info.id == session.info.id)
        {
            Some((kept, _)) => *kept = session.clone(),
            None => sessions.push((session.clone(), Vec::new())),
        }
        Ok(())
    }

    fn append(&self, session: &SessionId, message: &AgentMessage) -> Result<()> {
        let mut sessions = lock(&self.sessions);
        let (_, messages) = sessions
            .iter_mut()
            .find(|(s, _)| &s.info.id == session)
            .ok_or_else(|| not_found(format!("session {}", session.0)))?;
        messages.push(message.clone());
        Ok(())
    }

    fn remove(&self, session: &SessionId) -> Result<()> {
        lock(&self.sessions).retain(|(s, _)| &s.info.id != session);
        Ok(())
    }

    fn load_orchestration(&self) -> Result<Option<String>> {
        Ok(lock(&self.orchestration).clone())
    }

    fn save_orchestration(&self, json: &str) -> Result<()> {
        *lock(&self.orchestration) = Some(json.to_owned());
        Ok(())
    }
}

/// One step of a [`ScriptedAgent`] turn.
#[derive(Debug, Clone)]
pub enum Step {
    /// Writes assistant text.
    Say(&'static str),
    /// Asks to use a tool. If allowed, reports a successful result; if
    /// denied, an error result with the denial message. An allowed call of
    /// one of Ovra's tools (`mcp__ovra__<name>`, input = its arguments) goes
    /// to the IDE tools and reports their answer; an allowed `Write`
    /// (`file_path` relative to the session's folder, `content`) goes to
    /// the [`ScriptedAgent::writes_files`] writer, if any.
    Use {
        tool: &'static str,
        input: serde_json::Value,
    },
    /// Fails the session.
    Fail(&'static str),
}

/// An agent that plays a script: turn 0 on start, turn N on the N-th
/// message. After the last turn it waits idle. It reports `scripted-<id>` as
/// its own session id, or keeps the one it resumes. An interrupt skips the rest of
/// the turn. Needs a Tokio runtime.
#[derive(Default)]
pub struct ScriptedAgent {
    turns: Vec<Vec<Step>>,
    /// Scripts for sessions with IDE tools of a role, instead of `turns`.
    roles: Vec<(ToolRole, Vec<Vec<Step>>)>,
    writer: Option<Arc<Writer>>,
    /// Every spec it was started with.
    pub started: Mutex<Vec<AgentSpec>>,
    /// What `project_config` reports for every project.
    pub config: Mutex<ProjectAgentConfig>,
}

/// Writes a file for an allowed `Write` step: the absolute path and text.
type Writer = dyn Fn(&Path, &str) -> std::io::Result<()> + Send + Sync;

impl std::fmt::Debug for ScriptedAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedAgent")
            .field("turns", &self.turns)
            .field("roles", &self.roles)
            .field("writes_files", &self.writer.is_some())
            .finish_non_exhaustive()
    }
}

impl ScriptedAgent {
    pub fn new(turns: Vec<Vec<Step>>) -> Self {
        Self {
            turns,
            ..Self::default()
        }
    }

    /// Sessions with IDE tools of `role` play `turns` instead.
    #[must_use]
    pub fn for_role(mut self, role: ToolRole, turns: Vec<Vec<Step>>) -> Self {
        self.roles.push((role, turns));
        self
    }

    /// Allowed `Write` steps call `writer`; the core itself never touches
    /// files, so a front end that wants real changes passes one.
    #[must_use]
    pub fn writes_files(
        mut self,
        writer: impl Fn(&Path, &str) -> std::io::Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.writer = Some(Arc::new(writer));
        self
    }
}

impl AgentRuntime for ScriptedAgent {
    fn start(&self, spec: AgentSpec, host: AgentHost) -> Result<Box<dyn RunningAgent>> {
        let own = spec
            .resume
            .clone()
            .unwrap_or_else(|| format!("scripted-{}", spec.session.0));
        host.agent_session(own);
        let turns = self
            .roles
            .iter()
            .find(|(role, _)| Some(*role) == spec.tools)
            .map_or_else(|| self.turns.clone(), |(_, turns)| turns.clone());
        let tools = Tools {
            role: spec.tools,
            cwd: spec.cwd.clone(),
            writer: self.writer.clone(),
        };
        lock(&self.started).push(spec);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Option<String>>();
        let interrupted = Arc::new(AtomicBool::new(false));
        let process = ScriptedProcess {
            tx,
            interrupted: interrupted.clone(),
        };
        tokio::spawn(async move {
            let mut turns = turns.into_iter();
            let mut next = turns.next();
            loop {
                if let Some(steps) = next.take() {
                    host.status(AgentStatus::Working);
                    interrupted.store(false, Ordering::SeqCst);
                    if !play(&host, &tools, steps, &interrupted).await {
                        return;
                    }
                }
                host.message(AgentMessage::TurnEnd {
                    usage: TurnUsage::default(),
                });
                host.status(AgentStatus::Idle);
                match rx.recv().await {
                    Some(Some(_)) => next = turns.next(),
                    Some(None) | None => break,
                }
            }
            host.status(AgentStatus::Stopped);
        });
        Ok(Box::new(process))
    }

    fn project_config(&self, _: PathBuf) -> BoxFuture<'_, Result<ProjectAgentConfig>> {
        Box::pin(async move { Ok(lock(&self.config).clone()) })
    }
}

/// What a scripted session's allowed tool calls reach.
struct Tools {
    role: Option<ToolRole>,
    cwd: PathBuf,
    writer: Option<Arc<Writer>>,
}

impl Tools {
    /// The result of an allowed call: output and whether it is an error.
    async fn run(&self, host: &AgentHost, tool: &str, input: serde_json::Value) -> (String, bool) {
        if let Some(name) = tool.strip_prefix(crate::orchestration::IDE_TOOL_PREFIX) {
            let Some(role) = self.role else {
                return ("this session has no Ovra tools".to_owned(), true);
            };
            let call = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": name, "arguments": input}});
            let result = host.call_ide(role, call).await.unwrap_or_default();
            let result = &result["result"];
            let text = result["content"][0]["text"].as_str().unwrap_or("no answer");
            return (text.to_owned(), result["isError"] == true);
        }
        if let ("Write", Some(writer)) = (tool, &self.writer) {
            let path = self
                .cwd
                .join(input["file_path"].as_str().unwrap_or_default());
            let content = input["content"].as_str().unwrap_or_default();
            return match writer(&path, content) {
                Ok(()) => ("done".to_owned(), false),
                Err(e) => (e.to_string(), true),
            };
        }
        ("done".to_owned(), false)
    }
}

/// Plays one turn; `false` if the session failed.
async fn play(host: &AgentHost, tools: &Tools, steps: Vec<Step>, interrupted: &AtomicBool) -> bool {
    for (n, step) in steps.into_iter().enumerate() {
        if interrupted.load(Ordering::SeqCst) {
            break;
        }
        match step {
            Step::Say(text) => host.message(AgentMessage::AssistantText {
                text: text.to_owned(),
                parent: None,
            }),
            Step::Use { tool, input } => {
                let id = format!("tool{n}");
                host.message(AgentMessage::ToolUse {
                    id: id.clone(),
                    name: tool.to_owned(),
                    input: input.clone(),
                    parent: None,
                });
                let answer = host.ask(tool.to_owned(), input.clone(), None, None).await;
                let (output, is_error) = match answer.decision {
                    Decision::Allow => tools.run(host, tool, input).await,
                    Decision::Deny => (answer.message.unwrap_or_default(), true),
                };
                host.message(AgentMessage::ToolResult {
                    tool_use_id: id,
                    output,
                    is_error,
                    parent: None,
                });
            }
            Step::Fail(message) => {
                host.status(AgentStatus::Failed {
                    message: message.to_owned(),
                });
                return false;
            }
        }
    }
    true
}

#[derive(Debug)]
struct ScriptedProcess {
    tx: tokio::sync::mpsc::UnboundedSender<Option<String>>,
    interrupted: Arc<AtomicBool>,
}

impl RunningAgent for ScriptedProcess {
    fn send(&self, text: String) -> Result<()> {
        self.tx
            .send(Some(text))
            .map_err(|_| ProtocolError::new(ErrorCode::Conflict, "the session has ended"))
    }

    fn interrupt(&self) -> Result<()> {
        self.interrupted.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&self) {
        let _ = self.tx.send(None);
    }
}
