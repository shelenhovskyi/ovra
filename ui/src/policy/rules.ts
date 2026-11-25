// How a policy rule reads to people, and the caveats the spec asks the UI to
// show beside it (docs/specs/security.md, "Policies: auto, ask, deny").

import type { PolicyAction, PolicyRule } from "../lib/protocol";

/** Tools whose `match` is a glob on the path relative to the session's folder. */
const FILE_TOOLS = ["Read", "Edit", "MultiEdit", "Write", "NotebookEdit"];

/** Tool names offered while typing; any other name is allowed too. */
export const KNOWN_TOOLS = ["Bash", ...FILE_TOOLS, "Grep", "Glob", "WebFetch", "WebSearch"];

/** The order in which the core looks at rules: the first action that matches decides. */
const PRECEDENCE: PolicyAction[] = ["deny", "ask", "auto"];

export const ACTIONS: Record<PolicyAction, string> = { deny: "Deny", ask: "Ask", auto: "Auto" };

/** A rule as people read it: `Bash(npm test)`, or `WebFetch` for every call. */
export function ruleText(rule: PolicyRule): string {
  return rule.match === undefined ? rule.tool : `${rule.tool}(${rule.match})`;
}

/** A rule's name in labels: its text, "exactly" for a whole-command rule, and its action. */
export function ruleLabel(rule: PolicyRule): string {
  return `${ruleText(rule)}${rule.exact ? " exactly" : ""} ${rule.action}`;
}

/** The tool a form names: a known tool typed in another case is that tool. */
function toolName(tool: string): string {
  const name = tool.trim();
  return KNOWN_TOOLS.find((t) => t.toLowerCase() === name.toLowerCase()) ?? name;
}

/** Whether rules for `tool` may have a match; the core refuses one for any other tool. */
export function takesMatch(tool: string): boolean {
  const name = toolName(tool);
  return name === "Bash" || name === "WebFetch" || FILE_TOOLS.includes(name);
}

/** Whether a rule for `tool` with a match may match the whole command only (Bash). */
export function takesExact(tool: string): boolean {
  return toolName(tool) === "Bash";
}

/** What `match` means for `tool`, for the field's description. */
export function matchHint(tool: string): string {
  const name = toolName(tool);
  if (name === "Bash")
    return "A command prefix, matched on whole words: npm test also matches npm test -- -u. With Exact command, only npm test itself.";
  if (FILE_TOOLS.includes(name)) return "A glob on the path inside the folder the agent works in, such as docs/**.";
  if (name === "WebFetch") return "One host, such as docs.rs: subdomains do not match.";
  if (name !== "") return `${name} takes no match: the rule covers every call.`;
  return `Only Bash, WebFetch and the file tools (${FILE_TOOLS.join(", ")}) take a match.`;
}

/** Ovra's own tools, which policies cannot change (the core refuses them). */
const IDE_TOOL_PREFIX = "mcp__ovra__";

export type FormProblem = { field: "tool" | "match"; text: string };

/**
 * Why the core would refuse the rule a form describes, in plain words, and
 * the field to fix, or null. Mirrors the core's check
 * (`crates/core/src/policy.rs`), so the form never sends a rule the core is
 * known to refuse.
 */
export function formProblem(rule: PolicyRule): FormProblem | null {
  const tool = (text: string): FormProblem => ({ field: "tool", text });
  const match = (text: string): FormProblem => ({ field: "match", text });
  if (rule.tool === "") return tool("Choose a tool first.");
  if (rule.tool.startsWith(IDE_TOOL_PREFIX))
    return tool("Ovra's own tools follow their role's rules; a policy cannot change them.");
  if (rule.match === undefined || rule.tool === "Bash") return null;
  // The core takes a host as the netproxy writes it; this catches the usual slips only.
  if (rule.tool === "WebFetch")
    return /^[a-z0-9-]+(\.[a-z0-9-]+)*$/.test(rule.match)
      ? null
      : match("Write one host in lower case, such as docs.rs: no scheme, path or wildcard.");
  if (!takesMatch(rule.tool))
    return match(
      `${rule.tool} takes no match: a rule for it covers every call. Clear Match, or choose Bash, WebFetch or a file tool.`,
    );
  const parts = rule.match.split(/[/\\]/);
  if (/^[/\\]/.test(rule.match) || rule.match.includes(":") || parts.some((p) => p === "" || p === "." || p === ".."))
    return match("Write the path inside the agent's folder, such as docs/**: no leading /, no . or .. parts, no :.");
  return null;
}

/** What the form says about the tool typed: a name fixed for her, or one that may never match. */
export function toolNote(tool: string): string | null {
  const name = toolName(tool);
  if (name !== tool.trim()) return `Saved as ${name}: tool names are case-sensitive.`;
  return toolWarning(name);
}

/** Why a tool name may never match, or null: names are matched exactly. */
export function toolWarning(tool: string): string | null {
  const name = tool.trim();
  if (name === "" || KNOWN_TOOLS.includes(name) || name.startsWith("mcp__")) return null;
  const known = KNOWN_TOOLS.find((t) => t.toLowerCase() === name.toLowerCase());
  if (known) return `Tool names match exactly: ${name} never matches ${known}. Write ${known}.`;
  return `Ovra does not know a tool named ${name}, so this rule may never match. Check the spelling.`;
}

/** What holds where Bash and file deny and ask rules do not. */
const BOUNDARY = "The boundary that holds is the sandbox and its network proxy; without a sandbox, nothing does.";

/** What a rule does not promise, in plain words. */
export function caveats(rule: PolicyRule): string[] {
  const notes: string[] = [];
  const warning = toolWarning(rule.tool);
  if (warning) notes.push(warning);
  if (rule.action === "deny") {
    if (rule.tool === "Bash") {
      // A deny with no match refuses every command, however it is written.
      if (rule.match !== undefined) {
        notes.push(
          `Advisory: the same command written another way, such as env ${rule.match}, gets past it. ${BOUNDARY}`,
        );
      }
    } else if (FILE_TOOLS.includes(rule.tool)) {
      notes.push(`Advisory: Grep, Glob or cat in Bash still reach these files. ${BOUNDARY}`);
    } else {
      notes.push("Only this tool is refused; others such as Bash can do the same.");
    }
  }
  if (rule.action === "ask") {
    // An ask with no match asks about every command, however it is written.
    if (rule.tool === "Bash" && rule.match !== undefined) {
      notes.push(
        `Advisory: the same command written another way, such as env ${rule.match}, is not asked about. ${BOUNDARY}`,
      );
    } else if (FILE_TOOLS.includes(rule.tool)) {
      notes.push(`Advisory: Bash commands like cat still reach it. ${BOUNDARY}`);
    }
  }
  if (rule.action === "auto") {
    notes.push(
      "Applies only to an agent in the sandbox with limited network access. Otherwise the call is decided as if this rule were not there.",
    );
    if (rule.tool === "Bash") {
      const what = rule.match === undefined ? "Every command" : rule.match;
      notes.push(`${what} also runs whatever it calls, such as package.json scripts, and the agent can edit those.`);
    }
  }
  return notes;
}

/** The rules in the order the core looks at them, each with its place in `rules`. */
export function byPrecedence(rules: PolicyRule[]): { rule: PolicyRule; index: number }[] {
  return rules
    .map((rule, index) => ({ rule, index }))
    .sort((a, b) => PRECEDENCE.indexOf(a.rule.action) - PRECEDENCE.indexOf(b.rule.action) || a.index - b.index);
}

/**
 * The rule a form describes: names trimmed, a known tool in its own case, an empty match left out.
 * `exact` counts only for a Bash match.
 */
export function formRule(tool: string, match: string, action: PolicyAction, exact = false): PolicyRule {
  const pattern = match.trim();
  const name = toolName(tool);
  if (pattern === "") return { tool: name, action };
  return exact && takesExact(name)
    ? { tool: name, match: pattern, action, exact }
    : { tool: name, match: pattern, action };
}
