// Format on save (Settings > Editor): the shown file's language server
// formats it just before it is written. Saving never waits long and never
// fails because of the server: no server, no formatting support, an error
// or a slow answer all save the text as it is.

import { getIndentUnit, indentUnit } from "@codemirror/language";
import { LSPPlugin } from "@codemirror/lsp-client";
import type { ChangeSpec } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

/** How long a save waits for the server's formatting before it saves the text unformatted. */
export const FORMAT_ON_SAVE_TIMEOUT_MS = 1000;

type Position = { line: number; character: number };
type TextEdit = { range: { start: Position; end: Position }; newText: string };

/** Formats `view`'s file with its language server, if one serves it and can; resolves once done or given up. */
export async function formatBeforeSave(view: EditorView): Promise<void> {
  const plugin = LSPPlugin.get(view);
  const capabilities = plugin?.client.serverCapabilities as Record<string, unknown> | null | undefined;
  if (!plugin || !capabilities?.documentFormattingProvider) return;
  plugin.client.sync();
  const doc = view.state.doc;
  const request = plugin.client.request<unknown, TextEdit[] | null>("textDocument/formatting", {
    textDocument: { uri: plugin.uri },
    options: {
      tabSize: getIndentUnit(view.state),
      insertSpaces: !view.state.facet(indentUnit).includes("\t"),
    },
  });
  let timer: ReturnType<typeof setTimeout> | undefined;
  const late = new Promise<null>((resolve) => {
    timer = setTimeout(() => resolve(null), FORMAT_ON_SAVE_TIMEOUT_MS);
  });
  let edits: TextEdit[] | null;
  try {
    edits = await Promise.race([request, late]);
  } catch (err) {
    console.warn("[lsp] format on save failed:", err);
    return;
  } finally {
    clearTimeout(timer);
  }
  // An answer to text the person has changed since, or to another file, is not applied.
  if (!edits?.length || view.state.doc !== doc || LSPPlugin.get(view) !== plugin) return;
  try {
    const changes: ChangeSpec[] = edits.map((edit) => ({
      from: plugin.fromPosition(edit.range.start, doc),
      to: plugin.fromPosition(edit.range.end, doc),
      insert: edit.newText,
    }));
    view.dispatch({ changes, userEvent: "format" });
  } catch (err) {
    console.warn("[lsp] format on save not applied:", err);
  }
}
