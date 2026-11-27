// Plain-language summaries of what a tool call would do, for approval cards,
// transcripts and notifications, by the call's kind (any agent's). Unknown
// tools show their raw input.

import type { PushPlan, ToolCall } from "../lib/protocol";

type Detail = { label: string; text: string };

export type ToolSummary = {
  /** One line, e.g. `Run ls -la` or `Edit README.md`. */
  action: string;
  /** What `action` ends with when that is code or a path (`ls -la`, `README.md`), shown in mono; else empty. */
  object: string;
  /** What a person should read before allowing it. */
  details: Detail[];
};

const MAX_LINE = 120;

function str(input: unknown, key: string): string | undefined {
  if (typeof input !== "object" || input === null) return undefined;
  const value = (input as Record<string, unknown>)[key];
  return typeof value === "string" ? value : undefined;
}

function oneLine(text: string): string {
  const line = text.replace(/\s+/g, " ").trim();
  return line.length > MAX_LINE ? `${line.slice(0, MAX_LINE - 1)}…` : line;
}

function raw(input: unknown): Detail {
  return { label: "Input", text: JSON.stringify(input, null, 2) ?? String(input) };
}

/**
 * Tools whose verb says more than their kind's (`Write` is an edit that
 * replaces the whole file, `Glob` a search by name); display only.
 */
const VERBS: Record<string, string> = { Write: "Write", Glob: "Find files" };

/**
 * `call` is what the call does in shared terms (absent: `other`); the agent's
 * own name and input only add wording and details.
 */
export function describeTool(tool: string, input: unknown, call: ToolCall | undefined): ToolSummary {
  const kind = call?.kind ?? "other";
  const file = call?.paths?.[0];
  const pick = (...keys: string[]): Detail[] =>
    keys.flatMap((key) => {
      const text = str(input, key);
      return text === undefined ? [] : [{ label: key.replace(/_/g, " "), text }];
    });
  const done = (verb: string, object: string, details: Detail[]): ToolSummary => ({
    action: `${verb} ${object}`,
    object,
    details,
  });

  if (tool === PUSH_TOOL) {
    const push = describePush(input);
    if (push) return push;
  }
  if (tool === KEY_TOOL) {
    const key = str(input, "key");
    const host = str(input, "host");
    if (key !== undefined && host !== undefined) {
      return {
        action: `Use your ${key} key on ${host}`,
        object: "",
        details: [
          {
            label: "what happens",
            text: `Ovra adds the key to the agent's requests to ${host}. The agent never sees it.`,
          },
        ],
      };
    }
  }
  const ide = isIdeTool(tool) ? describeIdeTool(tool.slice(IDE_PREFIX.length), input) : null;
  if (ide) return { action: ide, object: "", details: pick("task", "text", "summary", "path") };
  if (kind === "execute" && call?.command !== undefined) {
    const command = call.command;
    return done("Run", oneLine(command), [{ label: "command", text: command }, ...pick("description")]);
  }
  if (kind === "edit" && file !== undefined) {
    const verb = VERBS[tool] ?? "Edit";
    const head = { label: "file", text: file };
    if (verb === "Write") return done(verb, file, [head, ...pick("content")]);
    const edits = pick("old_string", "new_string", "new_source");
    return done(verb, file, edits.length > 0 ? [head, ...edits] : [head, raw(input)]);
  }
  if (kind === "subagent") {
    const what = str(input, "description") ?? str(input, "prompt");
    return {
      action: what ? `Start a helper agent: ${oneLine(what)}` : "Start a helper agent",
      object: "",
      details: pick("description", "prompt"),
    };
  }
  if (kind === "read" && file !== undefined) return done("Read", file, [{ label: "file", text: file }]);
  const pattern = str(input, "pattern");
  if (kind === "search" && pattern !== undefined) {
    return done(VERBS[tool] ?? "Search", oneLine(pattern), pick("pattern", "path"));
  }
  if (kind === "fetch" && call?.url !== undefined) {
    return done("Fetch", oneLine(call.url), [{ label: "url", text: call.url }, ...pick("prompt")]);
  }
  return { action: `Use ${tool}`, object: "", details: [raw(input)] };
}

/** The tool name of a worker's push put to the person (`docs/specs/agent-push.md`); its input is a `PushPlan`. */
export const PUSH_TOOL = "Push";

/**
 * The tool name of a key the netproxy would add to a session's requests (`docs/specs/secrets-consent.md`); its input
 * is `{ key, host }`, and an allow holds for the session.
 */
export const KEY_TOOL = "UseKey";

const KIND_LETTER: Record<string, string> = { added: "A", modified: "M", deleted: "D", renamed: "R" };

/** A worker's push: where it goes, what it builds on, its files and message. `null` for input of another shape. */
function describePush(input: unknown): ToolSummary | null {
  const plan = input as Partial<PushPlan> | null;
  const remote = str(input, "remote");
  const branch = str(input, "branch");
  if (remote === undefined || branch === undefined || !Array.isArray(plan?.files)) return null;
  const host = str(input, "host");
  const base = str(input, "base");
  const on = host ? ` on ${host}` : "";
  const where = plan.new_branch
    ? `A new branch from ${remote}/${base ?? "?"}${on}`
    : `One new commit on top of ${remote}/${branch}${on}`;
  const details: Detail[] = [{ label: "where", text: where }];
  if (plan.protected_branch === true) {
    details.push({ label: "shared branch", text: "People and CI use this branch; the push lands there for everyone." });
  } else if (plan.own_branch === false) {
    details.push({ label: "not its own branch", text: "The worker pushes to a branch other than its own." });
  }
  details.push({ label: "runs in CI", text: "The pushed code runs in this repository's CI, with its secrets." });
  const files = plan.files.map((f) => {
    const letter = KIND_LETTER[f.kind] ?? "?";
    const path = f.from ? `${f.from} → ${f.path}` : f.path;
    return `${letter} ${path}  +${f.additions} −${f.deletions}`;
  });
  details.push({ label: files.length === 1 ? "1 file" : `${files.length} files`, text: files.join("\n") });
  const message = str(input, "message");
  if (message) details.push({ label: "commit message", text: message });
  const object = `${remote}/${branch}`;
  return { action: `Push to ${object}`, object, details };
}

/** "Always allow" of a push rule (`origin ovra/*`) in words: the branches and the remote. */
export function pushScope(match: string): { branches: string; remote: string } | null {
  const [remote, branch, ...rest] = match.split(" ");
  if (!remote || !branch || rest.length > 0) return null;
  return { remote, branches: branch.endsWith("/*") ? `${branch.slice(0, -1)}…` : branch };
}

const IDE_PREFIX = "mcp__ovra__";
const IDE_TOOLS = new Set([
  "spawn_agent",
  "list_agents",
  "read_output",
  "send_message",
  "stop_agent",
  "request_review",
  "open_in_ui",
]);

/** Whether `tool` is one of Ovra's own tools, by its exact name. */
function isIdeTool(tool: string): boolean {
  return tool.startsWith(IDE_PREFIX) && IDE_TOOLS.has(tool.slice(IDE_PREFIX.length));
}

/** A worker name or session id as the tools allow it; anything else is not pasted into a sentence. */
const ID = /^[a-z0-9][a-z0-9-]{0,39}$/;
const id = (input: unknown, key: string): string | undefined => {
  const value = str(input, key);
  return value !== undefined && ID.test(value) ? value : undefined;
};

/** Ovra's own tools (`mcp__ovra__*`), which a task's coordinator and workers call. */
function describeIdeTool(name: string, input: unknown): string | null {
  const agent = id(input, "agent_id");
  const worker = agent ? `worker ${agent}` : "a worker";
  switch (name) {
    case "spawn_agent": {
      const task = str(input, "task");
      const named = id(input, "name");
      return `Start ${named ? `worker ${named}` : "a worker"}${task ? `: ${oneLine(task)}` : ""}`;
    }
    case "list_agents":
      return "List the task's workers";
    case "read_output":
      return `Read the output of ${worker}`;
    case "send_message":
      return `Message ${worker}`;
    case "stop_agent":
      return `Stop ${worker}`;
    case "request_review": {
      const summary = str(input, "summary");
      return `Ask for review${agent ? ` of ${worker}` : ""}${summary ? `: ${oneLine(summary)}` : ""}`;
    }
    case "open_in_ui": {
      const target = str(input, "path") ?? (agent ? worker : str(input, "review_id") ? "a review" : "something");
      return `Show ${target} in Ovra`;
    }
    default:
      return null;
  }
}
