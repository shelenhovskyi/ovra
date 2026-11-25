// What Ovra's own tools (`mcp__ovra__*`) answered, in words. Their output is
// JSON or a short line for the agent, with session ids the person never sees;
// the transcript shows this line and folds the raw answer away.

const PREFIX = "mcp__ovra__";

function parsed(output: string): Record<string, unknown> | null {
  try {
    const value: unknown = JSON.parse(output);
    return typeof value === "object" && value !== null && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** A plain line for a successful answer of Ovra's tool `tool`; null for other tools or answers it does not know. */
export function ideResult(tool: string, output: string): string | null {
  if (!tool.startsWith(PREFIX)) return null;
  const json = parsed(output);
  switch (tool.slice(PREFIX.length)) {
    case "spawn_agent": {
      const name = typeof json?.name === "string" ? json.name : null;
      const branch = typeof json?.branch === "string" ? json.branch : null;
      if (!name) return null;
      return `Worker ${name} started on its own copy of the project${branch ? ` (branch ${branch})` : ""}`;
    }
    case "list_agents":
      return Array.isArray(json?.agents) ? `Listed ${count(json.agents.length, "worker", "workers")}` : null;
    case "read_output":
      return Array.isArray(json?.messages) ? `Read ${count(json.messages.length, "message", "messages")}` : null;
    case "request_review":
      return Array.isArray(json?.files)
        ? `Review asked for, ${count(json.files.length, "changed file", "changed files")}`
        : null;
    case "send_message":
      return "Message sent";
    case "stop_agent":
      return "Worker stopped";
    case "open_in_ui":
      return "Shown in Ovra";
    default:
      return null;
  }
}
