// The words of "Skills and instructions" (Catalog.svelte): what each kind of
// item is, in a beginner's terms, when Claude in Ovra loads it, and the
// friendly names of the fields and hook events the files use.

import type { AgentKind, CatalogItem, CatalogKind, Loads } from "../lib/protocol";

type Kind = { kind: CatalogKind; title: string; line: string };

/** In the order the list shows them. */
const KINDS: Kind[] = [
  { kind: "rules", title: "Instructions", line: "Files every session reads first: how to work in this project." },
  { kind: "skill", title: "Skills", line: "Know-how the agent picks up on its own when a task needs it." },
  {
    kind: "subagent",
    title: "Subagents",
    line: "Helpers with their own instructions that the agent can hand work to.",
  },
  { kind: "command", title: "Commands", line: "Saved prompts you run by typing their /name." },
  { kind: "hook", title: "Hooks", line: "Commands that run by themselves when something happens." },
  { kind: "mcp_server", title: "Tool servers", line: "Programs that give the agent more tools (MCP servers)." },
];

/** One item's kind, singular, as its detail names it. */
export const KIND_NAMES: Record<CatalogKind, string> = {
  rules: "Instructions file",
  skill: "Skill",
  subagent: "Subagent",
  command: "Command",
  hook: "Hook",
  mcp_server: "Tool server",
};

export const AGENT_NAMES: Record<AgentKind, string> = { claude: "Claude", gemini: "Gemini", codex: "Codex" };

const FIELD_LABELS: Record<string, string> = {
  tools: "Tools",
  "allowed-tools": "Tools it may use without asking",
  "disallowed-tools": "Tools it may not use",
  model: "Model",
  "argument-hint": "Arguments",
  folder: "Folder",
  matcher: "Runs for",
  timeout: "Time limit",
  type: "Type",
  env: "Variables it gets",
  env_vars: "Variables it gets",
  headers: "Headers it sends",
  http_headers: "Headers it sends",
  color: "Colour",
  license: "Licence",
};

export function fieldLabel(name: string): string {
  return FIELD_LABELS[name] ?? name;
}

/** Claude's and Gemini's hook events, in words. */
const EVENTS: Record<string, string> = {
  PreToolUse: "Before a tool runs",
  PostToolUse: "After a tool runs",
  PostToolUseFailure: "After a tool fails",
  PermissionRequest: "When the agent asks for permission",
  UserPromptSubmit: "When you send a message",
  Notification: "When the agent notifies you",
  Stop: "When the agent finishes",
  SubagentStart: "When a subagent starts",
  SubagentStop: "When a subagent finishes",
  SessionStart: "When a session starts",
  SessionEnd: "When a session ends",
  PreCompact: "Before the conversation is shortened",
  BeforeTool: "Before a tool runs",
  AfterTool: "After a tool runs",
  BeforeAgent: "Before the agent works on a message",
  AfterAgent: "When the agent finishes",
  BeforeModel: "Before each model call",
  AfterModel: "After each model call",
  BeforeToolSelection: "Before the agent picks tools",
};

/** What the list and the detail call an item: a hook by its event, in words. */
export function title(item: CatalogItem): string {
  return item.kind === "hook" ? (EVENTS[item.name] ?? item.name) : item.name;
}

/** When Claude in Ovra loads it; `null` when the agent decides. */
export function loadsLine(loads: Loads, trusted: boolean): { text: string; trust: boolean } | null {
  switch (loads) {
    case "always":
      return { text: "Claude reads it in every session.", trust: false };
    case "when_trusted":
      return trusted
        ? { text: "Claude uses it: you trust this project.", trust: false }
        : { text: "Claude uses it only once you trust this project.", trust: true };
    case "not_in_ovra":
      return { text: "Not used in Ovra: agents here get Ovra's own tools only.", trust: false };
    case "agent":
      return null;
  }
}

export type Group = Kind & { items: { key: string; item: CatalogItem }[] };

/**
 * The items by kind, in `KINDS` order, kept when `filter`'s words are all
 * in the name, title, description or path. Each has a key that stays the
 * same when the list is read again.
 */
export function groups(items: CatalogItem[], filter: string): Group[] {
  const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
  const seen = new Map<string, number>();
  const keyed = items.map((item) => {
    const base = `${item.kind}|${item.path}|${item.name}`;
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    return { key: `${base}|${n}`, item };
  });
  const kept = keyed.filter(({ item }) => {
    const text = [item.name, title(item), item.description ?? "", item.path].join(" ").toLowerCase();
    return words.every((word) => text.includes(word));
  });
  return KINDS.map((kind) => ({ ...kind, items: kept.filter(({ item }) => item.kind === kind.kind) })).filter(
    (group) => group.items.length > 0,
  );
}

/** The files the catalog is read from: a change to one reads it again. */
export function touchesCatalog(paths: string[]): boolean {
  const FILES = ["CLAUDE.md", "CLAUDE.local.md", "AGENTS.md", "AGENTS.override.md", "GEMINI.md", ".mcp.json"];
  const FOLDERS = [".claude", ".gemini", ".codex"];
  return paths.some((path) => FILES.includes(path) || FOLDERS.some((f) => path === f || path.startsWith(`${f}/`)));
}

/** Whether the item's file is Markdown the detail can show. */
export function showsText(item: CatalogItem): boolean {
  return item.path.endsWith(".md");
}

/** A Markdown file without its `---` header, which the detail shows as fields. */
export function withoutHeader(text: string): string {
  const match = /^﻿?---\r?\n[\s\S]*?\r?\n---[ \t]*(\r?\n|$)/.exec(text);
  return match ? text.slice(match[0].length) : text;
}
