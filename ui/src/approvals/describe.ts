// Plain-language summaries of what a tool call would do, for approval cards,
// transcripts and notifications. Unknown tools show their raw input.

type Detail = { label: string; text: string };

export type ToolSummary = {
  /** One line, e.g. `Run ls -la` or `Edit README.md`. */
  action: string;
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

export function describeTool(tool: string, input: unknown): ToolSummary {
  const command = str(input, "command");
  const file = str(input, "file_path") ?? str(input, "notebook_path");
  const pick = (...keys: string[]): Detail[] =>
    keys.flatMap((key) => {
      const text = str(input, key);
      return text === undefined ? [] : [{ label: key.replace(/_/g, " "), text }];
    });

  const ide = isIdeTool(tool) ? describeIdeTool(tool.slice(IDE_PREFIX.length), input) : null;
  if (ide) return { action: ide, details: pick("task", "text", "summary", "path") };
  if (tool === "Bash" && command !== undefined) {
    return { action: `Run ${oneLine(command)}`, details: pick("command", "description") };
  }
  if ((tool === "Edit" || tool === "MultiEdit" || tool === "NotebookEdit") && file !== undefined) {
    const edits = pick("old_string", "new_string", "new_source");
    return {
      action: `Edit ${file}`,
      details:
        edits.length > 0 ? [{ label: "file", text: file }, ...edits] : [{ label: "file", text: file }, raw(input)],
    };
  }
  if (tool === "Agent" || tool === "Task") {
    const what = str(input, "description") ?? str(input, "prompt");
    return {
      action: what ? `Start a helper agent: ${oneLine(what)}` : "Start a helper agent",
      details: pick("description", "prompt"),
    };
  }
  if (tool === "Write" && file !== undefined) {
    return { action: `Write ${file}`, details: [{ label: "file", text: file }, ...pick("content")] };
  }
  if (tool === "Read" && file !== undefined)
    return { action: `Read ${file}`, details: [{ label: "file", text: file }] };
  if ((tool === "Glob" || tool === "Grep") && str(input, "pattern") !== undefined) {
    return {
      action: `${tool === "Glob" ? "Find files" : "Search"} ${oneLine(str(input, "pattern") ?? "")}`,
      details: pick("pattern", "path"),
    };
  }
  if (tool === "WebFetch" && str(input, "url") !== undefined) {
    return { action: `Fetch ${oneLine(str(input, "url") ?? "")}`, details: pick("url", "prompt") };
  }
  return { action: `Use ${tool}`, details: [raw(input)] };
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
