// Turns a recording (recorder.ts) into a contract test fixture for
// crates/agent-runtime: personal data scrubbed, and the steps the core takes
// (answers, replies, interrupts, stop) added between the recorded lines.
// Step format: crates/agent-runtime/fixtures/README.md.
import { realpathSync } from "node:fs";
import { homedir, tmpdir } from "node:os";

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
type Obj = { [key: string]: Json };
type Step = Obj;

/** Default answer text of a denial without a message (driver.rs). */
const DEFAULT_DENIAL = "The person denied this tool call.";

/** Tools every Claude Code has; the rest of `system/init.tools` is the machine's own. */
const STANDARD_TOOLS = new Set([
  "Task",
  "Agent",
  "AskUserQuestion",
  "Bash",
  "Edit",
  "ExitPlanMode",
  "Glob",
  "Grep",
  "NotebookEdit",
  "Read",
  "TodoWrite",
  "WebFetch",
  "WebSearch",
  "Write",
]);

/** Keys that only describe this machine, account or plan. */
const DROPPED = new Set([
  "transcript_path",
  "memory_paths",
  "messaging_socket_path",
  "pid",
  "account",
  "user_output_styles_dir",
  "original_cwd",
  "slash_commands",
  "terminal_slash_commands",
  "skills",
  "plugins",
  "capabilities",
  "additional_directories",
  "available_output_styles",
  "overageStatus",
  "overageDisabledReason",
  "isUsingOverage",
]);

const isObj = (v: Json | undefined): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Replaces paths, ids, times and plan details with stable placeholders. */
class Scrubber {
  private readonly ids = new Map<string, string>();
  private readonly paths: [RegExp, string][];

  constructor(base: string) {
    const bases = new Set([base]);
    try {
      bases.add(realpathSync(base));
    } catch {}
    // The project is `<base>/w`, so it becomes `/w`; its siblings `/<name>`.
    this.paths = [...bases].sort((a, b) => b.length - a.length).map((b) => [new RegExp(escapeRegExp(b), "g"), ""]);
    // The CLI's own scratch folder, `<tmp>/claude-<uid>/<cwd with dashes>`.
    for (const t of new Set(["/tmp", tmpdir()])) {
      this.paths.unshift([new RegExp(`${escapeRegExp(t)}/claude-\\d+/[^/"\\s\\\\]+`, "g"), "/claude-tmp"]);
    }
    this.paths.push([new RegExp(escapeRegExp(homedir()), "g"), "/home/user"]);
  }

  private id(kind: string, value: string): string {
    const key = `${kind}:${value}`;
    let id = this.ids.get(key);
    if (!id) {
      const n = [...this.ids.keys()].filter((k) => k.startsWith(`${kind}:`)).length + 1;
      id =
        kind === "uuid"
          ? `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`
          : `${kind}_${String(n).padStart(2, "0")}`;
      this.ids.set(key, id);
    }
    return id;
  }

  text(value: string): string {
    let out = value;
    for (const [pattern, replacement] of this.paths) out = out.replace(pattern, replacement);
    out = out.replace(/\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/g, (m) => this.id("uuid", m));
    out = out.replace(/\b(msg|toolu|srvtoolu|req)_[A-Za-z0-9]{10,}\b/g, (m, kind: string) => this.id(kind, m));
    out = out.replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g, "user@example.com");
    return out;
  }

  value(value: Json, key = ""): Json | undefined {
    if (DROPPED.has(key)) return undefined;
    if (typeof value === "string") {
      if (key === "timestamp") return "2026-11-24T12:00:00.000Z";
      // Encrypted thinking: opaque, and only the API can read it.
      if (key === "signature") return "signature";
      return this.text(value);
    }
    if (Array.isArray(value)) {
      const items = key === "tools" ? value.filter((t) => typeof t === "string" && STANDARD_TOOLS.has(t)) : value;
      return items.map((v) => this.value(v)).filter((v): v is Json => v !== undefined);
    }
    if (isObj(value)) {
      const out: Obj = {};
      for (const [k, v] of Object.entries(value)) {
        const scrubbed = this.value(v, k);
        if (scrubbed !== undefined) out[k] = scrubbed;
      }
      return out;
    }
    if (typeof value === "number") {
      if (key === "resetsAt") return 1795000000;
      if (key === "utilization") return 0.5;
      if (key === "pid") return 1;
    }
    return value;
  }
}

/** The CLI's answer to `initialize` lists the machine's commands, agents and models. */
function slimInitialize(line: Obj): Obj {
  const response = (line.response as Obj | undefined)?.response;
  if (isObj(response)) {
    for (const key of ["commands", "agents", "models"]) if (key in response) response[key] = [];
  }
  return line;
}

const controlSubtype = (line: Obj): string | undefined => {
  const request = line.request;
  return isObj(request) && typeof request.subtype === "string" ? request.subtype : undefined;
};

/** Converts one recording to fixture lines. `base` is the folder holding the project folder `w`. */
export function toFixture(recording: string, base: string): string {
  const scrub = new Scrubber(base);
  const raw = recording
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => JSON.parse(l) as Obj)
    .filter((step) => !("meta" in step));
  const steps: Step[] = [];
  const asked = new Map<string, { tool: string; n: number }>();
  let firstUser = true;
  let idle = false;
  const lastOvra = raw.map((s) => "ovra" in s).lastIndexOf(true);
  raw.forEach((step, index) => {
    if (isObj(step.cli)) {
      const line = scrub.value(step.cli) as Obj;
      if (line.type === "control_request" && controlSubtype(line) === "can_use_tool") {
        const request = line.request as Obj;
        asked.set(String(line.request_id), { tool: String(request.tool_name), n: asked.size + 1 });
      }
      if (line.type === "control_response" && (line.response as Obj)?.request_id === "init") slimInitialize(line);
      if (line.type === "result") idle = true;
      steps.push({ cli: line });
      return;
    }
    if (!isObj(step.ovra)) throw new Error(`unexpected recording step ${JSON.stringify(step)}`);
    const line = scrub.value(step.ovra) as Obj;
    const answer = isObj(line.response) ? (line.response as Obj).response : undefined;
    if (line.type === "control_response" && isObj(answer) && typeof answer.behavior === "string") {
      const request = asked.get(String((line.response as Obj).request_id));
      if (!request) throw new Error(`an answer to a request the CLI did not make: ${JSON.stringify(line)}`);
      const params: Obj = { id: `perm${request.n}`, decision: answer.behavior };
      if (answer.behavior === "deny" && answer.message !== DEFAULT_DENIAL) params.message = answer.message ?? null;
      steps.push({ wait: { type: "permissions.requested", data: { tool: request.tool } } });
      steps.push({ core: { method: "permissions.answer", params } });
    } else if (line.type === "user") {
      if (!firstUser) {
        steps.push({ wait: { type: "agents.status", data: { status: { state: "idle" } } } });
        const content = (line.message as Obj).content;
        steps.push({ core: { method: "agents.send", params: { session: "s1", text: content } } });
      }
      firstUser = false;
      idle = false;
    } else if (line.type === "control_request" && controlSubtype(line) === "interrupt") {
      // Stop interrupts, then closes stdin: the last line Ovra writes.
      const method = index === lastOvra ? "agents.stop" : "agents.interrupt";
      if (idle) steps.push({ wait: { type: "agents.status", data: { status: { state: "idle" } } } });
      steps.push({ core: { method, params: { session: "s1" } } });
    }
    steps.push({ ovra: line });
  });
  const text = `${steps.map((s) => JSON.stringify(s)).join("\n")}\n`;
  for (const leak of [/\/(root|home\/(?!user\b)[^/"]+|Users|tmp|private|var\/folders)\//, /sk-ant-/]) {
    const found = text.match(leak);
    if (found) throw new Error(`the fixture still contains ${found[0]}`);
  }
  return text;
}
