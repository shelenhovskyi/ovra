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
    /// Reads the main checkout and searches the web, changes nothing; has
    /// its own cap (ADR 0013).
    Research,
}

/// The state of a session. Closed set: the UI shows exactly these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AgentStatus {
    /// The project already has as many agents working as it may
    /// ([`AgentCapacity::limit`]); the agent starts when one finishes.
    Queued,
    /// A worker's worktree is being prepared (`.worktreeinclude` and the
    /// setup steps of `.ovra/worktree.json`, `docs/specs/worktree-setup.md`);
    /// the agent starts when that is done. Counts as working for the cap.
    /// `Worker::setup` has the step and its log.
    Preparing,
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
    /// Whether `agents.resume` can continue it: it ended and the agent
    /// reported its own id for the conversation.
    #[serde(default)]
    pub resumable: bool,
    /// Who pays for this session's usage, as the agent said when it
    /// started; `None` until then, or if it did not say.
    #[serde(default)]
    pub billing: Option<Billing>,
    /// The sandbox the runtime ran it in; `None` while unknown and for a
    /// session that ran without one (`sandbox_reported` tells them apart).
    #[serde(default)]
    pub sandbox: Option<SandboxReport>,
    /// Whether the runtime reported how it ran the current process: `false`
    /// while it starts. Sessions saved before this field read as `false`;
    /// a `sandbox` that is set counts as reported.
    #[serde(default)]
    pub sandbox_reported: bool,
    /// The Claude account it runs with ([`ClaudeAccount::id`]); `None` for
    /// the default account, and for sessions saved before accounts existed.
    #[serde(default)]
    pub account: Option<String>,
    /// A direct session's files were set back to before this turn (from 1,
    /// as in [`Checkpoint::turn`]): the transcript marks it and the turns
    /// after it as undone. `None` when nothing was gone back.
    #[serde(default)]
    pub undone_from: Option<u32>,
    /// `agents.undo_go_back` can put back what the last go-back restored:
    /// no turn started since.
    #[serde(default)]
    pub can_undo_go_back: bool,
}

/// A turn of a direct session whose files can be set back to how they
/// were before it (`docs/specs/checkpoints.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The turn's number in the session, from 1.
    pub turn: u32,
    /// When the turn's message was sent, in milliseconds since the Unix
    /// epoch.
    pub at_ms: u64,
    /// The first line of the turn's message.
    pub message: String,
    /// How many files differ between before and after the turn.
    pub files_changed: u32,
}

/// How many agents of a project work at once, of how many it may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCapacity {
    /// Sessions that do work now: direct sessions and workers that have not
    /// ended. Coordinators only plan and wait for their workers, and a
    /// worker that asked for review waits idle for the person: neither
    /// counts.
    pub working: u32,
    /// The most that may work at once.
    pub limit: u32,
    /// Workers waiting for a free place (`queued`).
    pub queued: u32,
    /// Research sessions that work now (ADR 0013): counted apart from
    /// `working`, against `research_limit`.
    #[serde(default)]
    pub research_working: u32,
    #[serde(default)]
    pub research_limit: u32,
}

/// Tokens and cost of one turn, as the agent reports them, or their sum.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Estimated cost in millionths of a US dollar, if the agent reports one.
    pub cost_micro_usd: Option<u64>,
}

/// Who wrote a message an agent was sent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageFrom {
    /// The person: a prompt, a reply or a review comment.
    Person,
    /// The task's coordinator: a worker's task or a `send_message`.
    Coordinator,
    /// Ovra itself, in its own words (a note that a worker's review was
    /// decided, for example).
    Ovra,
    /// Saved before Ovra recorded who wrote a message: never shown as the
    /// person's.
    #[default]
    Unknown,
}

/// One entry of a session transcript. Agent-neutral: each runtime maps its
/// own messages to these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentMessage {
    UserText {
        text: String,
        /// Who wrote it: a person decides on approvals by what *they* asked.
        #[serde(default)]
        from: MessageFrom,
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
    /// The person stopped the turn before it finished (an interrupt); no
    /// failure. The turn's end follows.
    Interrupted,
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
    /// Where usage is billed, when the agent says.
    #[serde(default)]
    pub billing: Option<Billing>,
    /// `ANTHROPIC_API_KEY` is set for agents, which makes Claude Code bill
    /// that key instead of a subscription.
    #[serde(default)]
    pub api_key_in_env: bool,
}

/// How the runtime actually ran a session (`AgentHost::sandboxed`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxReport {
    /// The sandbox's name, such as `bubblewrap`.
    pub name: String,
    pub network: SandboxNetwork,
    /// What the sandbox does not cover, each a short phrase such as
    /// `DNS lookups`. Reports saved before the field read as none.
    #[serde(default)]
    pub gaps: Vec<String>,
}

/// What a sandboxed session can reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxNetwork {
    Blocked,
    /// Only through the session's netproxy.
    Proxy,
    /// Not limited: the platform cannot.
    Open,
}

/// Who pays for an agent's usage. Closed set: the UI explains each one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Billing {
    /// The user's Claude plan.
    Subscription,
    /// An Anthropic API key.
    ApiKey,
    /// A cloud provider's account, such as `bedrock` or `vertex`.
    Cloud { provider: String },
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
    /// The Claude accounts the user added. The default account is
    /// [`ClaudeAccount::default_account`] unless an entry with its id
    /// renames it.
    pub accounts: Vec<ClaudeAccount>,
    /// Each project's account ([`ClaudeAccount::id`]) for sessions started
    /// without one; a project not listed uses the default account.
    pub project_accounts: std::collections::BTreeMap<ProjectId, String>,
    /// How many agents may work at once in one project
    /// ([`AgentLimit`]); `None` is the default.
    pub max_working: Option<u32>,
    /// How many research sessions may work at once in one project (ADR
    /// 0013); `None` is the default.
    pub research_limit: Option<u32>,
}

/// How many agents may work at once in each project, and the range the
/// person may choose from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentLimit {
    pub limit: u32,
    pub default: u32,
    pub min: u32,
    pub max: u32,
}

impl AgentSettings {
    /// Every account, the default one first.
    pub fn all_accounts(&self) -> Vec<ClaudeAccount> {
        let default = self
            .accounts
            .iter()
            .find(|a| a.id == DEFAULT_ACCOUNT)
            .cloned()
            .unwrap_or_else(ClaudeAccount::default_account);
        std::iter::once(default)
            .chain(
                self.accounts
                    .iter()
                    .filter(|a| a.id != DEFAULT_ACCOUNT)
                    .cloned(),
            )
            .collect()
    }

    /// The account `id`, or the default account for `None`.
    pub fn account(&self, id: Option<&str>) -> Option<ClaudeAccount> {
        let id = id.unwrap_or(DEFAULT_ACCOUNT);
        self.all_accounts().into_iter().find(|a| a.id == id)
    }
}

/// The id of the account that runs `claude` as the user set it up, without
/// a configuration folder of Ovra's choosing.
pub const DEFAULT_ACCOUNT: &str = "default";

/// A Claude account: a Claude Code configuration folder with its own
/// sign-in, settings and transcripts. `claude` gets the folder as
/// `CLAUDE_CONFIG_DIR` (ADR 0006); Ovra never reads what is inside.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeAccount {
    /// Chosen by the core when the account is added; [`DEFAULT_ACCOUNT`]
    /// for the default one.
    pub id: String,
    /// The user's name for it, such as `Work`.
    pub name: String,
    /// Absolute path of its configuration folder, such as
    /// `/Users/me/.claude-work`; `None` (the default account only): Claude
    /// Code's own choice, `~/.claude` unless the user's environment says.
    #[serde(default)]
    pub config_dir: Option<String>,
}

impl ClaudeAccount {
    pub fn default_account() -> Self {
        Self {
            id: DEFAULT_ACCOUNT.to_owned(),
            name: "Default".to_owned(),
            config_dir: None,
        }
    }
}

/// One account and whether it is ready to run sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountState {
    pub account: ClaudeAccount,
    /// Its `claude`, checked with its folder: found, supported, signed in,
    /// and who pays.
    pub setup: AgentSetup,
}

/// The accounts, with what the user may want to add.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountsOverview {
    /// The default account first.
    pub accounts: Vec<AccountState>,
    /// Configuration folders found on this computer (`~/.claude-*`) that
    /// no account uses yet: suggestions only.
    pub found: Vec<String>,
    /// The asked project's account; `None` for the default one or when no
    /// project was asked about.
    pub project_account: Option<String>,
}

/// The project's own agent configuration, which loads only once the user
/// trusts the project. Agents read the project's instructions (`CLAUDE.md`)
/// either way.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectAgentConfig {
    /// Changes whenever anything that would load changes, including file
    /// contents the items do not show (a skill's script, an env value).
    pub fingerprint: String,
    pub items: Vec<ConfigItem>,
}

/// What a project would load now, next to what the user trusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfigReview {
    pub current: ProjectAgentConfig,
    /// What the user reviewed when they trusted the project; `None` if they
    /// do not trust it. When its fingerprint differs from `current`'s,
    /// sessions run without the project's configuration until the user
    /// trusts it again.
    pub trusted: Option<ProjectAgentConfig>,
}

impl ProjectConfigReview {
    /// Trusted, and nothing changed since.
    pub fn applies(&self) -> bool {
        self.trusted
            .as_ref()
            .is_some_and(|t| t.fingerprint == self.current.fingerprint)
    }
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
    /// Whole tools, or anything a shell or interpreter can do, allowed
    /// without asking: Ovra's approvals no longer see these calls.
    BroadAllowRules,
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
    /// The Claude accounts, each checked, and `project`'s account.
    #[serde(rename = "agents.accounts")]
    Accounts {
        #[serde(default)]
        project: Option<ProjectId>,
    },
    /// Adds an account (an empty `id`) or changes one, then answers
    /// `agents.accounts`. The default account keeps its folder.
    #[serde(rename = "agents.save_account")]
    SaveAccount { account: ClaudeAccount },
    /// Removes an account; projects that used it go back to the default
    /// one. Answers `agents.accounts`.
    #[serde(rename = "agents.remove_account")]
    RemoveAccount { id: String },
    /// Sets (or with `null` clears) the account `project`'s sessions start
    /// with when none is chosen. Answers `ok`.
    #[serde(rename = "agents.set_project_account")]
    SetProjectAccount {
        project: ProjectId,
        account: Option<String>,
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
        /// The Claude account; `None` uses the project's.
        #[serde(default)]
        account: Option<String>,
    },
    /// Sends the next message. Answers `ok`.
    #[serde(rename = "agents.send")]
    Send { session: SessionId, text: String },
    /// Continues an ended session, also one from before a restart, with a
    /// new prompt. Answers `agents.session`.
    #[serde(rename = "agents.resume")]
    Resume { session: SessionId, prompt: String },
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
    /// How many of the project's agents work, of how many may.
    #[serde(rename = "agents.capacity")]
    Capacity { project: ProjectId },
    /// How many agents may work at once in each project.
    #[serde(rename = "agents.limit")]
    Limit,
    /// Sets how many agents may work at once in each project; refused
    /// (`bad_request`) outside [`AgentLimit`]'s range. Queued workers start
    /// if it rose. Answers `agents.limit` and sends `agents.limit_changed`.
    #[serde(rename = "agents.set_limit")]
    SetLimit { limit: u32 },
    /// How many research sessions may work at once in each project
    /// (ADR 0013), apart from the agents of `agents.limit`.
    #[serde(rename = "agents.research_limit")]
    ResearchLimit,
    /// Sets it; refused (`bad_request`) outside [`AgentLimit`]'s range.
    /// Answers `agents.research_limit` and sends
    /// `agents.research_limit_changed`.
    #[serde(rename = "agents.set_research_limit")]
    SetResearchLimit { limit: u32 },
    /// The turns of a direct session that can be gone back to, newest first.
    #[serde(rename = "agents.checkpoints")]
    Checkpoints { session: SessionId },
    /// Sets the agent's changes since before `turn` back; files changed
    /// outside the agent since stay and are listed as conflicts. Refused
    /// (`conflict`) while a turn runs, `not_found` for a dropped turn.
    /// Answers `agents.went_back`.
    #[serde(rename = "agents.go_back")]
    GoBack { session: SessionId, turn: u32 },
    /// Puts back what the last go-back restored, until the next turn
    /// starts. Answers `agents.undid_go_back`.
    #[serde(rename = "agents.undo_go_back")]
    UndoGoBack { session: SessionId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentsResponse {
    #[serde(rename = "agents.setup")]
    Setup(AgentSetup),
    #[serde(rename = "agents.accounts")]
    Accounts(AccountsOverview),
    #[serde(rename = "agents.project_config")]
    ProjectConfig(ProjectConfigReview),
    #[serde(rename = "agents.list")]
    List(Vec<SessionInfo>),
    #[serde(rename = "agents.session")]
    Session(SessionInfo),
    #[serde(rename = "agents.transcript")]
    Transcript(Vec<AgentMessage>),
    #[serde(rename = "agents.capacity")]
    Capacity(AgentCapacity),
    #[serde(rename = "agents.limit")]
    Limit(AgentLimit),
    #[serde(rename = "agents.research_limit")]
    ResearchLimit(AgentLimit),
    #[serde(rename = "agents.checkpoints")]
    Checkpoints(Vec<Checkpoint>),
    #[serde(rename = "agents.went_back")]
    WentBack {
        /// Files set back, relative to the work dir.
        restored: Vec<String>,
        /// Files left as they are: changed outside the agent since.
        conflicts: Vec<String>,
        /// `agents.undo_go_back` can undo it.
        can_undo: bool,
    },
    #[serde(rename = "agents.undid_go_back")]
    UndidGoBack {
        /// Files put back as they were before the go-back.
        restored: Vec<String>,
        /// Files left as they are: changed since the go-back.
        conflicts: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentsEvent {
    /// The person changed how many agents may work at once; clients ask
    /// for capacities again.
    #[serde(rename = "agents.limit_changed")]
    LimitChanged(AgentLimit),
    /// The same for research sessions (`agents.research_limit`).
    #[serde(rename = "agents.research_limit_changed")]
    ResearchLimitChanged(AgentLimit),
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
    /// The agent said who pays for the session (`SessionInfo::billing`).
    #[serde(rename = "agents.billing")]
    Billing {
        session: SessionId,
        billing: Billing,
    },
    /// The runtime said how it runs the session's process
    /// (`SessionInfo::sandbox`; `None`: without a sandbox).
    #[serde(rename = "agents.sandbox")]
    Sandbox {
        session: SessionId,
        sandbox: Option<SandboxReport>,
    },
    /// The next piece of the assistant text the agent is writing now, for
    /// display only: the complete `AssistantText` message replaces what
    /// came so far, and the transcript never keeps these.
    #[serde(rename = "agents.text_delta")]
    TextDelta { session: SessionId, text: String },
}
