// What the trust cards say about a project's own agent settings. Kept apart
// from the components so the wording of every state is tested in one place.

import type { ConfigItem, ConfigKind, ProjectAgentConfig } from "../../lib/protocol";

/** Whether a change to these project paths may change what trusting the project loads. */
export function touchesConfig(paths: string[]): boolean {
  return paths.some((path) => path === ".claude" || path.startsWith(".claude/"));
}

/** Headings of the configuration a trusted project loads, in the details. */
export const KIND_LABELS: Record<ConfigKind, string> = {
  hooks: "Hooks: commands that run on agent events",
  helpers: "Helper commands",
  plugins: "Plugins, which can bring their own hooks and servers",
  env: "Environment variables",
  broad_allow_rules: "Whole tools or any command allowed without asking you",
  allow_rules: "Actions allowed without asking you",
  settings: "Other settings",
  agents: "Custom subagents",
  commands: "Slash commands",
  skills: "Skills",
  unreadable: "Could not be read, may still load",
};

/** Whether loading this kind runs commands on the user's computer. */
export function runsCode(kind: ConfigKind): boolean {
  return kind === "hooks" || kind === "helpers" || kind === "plugins";
}

/** Display order: rules that skip approvals, then what runs code or could not be checked, then the rest. */
export function weight(kind: ConfigKind): number {
  if (kind === "broad_allow_rules") return 2;
  return runsCode(kind) || kind === "unreadable" ? 1 : 0;
}

export type ConfigChange = { path: string; kind: ConfigKind; added: string[]; removed: string[] };

/** What changed between the trusted configuration and the current one, entry by entry. */
export function changes(before: ProjectAgentConfig, after: ProjectAgentConfig): ConfigChange[] {
  const key = (item: { path: string; kind: ConfigKind }) => `${item.path}\u0000${item.kind}`;
  const old = new Map(before.items.map((item) => [key(item), item]));
  const now = new Map(after.items.map((item) => [key(item), item]));
  const result: ConfigChange[] = [];
  for (const item of [...after.items, ...before.items.filter((item) => !now.has(key(item)))]) {
    const was = new Set(old.get(key(item))?.entries ?? []);
    const is = new Set(now.get(key(item))?.entries ?? []);
    const added = [...is].filter((entry) => !was.has(entry));
    const removed = [...was].filter((entry) => !is.has(entry));
    if (added.length > 0 || removed.length > 0) result.push({ path: item.path, kind: item.kind, added, removed });
  }
  return result.sort((a, b) => weight(b.kind) - weight(a.kind));
}

// The core cuts long lists and ends them with this line.
const CUT = /^… and (\d+) more$/;

/** How many entries `entries` stands for, counting the core's "… and N more" line. */
function count(kind: ConfigKind, entries: string[]): number {
  // An unreadable file's entry is the reason, not something that loads.
  if (kind === "unreadable") return entries.length > 0 ? 1 : 0;
  return entries.reduce((sum, entry) => sum + (Number(CUT.exec(entry)?.[1]) || 1), 0);
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** "9 narrow permissions", for the summary line. */
export function countText(kind: ConfigKind, n: number): string {
  switch (kind) {
    case "broad_allow_rules":
      return plural(n, "broad permission", "broad permissions");
    case "allow_rules":
      return plural(n, "narrow permission", "narrow permissions");
    case "hooks":
      return `${plural(n, "hook", "hooks")} that ${n === 1 ? "runs" : "run"} code`;
    case "helpers":
      return `${plural(n, "helper command", "helper commands")} that ${n === 1 ? "runs" : "run"} code`;
    case "plugins":
      return `${plural(n, "plugin", "plugins")} that can run code`;
    case "env":
      return plural(n, "environment variable", "environment variables");
    case "settings":
      return plural(n, "other setting", "other settings");
    case "agents":
      return plural(n, "custom subagent", "custom subagents");
    case "commands":
      return plural(n, "slash command", "slash commands");
    case "skills":
      return plural(n, "skill", "skills");
    case "unreadable":
      return plural(n, "file Ovra could not read", "files Ovra could not read");
  }
}

/** Summary order: what skips approvals or runs code, then other permissions, then the rest. */
const ORDER: ConfigKind[] = [
  "broad_allow_rules",
  "hooks",
  "helpers",
  "plugins",
  "allow_rules",
  "env",
  "agents",
  "commands",
  "skills",
  "settings",
  "unreadable",
];

/** Entries counted by kind, in summary order. */
function tally(items: { kind: ConfigKind; entries: string[] }[]): [ConfigKind, number][] {
  const counts = new Map<ConfigKind, number>();
  for (const item of items) counts.set(item.kind, (counts.get(item.kind) ?? 0) + count(item.kind, item.entries));
  return ORDER.flatMap((kind) => {
    const n = counts.get(kind) ?? 0;
    return n > 0 ? [[kind, n] as [ConfigKind, number]] : [];
  });
}

/** One line of counts: "1 broad permission · 1 hook that runs code · 2 skills". */
export function summary(items: ConfigItem[]): string {
  return tally(items)
    .map(([kind, n]) => countText(kind, n))
    .join(" · ");
}

/** "3 added, 1 removed" for a change; empty when only file contents changed. */
export function changeSummary(diff: ConfigChange[]): string {
  const added = diff.reduce((sum, c) => sum + c.added.length, 0);
  const removed = diff.reduce((sum, c) => sum + c.removed.length, 0);
  return [added > 0 ? `${added} added` : "", removed > 0 ? `${removed} removed` : ""].filter(Boolean).join(", ");
}

const CODE_NOUNS: Partial<Record<ConfigKind, [string, string]>> = {
  hooks: ["hook", "hooks"],
  helpers: ["helper command", "helper commands"],
  plugins: ["plugin", "plugins"],
};

function list(parts: string[]): string {
  return parts.length < 2 ? parts.join("") : `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
}

/**
 * Who the trust card speaks to: a project not trusted yet, a trusted one, or
 * a trusted one whose settings changed (the warnings are about what was added).
 */
export type TrustState = "untrusted" | "trusted" | "changed";

/** The trust buttons ignore presses this long after the card or its details change, as approvals do. */
export const TRUST_ARM_MS = 500;

export type Warnings = {
  /** Rules that let Claude act without Ovra's approvals, named. */
  broad: string | null;
  /** What runs commands on the user's computer, counted, and quoted when short. */
  code: string | null;
  /** Environment variables that can redirect Claude or run code, named. */
  env: string | null;
  /** Files Ovra could not read, named: whatever they hold may still load. */
  unreadable: string | null;
};

/**
 * Environment variables that send Claude's traffic elsewhere, change who pays,
 * or make programs run extra code. Matched in upper case: Windows ignores case.
 */
const RISKY_ENV = [
  /^ANTHROPIC_/,
  /^CLAUDE_/,
  /^DYLD_/,
  /_BASE_URL$/,
  /_PROXY$/,
  /^(NODE_OPTIONS|BASH_ENV|ENV|LD_PRELOAD|LD_LIBRARY_PATH|PATH|PYTHONPATH|PYTHONSTARTUP)$/,
  /^GIT_(SSH|SSH_COMMAND|EXEC_PATH|ASKPASS)$/,
];

/** Whether setting this environment variable can redirect Claude or run code. */
export function riskyEnv(name: string): boolean {
  const upper = name.trim().toUpperCase();
  return RISKY_ENV.some((pattern) => pattern.test(upper));
}

/** `text` cut to `max` characters, ending in "…" when cut. */
function clip(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

/** At most `max` names, then "N more" (counting `extra` names not listed). */
function named(names: string[], extra: number, max = 3): string {
  const shown = names.slice(0, max);
  const more = names.length - shown.length + extra;
  return list(more > 0 ? [...shown, `${more} more`] : shown);
}

/** Commands quoted in the code warning: only as many as stay readable. */
const QUOTED = 2;

/**
 * The risk in short lines, shown before the user can trust. Settings not
 * trusted yet are spoken of as what happens once trusted; for changed
 * settings, `items` are what was added and the lines say "new".
 */
export function warnings(items: { path: string; kind: ConfigKind; entries: string[] }[], state: TrustState): Warnings {
  const fresh = state === "changed";
  const once = state === "untrusted" ? "once you trust this project" : fresh ? "once you trust the changes" : "";
  const lead = once ? `${once.charAt(0).toUpperCase()}${once.slice(1)}, ` : "";

  const rules = items.filter((item) => item.kind === "broad_allow_rules").flatMap((item) => item.entries);
  const shownRules = rules.filter((rule) => !CUT.test(rule));
  const hiddenRules = count("broad_allow_rules", rules) - shownRules.length;

  const code = tally(items.filter((item) => runsCode(item.kind)));
  const total = code.reduce((sum, [, n]) => sum + n, 0);
  const parts = code.map(([kind, n]) => {
    const [one, many] = CODE_NOUNS[kind] ?? ["item", "items"];
    return plural(n, fresh ? `new ${one}` : one, fresh ? `new ${many}` : many);
  });
  // Plugins are names, not commands: quote only what hooks and helpers run, and only when that is all of it.
  const commands = items
    .filter((item) => item.kind === "hooks" || item.kind === "helpers")
    .flatMap((item) => item.entries);
  const quoted =
    commands.length === total && total <= QUOTED ? `: ${commands.map((c) => `"${clip(c, 80)}"`).join(" and ")}` : "";
  const run = once ? "will run" : total === 1 ? "runs" : "run";

  const vars = items.filter((item) => item.kind === "env").flatMap((item) => item.entries);
  const risky = vars.filter((name) => !CUT.test(name) && riskyEnv(name));
  const unlisted = vars.reduce((sum, name) => sum + (Number(CUT.exec(name)?.[1]) || 0), 0);
  const env = [
    risky.length > 0
      ? `${fresh ? "New project" : "Project"} settings set ${named(risky, 0, 4)}: ${once ? `${once}, ` : ""}${risky.length === 1 ? "it" : "they"} can send your prompts and code elsewhere or run code.`
      : "",
    unlisted > 0
      ? `${plural(unlisted, "environment variable is", "environment variables are")} not listed, so Ovra could not check ${unlisted === 1 ? "it" : "them"}: read the settings files.`
      : "",
  ].filter(Boolean);

  const files = items
    .filter((item) => item.kind === "unreadable" && item.entries.length > 0)
    .map((item) => `${item.path} (${clip(item.entries[0] ?? "", 60)})`);

  return {
    broad:
      rules.length > 0
        ? `${lead}Claude ${once ? "will use" : "uses"} ${named(shownRules, hiddenRules)} without asking you.`
        : null,
    code: total > 0 ? `${lead}${list(parts)} ${run} commands on your computer${quoted}.` : null,
    env: env.length > 0 ? env.join(" ") : null,
    unreadable:
      files.length > 0
        ? `Ovra could not read ${fresh ? "the new " : ""}${named(files, 0)}; whatever ${files.length === 1 ? "it holds" : "they hold"}, hooks too, may load${once ? ` ${once}` : ""}.`
        : null,
  };
}

export type Row = { text: string; sign?: "added" | "removed" };
export type Group = { kind: ConfigKind; files: { path: string; rows: Row[] }[] };

/** The details, one group per kind, riskiest first, each listing its files. */
export function groups(items: { path: string; kind: ConfigKind; rows: Row[] }[]): Group[] {
  const byKind = new Map<ConfigKind, Group>();
  for (const item of items) {
    const group = byKind.get(item.kind) ?? { kind: item.kind, files: [] };
    byKind.set(item.kind, group);
    group.files.push({ path: item.path, rows: item.rows });
  }
  return [...byKind.values()].sort((a, b) => ORDER.indexOf(a.kind) - ORDER.indexOf(b.kind));
}
