//! `agents.*`: agent sessions, their messages and their state.

use serde::{Deserialize, Serialize};

use super::context::ContextItem;
use super::projects::ProjectId;

/// Assigned by the core when a session starts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub String);

/// Which agent runs the session: a provider (ADR 0011). One variant per
/// provider an adapter knows, so code that must treat providers apart
/// does not compile until it does.
/// The default is what settings and sessions saved before ADR 0011 ran
/// with.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    /// The user's installed Claude Code (ADR 0006).
    #[default]
    Claude,
    /// The user's installed Gemini CLI, over ACP (`docs/specs/more-agents.md`).
    Gemini,
    /// The user's installed Codex, through Zed's `codex-acp`, over ACP.
    Codex,
}

/// A provider the runtime can start sessions of, and what it can do
/// (`agents.providers`, ADR 0011).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: AgentKind,
    /// What the UI calls it, such as `Claude`.
    pub name: String,
    pub capabilities: Capabilities,
}

/// What a provider's sessions can do. The core refuses what one cannot
/// serve, and the UI leaves out what it cannot do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Asks before each tool call that needs a person's approval.
    pub approvals: bool,
    /// Ovra's policies hold even in modes that skip approvals (a hook);
    /// without it the agent alone decides when to ask.
    pub enforced_policy: bool,
    /// Reaches Ovra's IDE tools (coordinator, workers, servers, browser).
    pub ide_tools: bool,
    /// Can run as a read-only coordinator.
    pub read_only_role: bool,
    /// Continues an ended session (`agents.resume`).
    pub resume: bool,
    /// Branches a session into a new one.
    pub fork: bool,
    /// Stops a turn and stays alive (`agents.interrupt`).
    pub interrupt: bool,
    /// Streams the text it is writing (`agents.text_delta`).
    pub text_deltas: bool,
    /// Reports its subagents' messages.
    pub subagents: bool,
    /// Reports tokens used.
    pub usage: bool,
    /// Reports what a turn cost.
    pub cost: bool,
    /// Says who pays (`agents.billing`).
    pub billing: bool,
    /// Runs accounts with configuration folders of their own.
    pub accounts: bool,
    /// Sees images sent with a message (pasted screenshots).
    #[serde(default)]
    pub images: bool,
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

/// The Claude model a session asks for, as `claude --model` takes it
/// (`docs/specs/task-commands.md`). Closed set: nothing else reaches the
/// command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentModel {
    Opus,
    Sonnet,
    Haiku,
}

impl AgentModel {
    /// The alias `claude --model` takes.
    pub fn alias(self) -> &'static str {
        match self {
            Self::Opus => "opus",
            Self::Sonnet => "sonnet",
            Self::Haiku => "haiku",
        }
    }
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
    /// The Claude account it runs with ([`Account::id`]); `None` for
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
    /// A research session started without `WebSearch` (`agents.start`'s
    /// `no_web`): it only reads the project.
    #[serde(default)]
    pub no_web: bool,
    /// The model it asked for; `None` runs the account's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<AgentModel>,
    /// When it was started, in ms since the Unix epoch; `None` for sessions
    /// saved before Ovra kept it.
    #[serde(default)]
    pub started_ms: Option<u64>,
    /// Its latest message or change of status, in ms since the Unix epoch;
    /// `None` for sessions saved before Ovra kept it.
    #[serde(default)]
    pub active_ms: Option<u64>,
    /// Plan first (`docs/specs/plan-first.md`): the agent proposes a plan,
    /// and Ovra refuses its file changes until the person approves it
    /// (`agents.approve_plan`).
    #[serde(default)]
    pub planning: bool,
    /// What the person queued while a turn ran (`agents.queue`), oldest
    /// first: each goes as the next message when a turn ends.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queue: Vec<QueuedMessage>,
    /// It was working when Ovra last quit or crashed, and has not run
    /// since: `agents.resume_interrupted` continues it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub interrupted: bool,
    /// Why it may be stuck, while it works; `None` when it looks fine
    /// (`agents.stuck` sends changes).
    /// Boxed: most sessions have none, and `SessionInfo` travels in many
    /// replies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stuck: Option<Box<Stuck>>,
    /// The name of the schedule that started it (ADR 0017); `None` for a
    /// person's start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
}

/// Why a working session may be stuck (`docs/specs/stuck-and-resume.md`).
/// Only a hint: Ovra never stops an agent for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Stuck {
    /// The same tool call got the same result `times` times this turn.
    Repeating {
        /// The agent's own tool name.
        name: String,
        call: ToolCall,
        times: u32,
    },
    /// The last `times` tool calls all failed.
    Failing { times: u32 },
    /// Nothing came from the agent for `minutes` minutes while it worked.
    Silent { minutes: u32 },
}

/// A message that waits for the running turn to end (`agents.queue`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedMessage {
    /// Chosen by the core, unique within the session.
    pub id: u32,
    /// The person's text, without the `@` context that goes with it.
    pub text: String,
}

/// A session `agents.history` found (`docs/specs/task-history.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub session: SessionInfo,
    /// The line of the conversation around the first match; `None` when the
    /// title or the project's name matched, or nothing was searched for.
    pub snippet: Option<Snippet>,
}

/// A piece of a message, with the match marked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    pub text: String,
    /// The first matched word's range in `text`, in UTF-16 code units.
    pub from: u32,
    pub to: u32,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCapacity {
    /// Sessions that do work now: direct sessions and workers that have not
    /// ended. Coordinators only plan and wait for their workers, and a
    /// worker that asked for review waits idle for the person: neither
    /// counts.
    pub working: u32,
    /// The most that may work at once.
    pub limit: u32,
    /// Sessions waiting for a free place (`queued`), and workers still
    /// being set up to wait.
    pub queued: u32,
    /// Research sessions that work now (ADR 0013): counted apart from
    /// `working`, against `research_limit`.
    #[serde(default)]
    pub research_working: u32,
    #[serde(default)]
    pub research_limit: u32,
    /// The waiting sessions, in the order they start.
    #[serde(default)]
    pub waiting: Vec<SessionId>,
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

/// A session's transcript (`agents.transcript`). Replies and events travel
/// apart, so a message can arrive both here and as an `agents.message`
/// afterwards: `seq` tells them apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTranscript {
    /// The messages kept, oldest first.
    pub messages: Vec<AgentMessage>,
    /// The number of the last of them (`agents.message`'s `seq`); 0 when empty.
    pub seq: u64,
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
        /// The agent's own name and input, for display and the audit log.
        name: String,
        input: serde_json::Value,
        /// What the call does in shared terms. Transcripts saved before
        /// Ovra recorded it load as `other`.
        #[serde(default)]
        call: ToolCall,
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

/// What a tool call does, in the terms every agent shares (ADR 0011): the
/// runtime maps its own tool names and inputs to these, and shared code
/// (policies, direct mode, the audit log, approvals, the transcript) reads
/// them next to the raw name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub kind: ToolKind,
    /// The files the call reads or changes, as the agent wrote them
    /// (absolute, or relative to the session's folder).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// The shell command an `execute` call runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// The URL a `fetch` call reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A tool call's kind: ACP's list, plus `subagent` for a call that starts a
/// helper agent, whose own calls follow with it as their `parent`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Execute,
    Think,
    Fetch,
    Subagent,
    /// Anything else, and a kind Ovra does not know yet.
    #[default]
    #[serde(other)]
    Other,
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
    /// Absolute paths of the other providers' executables the user chose;
    /// a provider not listed is searched for.
    pub paths: std::collections::BTreeMap<AgentKind, String>,
    /// The Claude accounts the user added. The default account is
    /// [`Account::default_account`] unless an entry with its id
    /// renames it.
    pub accounts: Vec<Account>,
    /// Each project's account ([`Account::id`]) for sessions started
    /// without one; a project not listed uses the default account.
    pub project_accounts: std::collections::BTreeMap<ProjectId, String>,
    /// How many agents may work at once in one project
    /// ([`AgentLimit`]); `None` is the default.
    pub max_working: Option<u32>,
    /// How many research sessions may work at once in one project (ADR
    /// 0013); `None` is the default.
    pub research_limit: Option<u32>,
    /// Whether schedules start agents (ADR 0017); off by default.
    pub schedules_enabled: bool,
    /// The person's schedules of every project, oldest first. A stored
    /// `next_ms` is when the next run is due, also while paused or off.
    /// One that cannot be read is dropped, so it never hides the rest.
    #[serde(deserialize_with = "readable")]
    pub schedules: Vec<super::schedules::Schedule>,
    /// How many schedules were ever made, for their ids.
    pub schedules_made: u64,
}

/// The entries of a list that can be read.
fn readable<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let values = Vec::<serde_json::Value>::deserialize(deserializer)?;
    Ok(values
        .into_iter()
        .filter_map(|v| {
            serde_json::from_value(v)
                .map_err(|e| tracing::warn!("dropped a saved entry that cannot be read: {e}"))
                .ok()
        })
        .collect())
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
    /// The executable the user chose for `agent`, if any.
    pub fn path(&self, agent: AgentKind) -> Option<&str> {
        match agent {
            AgentKind::Claude => self.claude_path.as_deref(),
            other => self.paths.get(&other).map(String::as_str),
        }
    }

    /// Chooses (or with `None` clears) the executable of `agent`.
    pub fn set_path(&mut self, agent: AgentKind, path: Option<String>) {
        match (agent, path) {
            (AgentKind::Claude, path) => self.claude_path = path,
            (other, Some(path)) => {
                self.paths.insert(other, path);
            }
            (other, None) => {
                self.paths.remove(&other);
            }
        }
    }

    /// Every account, the default one first.
    pub fn all_accounts(&self) -> Vec<Account> {
        let default = self
            .accounts
            .iter()
            .find(|a| a.id == DEFAULT_ACCOUNT)
            .cloned()
            .unwrap_or_else(Account::default_account);
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
    pub fn account(&self, id: Option<&str>) -> Option<Account> {
        let id = id.unwrap_or(DEFAULT_ACCOUNT);
        self.all_accounts().into_iter().find(|a| a.id == id)
    }
}

/// The most messages that wait in one session's queue.
pub const MAX_QUEUED: usize = 10;

/// The most entries `agents.history` answers with.
pub const HISTORY_LIMIT: usize = 200;

/// The id of the account that runs `claude` as the user set it up, without
/// a configuration folder of Ovra's choosing.
pub const DEFAULT_ACCOUNT: &str = "default";

/// An account of one provider: for Claude, a Claude Code configuration
/// folder with its own sign-in, settings and transcripts, which `claude`
/// gets as `CLAUDE_CONFIG_DIR` (ADR 0006). Ovra never reads what is
/// inside.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Whose account it is; accounts saved before ADR 0011 are Claude's.
    #[serde(default)]
    pub provider: AgentKind,
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

impl Account {
    pub fn default_account() -> Self {
        Self {
            provider: AgentKind::Claude,
            id: DEFAULT_ACCOUNT.to_owned(),
            name: "Default".to_owned(),
            config_dir: None,
        }
    }
}

/// One account and whether it is ready to run sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountState {
    pub account: Account,
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
    /// The specific entries: hook commands, `NAME = value` variables (a
    /// secret-looking value masked to a digest), rules, names.
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
    /// Whole tools, anything a shell or interpreter can do, network,
    /// publish or delete commands, or edits outside the project, allowed
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
    /// The providers the runtime can start, with their capabilities.
    #[serde(rename = "agents.providers")]
    Providers,
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
    SaveAccount { account: Account },
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
    /// Sessions of every project, or of `project`, whose title, project
    /// name or conversation has every word of `query`; newest activity
    /// first, at most [`HISTORY_LIMIT`].
    #[serde(rename = "agents.history")]
    History {
        #[serde(default)]
        query: String,
        #[serde(default)]
        project: Option<ProjectId>,
    },
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
        /// What the person added with `@`; it follows the prompt as Ovra's.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        context: Vec<ContextItem>,
        /// Research only: no `WebSearch` either, for prompts that carry
        /// other people's text (ADR 0013).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        no_web: bool,
        /// Claude only: the model to ask for; `None` runs the account's
        /// default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<AgentModel>,
        /// Direct only: the agent proposes a plan first and changes no
        /// file until the person approves it (`docs/specs/plan-first.md`).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        plan_first: bool,
    },
    /// Sends the next message. Answers `ok`.
    #[serde(rename = "agents.send")]
    Send {
        session: SessionId,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        context: Vec<ContextItem>,
    },
    /// Sends the next message once the running turn ends, or now if none
    /// runs; also for a session that waits to start. Refused (`conflict`)
    /// when [`MAX_QUEUED`] wait already. Answers `ok`; `agents.queue`
    /// tells the new queue.
    #[serde(rename = "agents.queue")]
    Queue {
        session: SessionId,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        context: Vec<ContextItem>,
    },
    /// Takes a queued message back; `not_found` once it was sent. Answers
    /// `ok`.
    #[serde(rename = "agents.unqueue")]
    Unqueue { session: SessionId, id: u32 },
    /// The person approves the plan of a session in plan first: Ovra lets
    /// it change files again and tells it to go ahead. Refused (`conflict`)
    /// while a turn runs. Answers `ok`.
    #[serde(rename = "agents.approve_plan")]
    ApprovePlan { session: SessionId },
    /// Continues an ended session, also one from before a restart, with a
    /// new prompt. Answers `agents.session`.
    #[serde(rename = "agents.resume")]
    Resume { session: SessionId, prompt: String },
    /// Continues the sessions that Ovra's quit cut off
    /// ([`SessionInfo::interrupted`]): `session`, or all of `project`'s,
    /// coordinators first, with Ovra's note that it was cut off. Answers
    /// `agents.resumed`.
    #[serde(rename = "agents.resume_interrupted")]
    ResumeInterrupted {
        project: ProjectId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<SessionId>,
    },
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

/// A session `agents.resume_interrupted` could not continue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotResumed {
    pub session: SessionId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentsResponse {
    #[serde(rename = "agents.providers")]
    Providers(Vec<ProviderInfo>),
    #[serde(rename = "agents.setup")]
    Setup(AgentSetup),
    #[serde(rename = "agents.accounts")]
    Accounts(AccountsOverview),
    #[serde(rename = "agents.project_config")]
    ProjectConfig(ProjectConfigReview),
    #[serde(rename = "agents.list")]
    List(Vec<SessionInfo>),
    #[serde(rename = "agents.history")]
    History(Vec<HistoryEntry>),
    #[serde(rename = "agents.session")]
    Session(SessionInfo),
    #[serde(rename = "agents.transcript")]
    Transcript(SessionTranscript),
    #[serde(rename = "agents.capacity")]
    Capacity(AgentCapacity),
    #[serde(rename = "agents.limit")]
    Limit(AgentLimit),
    #[serde(rename = "agents.research_limit")]
    ResearchLimit(AgentLimit),
    #[serde(rename = "agents.checkpoints")]
    Checkpoints(Vec<Checkpoint>),
    #[serde(rename = "agents.resumed")]
    Resumed {
        resumed: Vec<SessionId>,
        /// The ones that could not continue, each with why.
        left: Vec<NotResumed>,
    },
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
        /// The message's number in the session, from 1, counting every
        /// message ever added: one a client already has from
        /// `agents.transcript` (`seq` at most the transcript's) is not new.
        seq: u64,
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
    /// The session's queue changed (`SessionInfo::queue`).
    #[serde(rename = "agents.queue")]
    Queue {
        session: SessionId,
        queue: Vec<QueuedMessage>,
    },
    /// The session entered or left plan first (`SessionInfo::planning`).
    #[serde(rename = "agents.planning")]
    Planning { session: SessionId, planning: bool },
    /// A working session may be stuck, or no longer looks so (`None`).
    #[serde(rename = "agents.stuck")]
    Stuck {
        session: SessionId,
        stuck: Option<Stuck>,
    },
}
