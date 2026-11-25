// Language-server actions for the keyboard and the palette: each checks that
// the server can do it and says so when it cannot, instead of the library's
// silence. Hover at the cursor is ours: the library shows hover only for the
// pointer.

import { nextDiagnostic, openLintPanel, previousDiagnostic } from "@codemirror/lint";
import { formatDocument, jumpToDefinition, LSPPlugin, renameSymbol } from "@codemirror/lsp-client";
import { StateEffect, StateField } from "@codemirror/state";
import { type EditorView, type KeyBinding, showTooltip, type Tooltip } from "@codemirror/view";

export type LspAction = "definition" | "references" | "rename" | "format" | "hover" | "problems" | "next_problem";

type Context = {
  /** Tells the person something short. */
  note: (text: string) => void;
  /** The name of the server behind `view`. */
  serverName: (plugin: LSPPlugin) => string;
  /** Lists references, also in files the editor does not show. */
  references: (view: EditorView) => boolean;
};

/** What each action needs from the server, and how to say it lacks it. */
const NEEDS: Partial<Record<LspAction, [capability: string, what: string]>> = {
  definition: ["definitionProvider", "go to a definition"],
  references: ["referencesProvider", "find references"],
  rename: ["renameProvider", "rename"],
  format: ["documentFormattingProvider", "format this file"],
  hover: ["hoverProvider", "describe the symbol here"],
};

/** Runs `action` in `view`; returns false only when no language server is attached. */
export function runAction(view: EditorView, action: LspAction, context: Context): boolean {
  const plugin = LSPPlugin.get(view);
  if (action === "problems") return openLintPanel(view);
  if (action === "next_problem") return nextDiagnostic(view);
  if (!plugin) return false;
  const need = NEEDS[action];
  const capabilities = plugin.client.serverCapabilities as Record<string, unknown> | null;
  if (need && capabilities && !capabilities[need[0]]) {
    context.note(`${context.serverName(plugin)} cannot ${need[1]}.`);
    return true;
  }
  switch (action) {
    case "definition":
      return jumpToDefinition(view);
    case "references":
      return context.references(view);
    case "rename":
      return renameSymbol(view);
    case "format":
      return formatDocument(view);
    case "hover":
      showHover(view, plugin, context);
      return true;
  }
}

/** The editor's keys for the actions; they replace the library's own. */
export function actionKeys(context: Context): KeyBinding[] {
  const run = (action: LspAction) => (view: EditorView) => runAction(view, action, context);
  return [
    { key: "F12", run: run("definition"), preventDefault: true },
    { key: "Shift-F12", run: run("references"), preventDefault: true },
    { key: "F2", run: run("rename"), preventDefault: true },
    { key: "Shift-Alt-f", run: run("format"), preventDefault: true },
    // One press: a Mod-k chord would take the global Mod+K (Search or run).
    { key: "Mod-i", run: run("hover"), preventDefault: true },
    { key: "Mod-Shift-m", run: run("problems"), preventDefault: true },
    { key: "F8", run: run("next_problem"), preventDefault: true },
    { key: "Shift-F8", run: previousDiagnostic, preventDefault: true },
    { key: "Escape", run: closeHover },
  ];
}

const setHover = StateEffect.define<Tooltip | null>();

/** The hover shown from the keyboard; gone on the next edit or cursor move. */
export const keyboardHover = StateField.define<Tooltip | null>({
  create: () => null,
  update(tooltip, tr) {
    for (const effect of tr.effects) if (effect.is(setHover)) return effect.value;
    return tr.docChanged || tr.selection ? null : tooltip;
  },
  provide: (field) => showTooltip.from(field),
});

function closeHover(view: EditorView): boolean {
  if (!view.state.field(keyboardHover, false)) return false;
  view.dispatch({ effects: setHover.of(null) });
  return true;
}

type Hover = { contents: unknown } | null;

function showHover(view: EditorView, plugin: LSPPlugin, context: Context): void {
  const pos = view.state.selection.main.head;
  plugin.client.sync();
  plugin.client
    .request<unknown, Hover>("textDocument/hover", {
      position: plugin.toPosition(pos),
      textDocument: { uri: plugin.uri },
    })
    .then(
      (result) => {
        if (LSPPlugin.get(view) !== plugin || view.state.selection.main.head !== pos) return;
        const html = result ? hoverHtml(plugin, result.contents) : "";
        if (!html) {
          context.note("Nothing to show for the symbol at the cursor.");
          return;
        }
        view.dispatch({ effects: setHover.of(tooltipAt(pos, html)) });
      },
      (err: unknown) => context.note(`Hover failed: ${err instanceof Error ? err.message : String(err)}`),
    );
}

function tooltipAt(pos: number, html: string): Tooltip {
  return {
    pos,
    above: true,
    create() {
      const dom = document.createElement("div");
      dom.className = "cm-lsp-hover-tooltip cm-lsp-documentation";
      dom.setAttribute("role", "status");
      // `docToHTML` runs the client's sanitizer.
      dom.innerHTML = html;
      return { dom };
    },
  };
}

/** Hover contents as the library renders them: markup, a marked string, or a list of them. */
function hoverHtml(plugin: LSPPlugin, contents: unknown): string {
  const one = (value: unknown): string => {
    if (typeof value === "string") return plugin.docToHTML(value, "markdown");
    if (value && typeof value === "object" && "value" in value) {
      const { value: text, language, kind } = value as { value: string; language?: string; kind?: string };
      if (kind === "markdown" || kind === "plaintext") return plugin.docToHTML({ kind, value: text });
      return plugin.docToHTML(`\`\`\`${language ?? ""}\n${text}\n\`\`\``, "markdown");
    }
    return "";
  };
  return Array.isArray(contents) ? contents.map(one).filter(Boolean).join("<br>") : one(contents);
}
