//! `agents.*`: agent sessions, their messages and their state.

use serde::{Deserialize, Serialize};

use super::projects::ProjectId;

/// Assigned by the core when a session starts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub String);

/// Which agent runs the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    /// The user's installed Claude Code (ADR 0006).
    Claude,
}

/// Where the agent works (ADR 0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    /// In the project's main checkout; at most one per project.
    Direct,
    /// In its own git worktree (phase 3).
    Isolated,
}

/// The state of a session. Closed set: the UI shows exactly these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AgentStatus {
    /// The agent process is starting.
    Starting,
    /// The agent is working on a turn.
    Working,
    /// The agent waits for a person to answer a permission request.
    WaitingForApproval,
    /// The turn is done; the agent waits for the next message.
    Idle,
    /// The session ended normally or was stopped.
    Stopped,
    /// The session ended with an error.
    Failed { message: String },
}

impl AgentStatus {
    /// Stopped and failed sessions take no more messages.
    pub fn is_ended(&self) -> bool {
        matches!(self, Self::Stopped | Self::Failed { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: SessionId,
    pub project: ProjectId,
    pub agent: AgentKind,
    pub mode: SessionMode,
    pub status: AgentStatus,
    /// Short name for lists: the first line of the first prompt.
    pub title: String,
    /// The sum of every finished turn's usage.
    pub usage: TurnUsage,
}

/// Tokens and cost of one turn, as the agent reports them, or their sum.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Estimated cost in millionths of a US dollar, if the agent reports one.
    pub cost_micro_usd: Option<u64>,
}

/// One entry of a session transcript. Agent-neutral: each runtime maps its
/// own messages to these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentMessage {
    UserText {
        text: String,
    },
    AssistantText {
        text: String,
        /// Set when a subagent wrote it: the tool call that started the subagent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<String>,
    },
    ToolResult {
        tool_use_id: String,
        output: String,
        is_error: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<String>,
    },
    /// The end of a turn.
    TurnEnd {
        usage: TurnUsage,
    },
    Error {
        message: String,
    },
}

/// Whether an agent can run on this machine, and what to do if not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSetup {
    /// The executable the user chose; `None` searches the usual places.
    pub path: Option<String>,
    /// The executable that was checked, if one was found.
    pub executable: Option<String>,
    /// Its version, if it could be read.
    pub version: Option<String>,
    pub state: SetupState,
}

/// What stands between the user and a working agent. Closed set: the UI
/// explains each one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SetupState {
    /// Found, supported and signed in.
    Ready,
    /// Not at the chosen path, on `PATH` or where installers put it.
    NotFound,
    /// Older than the tested range: the user should update it.
    Outdated { minimum: String },
    /// Newer than this build of Ovra supports: the user should update Ovra.
    TooNew { below: String },
    /// Installed but not signed in.
    SignedOut,
    /// Found but could not be checked.
    Broken { message: String },
}

/// Agent settings the user chose, kept across restarts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    /// Absolute path of the `claude` executable; `None` searches for it.
    pub claude_path: Option<String>,
}

/// The project's own agent configuration, which loads only once the user
/// trusts the project. Agents read the project's instructions (`CLAUDE.md`)
/// either way.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectAgentConfig {
    pub items: Vec<ConfigItem>,
}

/// One kind of configuration found in one file or folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigItem {
    /// Relative to the project root, `/`-separated.
    pub path: String,
    pub kind: ConfigKind,
    /// The specific entries: hook commands, variable names, rules, names.
    pub entries: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigKind {
    /// Commands that run on agent events. Runs code.
    Hooks,
    /// Commands the agent runs to get keys or other values. Runs code.
    Helpers,
    /// Plugins and marketplaces, which can bring hooks and servers. Runs code.
    Plugins,
    /// Environment variables set for the agent and its tools.
    Env,
    /// Tool uses allowed without asking.
    AllowRules,
    /// Other settings.
    Settings,
    /// Custom subagents.
    Agents,
    /// Custom slash commands.
    Commands,
    /// Skills.
    Skills,
    /// A settings file that could not be read or parsed; it may still load.
    Unreadable,
}

impl ConfigKind {
    /// Whether loading this runs commands on the user's machine.
    pub fn runs_code(self) -> bool {
        matches!(self, Self::Hooks | Self::Helpers | Self::Plugins)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum AgentsRequest {
    /// Checks that the agent is installed, supported and signed in.
    #[serde(rename = "agents.setup")]
    Setup { agent: AgentKind },
    /// Sets (or with `null` clears) the executable's path, then checks it.
    #[serde(rename = "agents.set_path")]
    SetPath {
        agent: AgentKind,
        path: Option<String>,
    },
    /// What trusting the project would load.
    #[serde(rename = "agents.project_config")]
    ProjectConfig { project: ProjectId },
    /// Sessions of a project, oldest first.
    #[serde(rename = "agents.list")]
    List { project: ProjectId },
    /// Starts a session with a first prompt.
    #[serde(rename = "agents.start")]
    Start {
        project: ProjectId,
        agent: AgentKind,
        mode: SessionMode,
        prompt: String,
    },
    /// Sends the next message. Answers `ok`.
    #[serde(rename = "agents.send")]
    Send { session: SessionId, text: String },
    /// Ends the current turn; the session then waits for the next message.
    /// Pending permission requests are denied. Answers `ok`.
    #[serde(rename = "agents.interrupt")]
    Interrupt { session: SessionId },
    /// Stops the session. Answers `ok`.
    #[serde(rename = "agents.stop")]
    Stop { session: SessionId },
    /// Every message of the session so far.
    #[serde(rename = "agents.transcript")]
    Transcript { session: SessionId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentsResponse {
    #[serde(rename = "agents.setup")]
    Setup(AgentSetup),
    #[serde(rename = "agents.project_config")]
    ProjectConfig(ProjectAgentConfig),
    #[serde(rename = "agents.list")]
    List(Vec<SessionInfo>),
    #[serde(rename = "agents.session")]
    Session(SessionInfo),
    #[serde(rename = "agents.transcript")]
    Transcript(Vec<AgentMessage>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentsEvent {
    #[serde(rename = "agents.message")]
    Message {
        session: SessionId,
        message: AgentMessage,
    },
    /// A session started or changed state.
    #[serde(rename = "agents.status")]
    Status {
        session: SessionId,
        project: ProjectId,
        status: AgentStatus,
    },
}
