// Quick fixes (`textDocument/codeAction`): the server's fixes and
// refactorings at the cursor, in a menu. A picked fix lands as edits the
// person reviews, like a rename (`renameAcross`): nothing is written to disk.
// Fixes that only name a server command are left out, since the core never
// applies the edits a server asks for on its own (`workspace/applyEdit`).

import type { LSPPlugin } from "@codemirror/lsp-client";
import type { EditorView } from "@codemirror/view";

type Position = { line: number; character: number };
type Range = { start: Position; end: Position };

/** The parts of an LSP `CodeAction` the menu uses. */
export type CodeAction = {
  title: string;
  kind?: string;
  isPreferred?: boolean;
  disabled?: { reason: string };
  edit?: { changes?: Record<string, unknown[]> };
  command?: unknown;
  data?: unknown;
};

/** What the client tells servers it can show and apply. */
export const CODE_ACTION_CAPABILITIES = {
  dynamicRegistration: false,
  codeActionLiteralSupport: {
    codeActionKind: {
      valueSet: ["", "quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite", "source"],
    },
  },
  isPreferredSupport: true,
  disabledSupport: true,
  dataSupport: true,
  resolveSupport: { properties: ["edit"] },
};

/** The menu lists at most this many fixes. */
const MAX_FIXES = 20;

/** Whether two LSP ranges touch, for the diagnostics a fix may answer. */
function touches(a: Range, b: Range): boolean {
  const before = (p: Position, q: Position) => p.line < q.line || (p.line === q.line && p.character < q.character);
  return !before(a.end, b.start) && !before(b.end, a.start);
}

/**
 * The actions a menu can offer, preferred first: those with edits, and
 * those the server fills in on `codeAction/resolve`. Commands and disabled
 * actions are left out.
 */
function usableActions(result: unknown, canResolve: boolean): CodeAction[] {
  if (!Array.isArray(result)) return [];
  const usable = (result as CodeAction[]).filter(
    (a) =>
      a &&
      typeof a.title === "string" &&
      typeof a.command !== "string" &&
      !a.disabled &&
      (a.edit !== undefined || (canResolve && a.command === undefined)),
  );
  usable.sort((a, b) => Number(Boolean(b.isPreferred)) - Number(Boolean(a.isPreferred)));
  return usable.slice(0, MAX_FIXES);
}

type Options = {
  /** The diagnostics the server last published for this file, as it sent them. */
  diagnostics: unknown[];
  /** Shows the fixes for the person to pick from, at a point in the window. */
  pick: (at: { x: number; y: number }, actions: CodeAction[], run: (action: CodeAction) => void) => void;
  /** Applies a `WorkspaceEdit`'s `changes`, all or none. */
  apply: (changes: Record<string, unknown[]>) => void;
  note: (text: string) => void;
};

/** Asks the server for fixes at the selection and offers them. */
export function quickFix(view: EditorView, plugin: LSPPlugin, options: Options): void {
  const { from, to, head } = view.state.selection.main;
  const range: Range = { start: plugin.toPosition(from), end: plugin.toPosition(to) };
  const diagnostics = options.diagnostics.filter((d) => {
    const r = (d as { range?: Range } | null)?.range;
    return r?.start && r.end && touches(r, range);
  });
  const capabilities = plugin.client.serverCapabilities as { codeActionProvider?: unknown } | null;
  const provider = capabilities?.codeActionProvider;
  const canResolve = typeof provider === "object" && provider !== null && "resolveProvider" in provider;
  plugin.client.sync();
  plugin.client
    .request<unknown, unknown>("textDocument/codeAction", {
      textDocument: { uri: plugin.uri },
      range,
      context: { diagnostics, triggerKind: 1 },
    })
    .then(
      (result) => {
        if (view.state.selection.main.head !== head) return;
        const actions = usableActions(result, canResolve);
        if (actions.length === 0) {
          options.note("No quick fixes here.");
          return;
        }
        const coords = view.coordsAtPos(head);
        const at = coords ? { x: coords.left, y: coords.bottom } : { x: 0, y: 0 };
        options.pick(at, actions, (action) => void run(plugin, action, canResolve, options));
      },
      (err: unknown) => options.note(`Quick fix failed: ${err instanceof Error ? err.message : String(err)}`),
    );
}

async function run(plugin: LSPPlugin, action: CodeAction, canResolve: boolean, options: Options): Promise<void> {
  let picked = action;
  if (picked.edit === undefined && canResolve) {
    try {
      picked = await plugin.client.request<CodeAction, CodeAction>("codeAction/resolve", action);
    } catch (err) {
      options.note(`Quick fix failed: ${err instanceof Error ? err.message : String(err)}`);
      return;
    }
  }
  const changes = picked?.edit?.changes;
  if (!changes || Object.keys(changes).length === 0) {
    options.note(`"${action.title}" needs the language server to change files itself, which Ovra does not allow.`);
    return;
  }
  options.apply(changes);
}
