// What an approval card shows under its title: the request in a person's
// words, never a field the title already names. Edits are a small diff, a
// command's description is plain text, and nothing is cut: a command the
// title shortens is shown whole.

import type { ToolSummary } from "./describe";

export type Part =
  /** Plain text in the body font: what the agent says the step is for. */
  | { kind: "note"; text: string }
  /** Mono text: a whole command, or what a file will contain. */
  | { kind: "code"; text: string }
  /** Lines taken out and put in. */
  | { kind: "diff"; removed: string[]; added: string[] }
  /** Anything else, label and value; labels in sentence case. */
  | { kind: "fields"; fields: { label: string; text: string }[] };

function str(input: unknown, key: string): string | undefined {
  if (typeof input !== "object" || input === null) return undefined;
  const value = (input as Record<string, unknown>)[key];
  return typeof value === "string" ? value : undefined;
}

const lines = (text: string | undefined): string[] => (text ? text.split("\n") : []);

function diff(edit: unknown): Part | null {
  const removed = lines(str(edit, "old_string"));
  const added = lines(str(edit, "new_string") ?? str(edit, "new_source"));
  return removed.length + added.length > 0 ? { kind: "diff", removed, added } : null;
}

const sentence = (label: string): string => label.charAt(0).toUpperCase() + label.slice(1);

/** The card's body for a request whose title is `summary.action`. */
export function cardBody(tool: string, input: unknown, summary: ToolSummary): Part[] {
  const record = typeof input === "object" && input !== null ? (input as Record<string, unknown>) : {};
  if (tool === "Bash" && typeof record.command === "string") {
    const command = record.command;
    const description = str(input, "description");
    return [
      // The title has it on one line; anything it changed or cut is shown whole.
      ...(summary.action === `Run ${command}` ? [] : [{ kind: "code", text: command } as const]),
      ...(description ? [{ kind: "note", text: description } as const] : []),
    ];
  }
  const file = str(input, "file_path") ?? str(input, "notebook_path");
  // A notebook cell put in or taken out is not a replacement: its raw input says which.
  const replaces = tool !== "NotebookEdit" || record.edit_mode === undefined || record.edit_mode === "replace";
  if ((tool === "Edit" || tool === "MultiEdit" || tool === "NotebookEdit") && file !== undefined && replaces) {
    const each: unknown[] = Array.isArray(record.edits) ? record.edits : [input];
    const edits = each.map(diff).filter((e): e is Part => e !== null);
    const all = each.some((e) => typeof e === "object" && e !== null && "replace_all" in e && e.replace_all === true);
    if (edits.length > 0 && edits.length === each.length) {
      return [...edits, ...(all ? [{ kind: "note", text: "Replaces every match in the file." } as const] : [])];
    }
  }
  if (tool === "Write" && typeof record.content === "string") return [{ kind: "code", text: record.content }];
  if (!replaces) return [{ kind: "fields", fields: [{ label: "Input", text: JSON.stringify(input, null, 2) }] }];
  // The field the title ends with (its object, whole) is not said again.
  const fields = summary.details
    .filter((d) => d.label === "Input" || d.text === "" || !summary.action.endsWith(` ${d.text}`))
    .map((d) => ({ label: sentence(d.label), text: d.text }));
  return fields.length > 0 ? [{ kind: "fields", fields }] : [];
}
