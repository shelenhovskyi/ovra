// Plain-language lines for audit entries. Kept apart from the component so
// every kind of entry is tested without mounting it.

import type { AuditEntry, AuditEvent, DecidedBy, NavigationBy, ReviewAction } from "../lib/protocol";

export type Described = {
  /** What happened, e.g. "Allowed Bash". */
  title: string;
  /** Who or what decided, and how the agent ran. */
  detail: string;
  /** The input in one plain line: a command, a path, a summary. */
  input: string | null;
  /** What the log keeps as it is (JSON, hashes), for a "Details" disclosure. */
  raw: string | null;
  /** Refusals and unsandboxed runs stand out. */
  tone: "plain" | "refused" | "warn";
};

/** The filter choices, and which kinds each shows. */
export const FILTERS = {
  all: { label: "Everything", kinds: null },
  tools: { label: "Tool calls", kinds: ["tool_decided"] },
  sessions: { label: "Agent starts", kinds: ["session_started"] },
  settings: { label: "Trust and policy", kinds: ["trust_changed", "policy_changed", "rule_added"] },
  reviews: { label: "Reviews", kinds: ["review_decided"] },
  network: { label: "Network and secrets", kinds: ["network_refused", "secret_used", "browser_navigation"] },
  servers: {
    label: "Servers",
    kinds: [
      "server_started",
      "server_stopped",
      "loopback_granted",
      "loopback_revoked",
      "language_server_started",
      "language_server_stopped",
    ],
  },
} as const satisfies Record<string, { label: string; kinds: readonly AuditEvent["kind"][] | null }>;

export type Filter = keyof typeof FILTERS;

/** Whether the entry passes the filter; `refusedOnly` keeps refused calls and requests. */
export function shows(filter: Filter, refusedOnly: boolean, entry: AuditEntry): boolean {
  const kinds: readonly string[] | null = FILTERS[filter].kinds;
  if (kinds && !kinds.includes(entry.event.kind)) return false;
  return !refusedOnly || describe(entry).tone === "refused";
}

const REVIEW: Record<ReviewAction, string> = {
  approved: "kept",
  partly_approved: "partly kept",
  rejected: "discarded",
  undone_reject: "discard undone",
  taken: "replaced with the worker's version",
  undone_take: "replace undone",
};

const NAVIGATION_BY: Record<NavigationBy, string> = {
  person: "by a person",
  no_answer: "because nobody answered in time",
  session_ended: "because the agent stopped",
};

function decidedBy(by: DecidedBy): string {
  switch (by.kind) {
    case "person":
      return "by a person";
    case "policy":
      return `by the project's policy (${by.rule})`;
    case "role":
      return "by Ovra's rules for its own tools";
    case "session_ended":
      return "because the agent stopped or finished";
  }
}

/** A tool as people read it: `mcp__ovra__request_review` is "Ovra: request review". */
export function toolLabel(tool: string): string {
  const [prefix, server, ...name] = tool.split("__");
  if (prefix !== "mcp" || !server || name.length === 0) return tool;
  return `${server === "ovra" ? "Ovra" : server}: ${name.join(" ").replaceAll("_", " ")}`;
}

/** Input fields that say in one line what a call was about, in the order they are looked for. */
const GIST = ["command", "file_path", "notebook_path", "path", "url", "query", "pattern", "summary", "description"];

/** A tool's JSON input as one plain line, and the JSON itself for the details. */
function toolInput(text: string): { input: string | null; raw: string | null } {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    // Cut or not JSON: the text is all there is.
    return { input: null, raw: text };
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) return { input: null, raw: text };
  const fields = value as Record<string, unknown>;
  if (Object.keys(fields).length === 0) return { input: null, raw: null };
  const gist = GIST.map((key) => fields[key]).find((v): v is string => typeof v === "string" && v.trim() !== "");
  return { input: gist ?? null, raw: JSON.stringify(value, null, 2) };
}

export function describe(entry: AuditEntry): Described {
  const event = entry.event;
  switch (event.kind) {
    case "session_started": {
      const where = {
        direct: "In the project folder",
        research: "Reading the project folder, changing nothing",
        isolated: "In its own copy of the project",
      }[event.mode];
      const settings = event.trusted ? "with the project's agent settings" : "without the project's agent settings";
      const sandbox = event.sandbox ? `sandbox ${event.sandbox.name}, network ${event.sandbox.network}` : "no sandbox";
      return {
        title: "Agent started",
        detail: `${where}, ${settings}, ${sandbox}.`,
        input: null,
        raw: null,
        tone: event.sandbox ? "plain" : "warn",
      };
    }
    case "tool_decided": {
      const verb = event.allowed ? "Allowed" : "Refused";
      const shown =
        event.input.kind === "file"
          ? {
              input: event.input.path,
              raw: event.input.sha256 ? `SHA-256 of what it writes: ${event.input.sha256}` : null,
            }
          : toolInput(event.input.text);
      return {
        title: `${verb} ${toolLabel(event.tool)}`,
        detail: `${verb} ${decidedBy(event.by)}.`,
        ...shown,
        tone: event.allowed ? "plain" : "refused",
      };
    }
    case "trust_changed":
      return {
        title: event.trusted ? "Project settings trusted" : "Trust in project settings removed",
        detail: event.trusted ? "Agents load the settings as they were reviewed." : "Agents no longer load them.",
        input: null,
        raw: event.fingerprint ? `Fingerprint of the reviewed settings: ${event.fingerprint}` : null,
        tone: "plain",
      };
    case "policy_changed": {
      const rules = event.policy.rules.map((r) => `${r.action} ${r.tool}${r.match ? ` ${r.match}` : ""}`);
      return {
        title: "Policy changed",
        detail:
          rules.length === 0
            ? "No rules: every tool call asks."
            : `${rules.length} ${rules.length === 1 ? "rule" : "rules"}.`,
        input: rules.length === 0 ? null : rules.join("\n"),
        raw: null,
        tone: "plain",
      };
    }
    case "rule_added": {
      const { tool, match, action } = event.rule;
      return {
        title: `Rule added: ${tool}${match ? `(${match})` : ""}`,
        detail: `${action === "auto" ? "Always allowed" : action} from an approval, ${decidedBy(event.by)}.`,
        input: `${action} ${tool}${match ? ` ${match}` : ""}`,
        raw: null,
        tone: "plain",
      };
    }
    case "review_decided":
      return {
        title: `Review ${REVIEW[event.action]}`,
        detail: `Review ${event.review}, decided by a person.`,
        input: null,
        raw: null,
        tone: "plain",
      };
    case "network_refused":
      return { title: `Network refused: ${event.host}`, detail: event.reason, input: null, raw: null, tone: "refused" };
    case "secret_used":
      return {
        title: `Secret ${event.name} used`,
        detail: `Sent to ${event.host}. The log never keeps its value.`,
        input: null,
        raw: null,
        tone: "plain",
      };
    case "server_started":
      return {
        title: `Server ${event.name} started`,
        detail: `Run ${event.generation}, started ${event.by.kind === "person" ? "by a person" : `by agent ${event.by.session}`}.`,
        input: null,
        raw: null,
        tone: "plain",
      };
    case "server_stopped":
      return {
        title: `Server ${event.name} stopped`,
        detail: `Run ${event.generation}, ${event.code === null ? "ended by a signal" : `exit code ${event.code}`}.`,
        input: null,
        raw: null,
        tone: "plain",
      };
    case "loopback_granted":
      return {
        title: `Server reachable on localhost:${event.port}`,
        detail: `The agent may reach server ${event.server} (run ${event.generation}) while it runs.`,
        input: null,
        raw: null,
        tone: "plain",
      };
    case "loopback_revoked":
      return {
        title: `Server no longer reachable on localhost:${event.port}`,
        detail: `Server ${event.server} (run ${event.generation}) stopped, or the agent did.`,
        input: null,
        raw: null,
        tone: "plain",
      };
    case "language_server_started":
      return {
        title: `Language server ${event.name} started`,
        detail:
          event.sandbox === null
            ? `For ${event.language} files, without a sandbox: it could read and change your files.`
            : `For ${event.language} files, in the sandbox (${event.sandbox}).`,
        input: null,
        raw: null,
        tone: event.sandbox === null ? "warn" : "plain",
      };
    case "language_server_stopped":
      return {
        title: `Language server ${event.name} ${event.reason === null ? "stopped" : "failed"}`,
        detail: event.reason ?? `It served ${event.language} files.`,
        input: null,
        raw: null,
        tone: "plain",
      };
    case "browser_navigation": {
      const verb = event.allowed ? "allowed" : "refused";
      return {
        title: `Browser ${event.allowed ? "may open" : "may not open"} ${event.origin}`,
        detail: `The agent's browser was ${verb} ${NAVIGATION_BY[event.by]}.`,
        input: null,
        raw: null,
        tone: event.allowed ? "plain" : "refused",
      };
    }
  }
}

const pad = (n: number) => String(n).padStart(2, "0");

/** Local time as `YYYY-MM-DD HH:MM:SS`. */
export function when(atMs: number): string {
  const d = new Date(atMs);
  const day = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  return `${day} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}
