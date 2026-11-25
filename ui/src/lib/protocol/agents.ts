import type { ProjectId } from "./projects";

export type SessionId = string;

/** `claude`: the user's installed Claude Code (ADR 0006). */
export type AgentKind = "claude";

/** ADR 0005: `direct` works in the main checkout, `isolated` in a worktree (phase 3). */
export type SessionMode = "direct" | "isolated";

/** Closed set: the UI shows exactly these states. */
export type AgentStatus =
  /** The project has as many agents working as it may; it starts when one finishes. */
  | { state: "queued" }
  /** A worker's worktree is prepared (`.worktreeinclude`, `.ovra/worktree.json`); counts as working. `Worker.setup` has the step. */
  | { state: "preparing" }
  | { state: "starting" }
  | { state: "working" }
  | { state: "waiting_for_approval" }
  | { state: "idle" }
  | { state: "stopped" }
  | { state: "failed"; message: string };

/** How many agents of a project work at once, of how many it may run. */
export type AgentCapacity = {
  /** Direct sessions and workers that have not ended; not coordinators, nor workers idle after asking for review. */
  working: number;
  limit: number;
  /** Workers waiting for a free place. */
  queued: number;
};

export type SessionInfo = {
  id: SessionId;
  project: ProjectId;
  agent: AgentKind;
  mode: SessionMode;
  status: AgentStatus;
  /** The first line of the first prompt. */
  title: string;
  /** The sum of every finished turn's usage. */
  usage: TurnUsage;
  /** Whether `agents.resume` can continue it: it ended and the agent reported its own id. */
  resumable: boolean;
  /** Who pays for the session's usage, as the agent said when it started. */
  billing: Billing | null;
  /** The sandbox the runtime ran it in; null while unknown or without one (`sandbox_reported` tells them apart). */
  sandbox: SandboxReport | null;
  /** Whether the runtime reported how it runs the current process: false while it starts. Sessions saved before the field read as false; a set `sandbox` counts as reported. */
  sandbox_reported: boolean;
  /** The Claude account it runs with (`ClaudeAccount.id`); null for the default account. */
  account: string | null;
  /** Files were set back to before this turn (from 1): it and later turns show as undone (absent from older cores). */
  undone_from?: number | null;
  /** `agents.undo_go_back` can put back what the last go-back restored (absent from older cores). */
  can_undo_go_back?: boolean;
};

/** A direct session's turn whose files can be set back to before it. */
export type Checkpoint = {
  /** From 1. */
  turn: number;
  /** When its message was sent, ms since the Unix epoch. */
  at_ms: number;
  /** The first line of its message. */
  message: string;
  files_changed: number;
};

/** How the runtime actually ran a session. */
export type SandboxReport = {
  name: string;
  network: "blocked" | "proxy" | "open";
  /** What the sandbox does not cover, each a short phrase such as "DNS lookups"; empty when none are known. */
  gaps: string[];
};

export type TurnUsage = {
  input_tokens: number;
  output_tokens: number;
  /** Millionths of a US dollar, if the agent reports a cost. */
  cost_micro_usd: number | null;
};

/** Who wrote a message an agent was sent: the person, the task's coordinator, Ovra in its own words, or `unknown` for one saved before Ovra recorded it. */
export type MessageFrom = "person" | "coordinator" | "ovra" | "unknown";

/** `parent` is set when a subagent produced the message: the tool call that started it. */
export type AgentMessage =
  | { kind: "user_text"; text: string; from: MessageFrom }
  | { kind: "assistant_text"; text: string; parent?: string }
  | { kind: "tool_use"; id: string; name: string; input: unknown; parent?: string }
  | { kind: "tool_result"; tool_use_id: string; output: string; is_error: boolean; parent?: string }
  | { kind: "turn_end"; usage: TurnUsage }
  /** The person stopped the turn before it finished; no failure. */
  | { kind: "interrupted" }
  | { kind: "error"; message: string };

/** Whether an agent can run on this machine. */
export type AgentSetup = {
  /** The executable the user chose; `null` searches the usual places. */
  path: string | null;
  /** The executable that was checked, if one was found. */
  executable: string | null;
  version: string | null;
  state: SetupState;
  /** Where usage is billed, when the agent says. */
  billing: Billing | null;
  /** `ANTHROPIC_API_KEY` is set for agents: Claude Code bills that key instead of a subscription. */
  api_key_in_env: boolean;
};

/** Who pays for an agent's usage. Closed set: the UI explains each one. */
export type Billing = { kind: "subscription" } | { kind: "api_key" } | { kind: "cloud"; provider: string };

/** Closed set: the UI explains each one. */
export type SetupState =
  | { state: "ready" }
  | { state: "not_found" }
  | { state: "outdated"; minimum: string }
  | { state: "too_new"; below: string }
  | { state: "signed_out" }
  | { state: "broken"; message: string };

/** A Claude account: a Claude Code configuration folder with its own sign-in, given to claude as CLAUDE_CONFIG_DIR. */
export type ClaudeAccount = {
  /** Chosen by the core when added (send ""); `"default"` for the default account, which runs claude as the user set it up. */
  id: string;
  name: string;
  /** Full path of its configuration folder; null (the default account only): Claude Code's own folder. */
  config_dir: string | null;
};

/** One account and whether it is ready to run sessions. */
export type AccountState = { account: ClaudeAccount; setup: AgentSetup };

export type AccountsOverview = {
  /** The default account first. */
  accounts: AccountState[];
  /** Configuration folders found on this computer that no account uses yet: suggestions only. */
  found: string[];
  /** The asked project's account; null for the default one or when no project was asked about. */
  project_account: string | null;
};

/** The project's own agent configuration, which loads only once the project is trusted. */
export type ProjectAgentConfig = {
  /** Changes with anything that would load, even contents the items do not show. */
  fingerprint: string;
  items: ConfigItem[];
};

/**
 * What would load now, next to what the user trusted (`null`: not trusted).
 * Different fingerprints mean the settings changed, and they do not load
 * until the user trusts them again.
 */
export type ProjectConfigReview = { current: ProjectAgentConfig; trusted: ProjectAgentConfig | null };

export type ConfigItem = {
  /** Relative to the project root. */
  path: string;
  kind: ConfigKind;
  /** Hook commands, variable names, rules or names. */
  entries: string[];
};

/** `hooks`, `helpers` and `plugins` run code on the user's machine; `broad_allow_rules` skip Ovra's approvals. */
export type ConfigKind =
  | "hooks"
  | "helpers"
  | "plugins"
  | "env"
  /** Whole tools or any shell command allowed without asking: Ovra's approvals no longer see them. */
  | "broad_allow_rules"
  | "allow_rules"
  | "settings"
  | "agents"
  | "commands"
  | "skills"
  | "unreadable";

export type AgentsRequest =
  | { method: "agents.setup"; params: { agent: AgentKind } }
  /** `path: null` clears the chosen executable. Answers `agents.setup`. */
  | { method: "agents.set_path"; params: { agent: AgentKind; path: string | null } }
  | { method: "agents.accounts"; params: { project: ProjectId | null } }
  /** `id: ""` adds an account. Answers `agents.accounts`. */
  | { method: "agents.save_account"; params: { account: ClaudeAccount } }
  /** Projects that used it go back to the default account. Answers `agents.accounts`. */
  | { method: "agents.remove_account"; params: { id: string } }
  /** `account: null` goes back to the default account. Answers `ok`. */
  | { method: "agents.set_project_account"; params: { project: ProjectId; account: string | null } }
  | { method: "agents.project_config"; params: { project: ProjectId } }
  | { method: "agents.list"; params: { project: ProjectId } }
  | {
      method: "agents.start";
      /** `account`: a `ClaudeAccount.id`; null or left out uses the project's. */
      params: {
        project: ProjectId;
        agent: AgentKind;
        mode: SessionMode;
        prompt: string;
        account?: string | null;
      };
    }
  | { method: "agents.send"; params: { session: SessionId; text: string } }
  | { method: "agents.resume"; params: { session: SessionId; prompt: string } }
  | { method: "agents.interrupt"; params: { session: SessionId } }
  | { method: "agents.stop"; params: { session: SessionId } }
  | { method: "agents.transcript"; params: { session: SessionId } }
  | { method: "agents.capacity"; params: { project: ProjectId } }
  | { method: "agents.limit" }
  /** Refused (`bad_request`) outside `min`..`max`; queued workers start if it rose. */
  | { method: "agents.set_limit"; params: { limit: number } }
  /** Newest first. */
  | { method: "agents.checkpoints"; params: { session: SessionId } }
  /** Refused (`conflict`) while a turn runs; `not_found` for a dropped turn. */
  | { method: "agents.go_back"; params: { session: SessionId; turn: number } }
  /** Until the next turn starts. */
  | { method: "agents.undo_go_back"; params: { session: SessionId } };

export type AgentsResponse =
  | { type: "agents.setup"; data: AgentSetup }
  | { type: "agents.accounts"; data: AccountsOverview }
  | { type: "agents.project_config"; data: ProjectConfigReview }
  | { type: "agents.list"; data: SessionInfo[] }
  | { type: "agents.session"; data: SessionInfo }
  | { type: "agents.transcript"; data: AgentMessage[] }
  | { type: "agents.capacity"; data: AgentCapacity }
  | { type: "agents.limit"; data: AgentLimit }
  | { type: "agents.checkpoints"; data: Checkpoint[] }
  | { type: "agents.went_back"; data: { restored: string[]; conflicts: string[]; can_undo: boolean } }
  | { type: "agents.undid_go_back"; data: { restored: string[]; conflicts: string[] } };

/** The methods of going back to an earlier turn (`docs/specs/checkpoints.md`). */
export type CheckpointMethod = "agents.checkpoints" | "agents.go_back" | "agents.undo_go_back";

/** The response a checkpoint method resolves to. */
export type CheckpointResponseFor<M extends CheckpointMethod> = M extends "agents.checkpoints"
  ? Extract<AgentsResponse, { type: "agents.checkpoints" }>
  : M extends "agents.go_back"
    ? Extract<AgentsResponse, { type: "agents.went_back" }>
    : Extract<AgentsResponse, { type: "agents.undid_go_back" }>;

/** How many agents may work at once in each project, and the range to choose from. */
export interface AgentLimit {
  limit: number;
  default: number;
  min: number;
  max: number;
}

export type AgentsEvent =
  /** The person changed the limit; ask for capacities again. */
  | { type: "agents.limit_changed"; data: AgentLimit }
  | { type: "agents.message"; data: { session: SessionId; message: AgentMessage } }
  | { type: "agents.status"; data: { session: SessionId; project: ProjectId; status: AgentStatus } }
  /** The agent said who pays for the session (`SessionInfo.billing`). */
  | { type: "agents.billing"; data: { session: SessionId; billing: Billing } }
  /** The runtime said how it runs the session's process (`SessionInfo.sandbox`; null: without a sandbox). */
  | { type: "agents.sandbox"; data: { session: SessionId; sandbox: SandboxReport | null } }
  /**
   * The next piece of the assistant text the agent is writing now, for display
   * only: the complete `assistant_text` message replaces it.
   */
  | { type: "agents.text_delta"; data: { session: SessionId; text: string } };
