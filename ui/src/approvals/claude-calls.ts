// For tests: a call of one of Claude's tools in shared terms, as the Claude
// runtime maps it (`crates/agent-runtime/src/tools.rs`).

import type { ToolCall, ToolKind } from "../lib/protocol";

const KINDS: Record<string, ToolKind> = {
  Bash: "execute",
  Read: "read",
  NotebookRead: "read",
  Edit: "edit",
  Write: "edit",
  MultiEdit: "edit",
  NotebookEdit: "edit",
  Glob: "search",
  Grep: "search",
  LS: "search",
  WebSearch: "search",
  WebFetch: "fetch",
  Task: "subagent",
  Agent: "subagent",
  TodoWrite: "think",
};

export function claudeCall(name: string, input: unknown): ToolCall {
  const fields = typeof input === "object" && input !== null ? (input as Record<string, unknown>) : {};
  const text = (key: string) => (typeof fields[key] === "string" ? (fields[key] as string) : undefined);
  const kind = KINDS[name] ?? "other";
  const path = name.startsWith("Notebook") ? text("notebook_path") : text("file_path");
  return {
    kind,
    ...((kind === "read" || kind === "edit") && path !== undefined ? { paths: [path] } : {}),
    ...(kind === "execute" && text("command") !== undefined ? { command: text("command") } : {}),
    ...(kind === "fetch" && text("url") !== undefined ? { url: text("url") } : {}),
  };
}
