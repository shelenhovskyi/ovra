// What the setup and trust cards say. Kept apart from the components so the
// wording of every state is tested in one place.

import type { AgentSetup, Billing, ConfigItem, ConfigKind, ProjectAgentConfig } from "../../lib/protocol";

/** Where to read about installing; opened through `open.url`. */
export const SETUP_DOCS = "https://code.claude.com/docs/en/setup";

/** Install commands for every OS: the UI does not know which one it runs on. */
export const INSTALL_COMMANDS: { label: string; command: string }[] = [
  { label: "macOS, Linux, WSL", command: "curl -fsSL https://claude.ai/install.sh | bash" },
  { label: "Windows PowerShell", command: "irm https://claude.ai/install.ps1 | iex" },
  { label: "npm", command: "npm install -g @anthropic-ai/claude-code" },
];

const PROVIDERS: Record<string, string> = {
  bedrock: "Amazon Bedrock",
  vertex: "Google Vertex AI",
  foundry: "Microsoft Foundry",
};

/** One sentence on who pays for `billing`. */
export function paidText(billing: Billing): string {
  if (billing.kind === "subscription") return "Usage counts against your Claude plan's limits.";
  if (billing.kind === "api_key") return "Usage is billed to an Anthropic API key, at API prices.";
  const provider = PROVIDERS[billing.provider] ?? billing.provider;
  return provider
    ? `Usage is billed to your ${provider} account.`
    : "Usage is billed to your cloud provider's account.";
}

/**
 * Who pays for agents' usage. With ANTHROPIC_API_KEY set (and no cloud
 * provider) the key decides, so the warning replaces the plan line instead
 * of contradicting it.
 */
export function billingText(setup: AgentSetup): { paid: string | null; warning: string | null } {
  const billing = setup.billing;
  if (keyBills(setup)) {
    const may = billing?.kind === "api_key" ? "is" : "may be";
    return {
      paid: null,
      warning: `Usage ${may} billed to the API key in ANTHROPIC_API_KEY, at API prices, not to your Claude plan. To use your plan, unset it and restart Ovra.`,
    };
  }
  return { paid: billing ? paidText(billing) : null, warning: null };
}

/** Who pays, as the tail of the one-line ready card; `null` when the API key warning says it. */
export function paidShort(setup: AgentSetup): string | null {
  const billing = setup.billing;
  if (!billing || keyBills(setup)) return null;
  if (billing.kind === "subscription") return "uses your Claude plan";
  if (billing.kind === "api_key") return "billed to an API key";
  return `billed to ${PROVIDERS[billing.provider] ?? (billing.provider || "your cloud provider")}`;
}

/** `ANTHROPIC_API_KEY` is set and no cloud provider takes precedence over it. */
function keyBills(setup: AgentSetup): boolean {
  return setup.api_key_in_env && setup.billing?.kind !== "cloud";
}

export type SetupAdvice = {
  /** One line for the card's heading. */
  title: string;
  /** What to do, in plain words; `null` when the billing warning says it all. */
  detail: string | null;
  /** A command to run in a terminal, if one fixes it. */
  command?: string;
  /** Whether to show the install commands. */
  install?: boolean;
  /** The saved path leads nowhere: offer to search for claude instead. */
  findAutomatically?: boolean;
};

/** The usual mix-up: the command line program keeps its own sign-in. */
const OWN_SIGN_IN =
  "Signing in to the Claude desktop app or claude.ai does not sign in the claude command line program that Ovra uses.";

export function advice(setup: AgentSetup): SetupAdvice {
  const version = setup.version ? ` ${setup.version}` : "";
  const state = setup.state;
  switch (state.state) {
    case "ready":
      return {
        title: `Claude${version} is ready`,
        // With the key paying, the sign-in line would only distract from the warning.
        detail: keyBills(setup) ? null : "Sessions use your own Claude sign-in and plan.",
      };
    case "not_found":
      if (setup.path) {
        return {
          title: "Claude was not found",
          detail: `There is no program at ${setup.path}.`,
          findAutomatically: true,
        };
      }
      return {
        title: "Claude is not installed",
        detail: `Ovra uses the claude command line program. Install it, then run claude once in a terminal and sign in there. ${OWN_SIGN_IN}`,
        install: true,
      };
    case "outdated":
      return {
        title: `Claude${version} is too old`,
        detail: `Ovra needs version ${state.minimum} or newer. Update it in a terminal, then check again.`,
        command: "claude update",
      };
    case "too_new":
      return {
        title: `Claude${version} is newer than Ovra supports`,
        detail: `This build of Ovra supports versions below ${state.below}. Update Ovra.`,
      };
    case "signed_out":
      return {
        title: "Sign in to Claude",
        detail: `Run claude once in a terminal and sign in there (or run the command below), then check again. ${OWN_SIGN_IN} Ovra never sees your sign-in.`,
        command: "claude auth login",
      };
    case "broken":
      return { title: "Claude could not be checked", detail: state.message };
  }
}

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

/** Display order: rules that skip approvals, then what runs code, then the rest. */
export function weight(kind: ConfigKind): number {
  if (kind === "broad_allow_rules") return 2;
  return runsCode(kind) ? 1 : 0;
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

/** "9 actions Claude may take without asking", for the summary line. */
export function countText(kind: ConfigKind, n: number): string {
  switch (kind) {
    case "broad_allow_rules":
      return plural(n, "unlimited permission", "unlimited permissions");
    case "allow_rules":
      return `${plural(n, "action", "actions")} Claude may take without asking`;
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

/** One line of counts: "1 unlimited permission · 1 hook that runs code · 2 skills". */
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

export type Warnings = {
  /** Rules that let Claude act without Ovra's approvals, named. */
  broad: string | null;
  /** What runs commands on the user's computer, counted. */
  code: string | null;
};

/**
 * The risk in one or two short lines, shown before the user can trust.
 * `fresh` says "new" for the settings that changed since the last trust.
 */
export function warnings(items: { kind: ConfigKind; entries: string[] }[], fresh = false): Warnings {
  const rules = items.filter((item) => item.kind === "broad_allow_rules").flatMap((item) => item.entries);
  const shown = rules.slice(0, 3);
  const more = count("broad_allow_rules", rules) - count("broad_allow_rules", shown);
  const named = list(more > 0 ? [...shown.filter((r) => !CUT.test(r)), `${more} more`] : shown);
  const code = tally(items.filter((item) => runsCode(item.kind)));
  const total = code.reduce((sum, [, n]) => sum + n, 0);
  const parts = code.map(([kind, n]) => {
    const [one, many] = CODE_NOUNS[kind] ?? ["item", "items"];
    return plural(n, fresh ? `new ${one}` : one, fresh ? `new ${many}` : many);
  });
  return {
    broad: rules.length > 0 ? `Claude will use ${named} without asking you.` : null,
    code: total > 0 ? `${list(parts)} ${total === 1 ? "runs" : "run"} commands on your computer.` : null,
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
