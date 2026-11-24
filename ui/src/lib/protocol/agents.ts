import type { ProjectId } from "./projects";

export type SessionId = string;

/** `claude`: the user's installed Claude Code (ADR 0006). */
export type AgentKind = "claude";

/** ADR 0005: `direct` works in the main checkout, `isolated` in a worktree (phase 3). */
export type SessionMode = "direct" | "isolated";

/** Closed set: the UI shows exactly these states. */
export type AgentStatus =
  | { state: "starting" }
  | { state: "working" }
  | { state: "waiting_for_approval" }
  | { state: "idle" }
  | { state: "stopped" }
  | { state: "failed"; message: string };

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
};

export type TurnUsage = {
  input_tokens: number;
  output_tokens: number;
  /** Millionths of a US dollar, if the agent reports a cost. */
  cost_micro_usd: number | null;
};

/** Who wrote a message an agent was sent: the person, the task's coordinator, or Ovra in its own words. */
export type MessageFrom = "person" | "coordinator" | "ovra";

/** `parent` is set when a subagent produced the message: the tool call that started it. */
export type AgentMessage =
  | { kind: "user_text"; text: string; from: MessageFrom }
  | { kind: "assistant_text"; text: string; parent?: string }
  | { kind: "tool_use"; id: string; name: string; input: unknown; parent?: string }
  | { kind: "tool_result"; tool_use_id: string; output: string; is_error: boolean; parent?: string }
  | { kind: "turn_end"; usage: TurnUsage }
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
  | { method: "agents.project_config"; params: { project: ProjectId } }
  | { method: "agents.list"; params: { project: ProjectId } }
  | {
      method: "agents.start";
      params: { project: ProjectId; agent: AgentKind; mode: SessionMode; prompt: string };
    }
  | { method: "agents.send"; params: { session: SessionId; text: string } }
  | { method: "agents.resume"; params: { session: SessionId; prompt: string } }
  | { method: "agents.interrupt"; params: { session: SessionId } }
  | { method: "agents.stop"; params: { session: SessionId } }
  | { method: "agents.transcript"; params: { session: SessionId } };

export type AgentsResponse =
  | { type: "agents.setup"; data: AgentSetup }
  | { type: "agents.project_config"; data: ProjectConfigReview }
  | { type: "agents.list"; data: SessionInfo[] }
  | { type: "agents.session"; data: SessionInfo }
  | { type: "agents.transcript"; data: AgentMessage[] };

export type AgentsEvent =
  | { type: "agents.message"; data: { session: SessionId; message: AgentMessage } }
  | { type: "agents.status"; data: { session: SessionId; project: ProjectId; status: AgentStatus } };
