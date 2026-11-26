// What the editor offers an agent task (docs/specs/task-context.md): the
// focused editor's selected lines and its file's problems in the @ menu,
// and Ask agent in the editor menu and on a problem's hover card.

import { type Diagnostic, forEachDiagnostic, linter } from "@codemirror/lint";
import type { Extension } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { askAgent, registerContextSource, type Suggestion } from "../agents/context.svelte";
import type { ContextWhat } from "../lib/protocol";
import type { Problem } from "./lsp/problems.svelte";
import { shownView } from "./search";

/** What the core takes of one text item, in UTF-8 bytes. */
const MAX_BYTES = 16 * 1024;
/** Problems the @ menu lists from one file. */
const MAX_PROBLEMS = 20;

/** The start of `text` that fits in `bytes` of UTF-8. */
export function fitBytes(text: string, bytes = MAX_BYTES): string {
  const encoder = new TextEncoder();
  if (encoder.encode(text).length <= bytes) return text;
  let low = 0;
  let high = text.length;
  while (low < high) {
    const mid = Math.ceil((low + high) / 2);
    if (encoder.encode(text.slice(0, mid)).length <= bytes) low = mid;
    else high = mid - 1;
  }
  return text.slice(0, low);
}

/** The main selection of `view` as lines of `path`; null when nothing is selected. */
export function selectionOf(view: EditorView, path: string): ContextWhat | null {
  const { from, to, empty } = view.state.selection.main;
  if (empty) return null;
  const doc = view.state.doc;
  const first = doc.lineAt(from);
  // A selection that ends where a line starts does not take that line.
  const last = to > from && doc.lineAt(to).from === to ? doc.lineAt(to - 1) : doc.lineAt(to);
  return {
    kind: "selection",
    path,
    from_line: first.number,
    to_line: last.number,
    text: fitBytes(view.state.sliceDoc(from, to)),
  };
}

function problem(view: EditorView, path: string, diagnostic: Diagnostic, from: number): ContextWhat {
  return {
    kind: "problem",
    path,
    line: view.state.doc.lineAt(from).number,
    severity: diagnostic.severity,
    message: fitBytes(diagnostic.message, 4096),
    source: diagnostic.source ?? null,
  };
}

/** The file's problems, first ones first. */
function problemsOf(view: EditorView, path: string): ContextWhat[] {
  const found: ContextWhat[] = [];
  forEachDiagnostic(view.state, (diagnostic, from) => {
    if (found.length < MAX_PROBLEMS) found.push(problem(view, path, diagnostic, from));
  });
  return found;
}

/** A problem that covers `pos`, if any. */
export function problemAt(view: EditorView, path: string, pos: number): ContextWhat | null {
  let found: ContextWhat | null = null;
  forEachDiagnostic(view.state, (diagnostic, from, to) => {
    if (!found && from <= pos && pos <= to) found = problem(view, path, diagnostic, from);
  });
  return found;
}

/** What the @ menu offers from `view`: its selection, then its problems. */
export function editorSuggestions(view: EditorView, path: string): Suggestion[] {
  const offered: Suggestion[] = [];
  const selection = selectionOf(view, path);
  if (selection?.kind === "selection") {
    const lines =
      selection.from_line === selection.to_line
        ? `line ${selection.from_line}`
        : `lines ${selection.from_line}-${selection.to_line}`;
    offered.push({ what: selection, label: "Selection", detail: `${path}, ${lines}` });
  }
  for (const item of problemsOf(view, path))
    if (item.kind === "problem")
      offered.push({ what: item, label: `Problem: ${item.message}`, detail: `${path}:${item.line}` });
  return offered;
}

/** Offers the focused editor's selection and problems while the editor is in edit mode. */
export function registerEditorContext(workspace: {
  readonly openPath: string | null;
  readonly mode: string;
}): () => void {
  return registerContextSource("editor", () => {
    const view = shownView();
    const path = workspace.openPath;
    return view && path && workspace.mode === "edit" ? editorSuggestions(view, path) : [];
  });
}

/** Ask agent from the editor menu: the selection, else the problem at the cursor, else the file. */
export function askAbout(view: EditorView, path: string): void {
  askAgent(selectionOf(view, path) ?? problemAt(view, path, view.state.selection.main.head) ?? { kind: "file", path });
}

/** Adds Ask agent to every problem's hover card; `path` names the view's file. */
export function askOnProblems(path: (view: EditorView) => string | null): Extension {
  return linter(null, {
    tooltipFilter: (diagnostics) =>
      diagnostics.map((diagnostic) => ({
        ...diagnostic,
        actions: [
          ...(diagnostic.actions ?? []),
          {
            name: "Ask agent",
            apply: (view: EditorView, from: number) => {
              const file = path(view);
              if (file) askAgent(problem(view, file, diagnostic, from));
            },
          },
        ],
      })),
  });
}

/** Ask agent from a row of the Problems list. */
export function askAboutProblem(path: string, item: Problem): void {
  askAgent({
    kind: "problem",
    path,
    line: item.line,
    severity: item.severity,
    message: fitBytes(item.message, 4096),
    source: item.source,
  });
}
