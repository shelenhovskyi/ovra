// A rename that changes files the editor does not show (docs/specs/lsp.md).
// Nothing is written to disk: each such file gets the edits in its buffer as
// one change the person can undo, and opens as an unsaved tab to review and
// save. Either every file gets its edits or none does, so the code is never
// left half renamed.

import type { ChangeSpec, Text } from "@codemirror/state";
import type { Buffer, Buffers } from "../buffers.svelte";
import { pathOf } from "./uris";
import type { BufferWorkspace } from "./workspace";

/** One LSP `TextEdit`. */
type TextEdit = { range: { start: Position; end: Position }; newText: string };
type Position = { line: number; character: number };

/** More files than this get no rename: too many tabs to review. */
const MAX_FILES = 50;

type Options = {
  workspace: BufferWorkspace;
  buffers: Buffers;
  /** Adds a tab for `path` without showing it. */
  keep: (path: string) => void;
  note: (text: string) => void;
  /** What the edits are, for the notes: a rename (the default) or a quick fix. */
  kind?: "rename" | "fix";
};

/** Applies `changes` (by URI) in open files and in the buffers of the others, or in none. */
export async function renameAcross(changes: Record<string, unknown[]>, options: Options): Promise<void> {
  const { workspace, buffers, note } = options;
  const fix = options.kind === "fix";
  const failed = fix ? "Quick fix not applied" : "Rename not applied";
  const again = fix ? "try the fix again" : "rename again";
  const files = Object.entries(changes).filter(([, edits]) => edits.length > 0) as [string, TextEdit[]][];
  if (files.length > MAX_FILES) {
    note(`${failed}: it would change ${files.length} files, more than ${MAX_FILES}.`);
    return;
  }
  const opened: string[] = [];
  const refuse = (why: string) => {
    for (const path of opened) buffers.discard(path);
    note(`${failed}: ${why}`);
  };
  type Target = { apply: (changes: ChangeSpec[]) => void; doc: Text; path: string | null; edits: TextEdit[] };
  const targets: Target[] = [];
  for (const [uri, edits] of files) {
    const file = workspace.open().find((f) => f.uri === uri);
    if (file) {
      const view = file.getView();
      // Typed into since the server answered: its positions are stale.
      if (!view.state.doc.eq(file.doc)) return refuse("the file changed while the server worked. Try again.");
      targets.push({
        doc: file.doc,
        path: null,
        edits,
        apply: (changes) => view.dispatch({ changes, userEvent: fix ? "input.fix" : "rename" }),
      });
      continue;
    }
    const path = pathOf(uri);
    if (path === null) return refuse("it would change a file outside the project.");
    const known = buffers.get(path);
    if (known?.dirty) return refuse(`${path} has unsaved changes. Save or revert it, then ${again}.`);
    const buffer: Buffer = known ?? buffers.open(path);
    if (!known) opened.push(path);
    await buffer.loaded;
    // A draft restored on load is unsaved text the server did not see.
    if (buffer.status !== "ready" || buffer.dirty) return refuse(`${path} cannot be changed here.`);
    const doc = buffer.doc;
    if (!doc) return refuse(`${path} cannot be changed here.`);
    targets.push({
      doc,
      path,
      edits,
      apply: (changes) => {
        buffer.edit({ changes, userEvent: fix ? "input.fix" : "rename" });
      },
    });
  }
  const planned: [Target, ChangeSpec[]][] = [];
  for (const target of targets) {
    const changes = toChanges(target.doc, target.edits);
    if (!changes) return refuse(`the server's edits do not fit ${target.path ?? "the open file"}.`);
    planned.push([target, changes]);
  }
  for (const [target, changes] of planned) {
    target.apply(changes);
    if (target.path !== null) options.keep(target.path);
  }
  const others = planned.flatMap(([t]) => (t.path === null ? [] : [t.path]));
  const shown = planned.flatMap(([t]) => (t.path === null ? ["this file"] : []));
  if (others.length > 0) {
    note(
      `${fix ? "Changed" : "Renamed in"} ${planned.length} files, none saved yet: ${[...shown, ...others].join(", ")}. Review each tab, then save, or undo there (Mod+Z).`,
    );
  }
}

/** The edits as CodeMirror changes, or null if one is out of range or they overlap. */
function toChanges(doc: Text, edits: TextEdit[]): ChangeSpec[] | null {
  const offset = (pos: Position): number | null => {
    if (!Number.isInteger(pos.line) || !Number.isInteger(pos.character) || pos.line < 0 || pos.character < 0)
      return null;
    if (pos.line >= doc.lines) return null;
    const line = doc.line(pos.line + 1);
    return pos.character > line.length ? null : line.from + pos.character;
  };
  const changes: { from: number; to: number; insert: string }[] = [];
  for (const edit of edits) {
    const from = offset(edit.range.start);
    const to = offset(edit.range.end);
    if (from === null || to === null || to < from || typeof edit.newText !== "string") return null;
    changes.push({ from, to, insert: edit.newText });
  }
  changes.sort((a, b) => a.from - b.from);
  for (let i = 1; i < changes.length; i++) {
    const previous = changes[i - 1];
    const current = changes[i];
    if (previous && current && current.from < previous.to) return null;
  }
  return changes;
}
