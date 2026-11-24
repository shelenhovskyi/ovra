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
};

export type TurnUsage = {
  input_tokens: number;
  output_tokens: number;
  /** Millionths of a US dollar, if the agent reports a cost. */
  cost_micro_usd: number | null;
};

/** `parent` is set when a subagent produced the message: the tool call that started it. */
export type AgentMessage =
  | { kind: "user_text"; text: string }
  | { kind: "assistant_text"; text: string; parent?: string }
  | { kind: "tool_use"; id: string; name: string; input: unknown; parent?: string }
  | { kind: "tool_result"; tool_use_id: string; output: string; is_error: boolean; parent?: string }
  | { kind: "turn_end"; usage: TurnUsage }
  | { kind: "error"; message: string };

export type AgentsRequest =
  | { method: "agents.list"; params: { project: ProjectId } }
  | {
      method: "agents.start";
      params: { project: ProjectId; agent: AgentKind; mode: SessionMode; prompt: string };
    }
  | { method: "agents.send"; params: { session: SessionId; text: string } }
  | { method: "agents.stop"; params: { session: SessionId } }
  | { method: "agents.transcript"; params: { session: SessionId } };

export type AgentsResponse =
  | { type: "agents.list"; data: SessionInfo[] }
  | { type: "agents.session"; data: SessionInfo }
  | { type: "agents.transcript"; data: AgentMessage[] };

export type AgentsEvent =
  | { type: "agents.message"; data: { session: SessionId; message: AgentMessage } }
  | { type: "agents.status"; data: { session: SessionId; project: ProjectId; status: AgentStatus } };
