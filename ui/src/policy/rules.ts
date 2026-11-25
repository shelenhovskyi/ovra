// How a policy rule reads to people, and the caveats the spec asks the UI to
// show beside it (docs/specs/security.md, "Policies: auto, ask, deny").

import type { PolicyAction, PolicyRule } from "../lib/protocol";

/** Tools whose `match` is a glob on the path relative to the session's folder. */
const FILE_TOOLS = ["Read", "Edit", "MultiEdit", "Write", "NotebookEdit"];

/** The order in which the core looks at rules: the first action that matches decides. */
const PRECEDENCE: PolicyAction[] = ["deny", "ask", "auto"];

export const ACTIONS: Record<PolicyAction, string> = { deny: "Deny", ask: "Ask", auto: "Auto" };

/** A rule as people read it: `Bash(npm test)`, or `WebFetch` for every call. */
export function ruleText(rule: PolicyRule): string {
  return rule.match === undefined ? rule.tool : `${rule.tool}(${rule.match})`;
}

/** What `match` means for `tool`, for the field's description. */
export function matchHint(tool: string): string {
  if (tool.trim() === "Bash") return "A command prefix, matched on whole words: npm test also matches npm test -- -u.";
  if (FILE_TOOLS.includes(tool.trim())) return "A glob on the path inside the session's folder, such as docs/**.";
  return "Only Bash and the file tools take a match. Leave it empty to match every call.";
}

/** What a rule does not promise, in plain words. */
export function caveats(rule: PolicyRule): string[] {
  const notes: string[] = [];
  if (rule.action === "deny" && rule.tool === "Bash") {
    notes.push(
      "Advisory: the same command written another way, such as env git push, gets past it. The sandbox is the boundary that holds.",
    );
  }
  if (rule.action === "deny" && FILE_TOOLS.includes(rule.tool)) {
    notes.push(
      "Advisory: Grep, Glob or cat in Bash still reach these files. The sandbox's protected and hidden paths are the boundary that holds.",
    );
  }
  if (rule.action === "auto") {
    notes.push("Applies only in a sandboxed session whose network is limited. Otherwise Ovra still asks.");
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

/** The rule a form describes: names trimmed, an empty match left out. */
export function formRule(tool: string, match: string, action: PolicyAction): PolicyRule {
  const pattern = match.trim();
  return pattern === "" ? { tool: tool.trim(), action } : { tool: tool.trim(), match: pattern, action };
}
