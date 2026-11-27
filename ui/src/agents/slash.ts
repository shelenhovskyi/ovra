// The `/` menu of a message box (docs/specs/task-commands.md): the person's
// templates and the project's commands and skills, as pure functions the
// menu component and its tests share.

import type { CatalogItem } from "../lib/protocol";
import { rank } from "../palette/fuzzy";
import type { Template } from "./prefs.svelte";

/** A project command or skill Claude runs when the message starts with `/name`. */
export type SlashCommand = {
  name: string;
  description: string | null;
  kind: "command" | "skill";
  /** It loads only once the person trusts the project. */
  whenTrusted: boolean;
};

export type SlashRow =
  | { key: string; kind: "template"; label: string; detail: string; template: Template }
  | { key: string; kind: "command"; label: string; detail: string; command: SlashCommand };

const MAX_ROWS = 8;

/**
 * The typed `/query` when the box starts with `/` and the caret is still in
 * that first word; null otherwise, so a `/` in a path or a sentence stays text.
 */
export function slashQuery(text: string, caret: number): string | null {
  const found = /^\/(\S*)$/.exec(text.slice(0, caret));
  return found ? (found[1] ?? "") : null;
}

/** The catalog's commands and skills that Claude sessions in Ovra load, by name. */
export function slashCommands(items: readonly CatalogItem[]): SlashCommand[] {
  const seen = new Set<string>();
  const out: SlashCommand[] = [];
  for (const item of items) {
    if (item.kind !== "command" && item.kind !== "skill") continue;
    if (!item.agents.includes("claude") || item.loads === "not_in_ovra") continue;
    const name = item.name.replace(/^\//, "");
    // A name with a space could not be typed as one command.
    if (!name || /\s/.test(name) || seen.has(name)) continue;
    seen.add(name);
    out.push({ name, description: item.description, kind: item.kind, whenTrusted: item.loads === "when_trusted" });
  }
  return out;
}

function firstLine(text: string): string {
  const line = text.trim().split("\n", 1)[0] ?? "";
  return line.length > 80 ? `${line.slice(0, 79)}…` : line;
}

/** The menu's rows for `query`: templates first, then commands, best matches first in each. */
export function slashRows(
  query: string,
  templates: readonly Template[],
  commands: readonly SlashCommand[],
): SlashRow[] {
  const ownRows: SlashRow[] = rank(templates, (t) => t.name, query, MAX_ROWS).map(({ item }) => ({
    key: `template ${item.id}`,
    kind: "template",
    label: item.name,
    detail: firstLine(item.text),
    template: item,
  }));
  const commandRows: SlashRow[] = rank(commands, (c) => c.name, query, MAX_ROWS).map(({ item }) => ({
    key: `command ${item.name}`,
    kind: "command",
    label: `/${item.name}`,
    detail: [
      item.whenTrusted ? "Only in a trusted project" : null,
      item.description ?? (item.kind === "skill" ? "Skill" : "Command"),
    ]
      .filter(Boolean)
      .join(" · "),
    command: item,
  }));
  return [...ownRows, ...commandRows].slice(0, MAX_ROWS);
}

/**
 * The box's text once `row` is picked: the typed `/query` becomes the
 * template's text or `/name `, and what followed it stays.
 */
export function applyRow(row: SlashRow, value: string, query: string): { value: string; caret: number } {
  const rest = value.slice(1 + query.length).replace(/^[^\S\n]+/, "");
  const head = row.kind === "template" ? (rest ? `${row.template.text} ` : row.template.text) : `/${row.command.name} `;
  return { value: head + rest, caret: head.length };
}
