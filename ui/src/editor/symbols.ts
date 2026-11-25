// Go to symbol in file: the language server's document symbols when one
// serves the file and lists them, else a plain outline read from the text
// (functions, classes, types, Markdown headings). Shown in the palette.

import { LSPPlugin } from "@codemirror/lsp-client";
import type { Text } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { Entry, Source } from "../palette/sources";

/** A symbol of the shown file; `from`-`to` is its name in the text. */
export type FileSymbol = { name: string; kind: string; container?: string; from: number; to: number };

/** LSP `SymbolKind` as words; unknown kinds say "symbol". */
const KINDS = [
  "",
  "file",
  "module",
  "namespace",
  "package",
  "class",
  "method",
  "property",
  "field",
  "constructor",
  "enum",
  "interface",
  "function",
  "variable",
  "constant",
  "string",
  "number",
  "boolean",
  "array",
  "object",
  "key",
  "null",
  "enum member",
  "struct",
  "event",
  "operator",
  "type parameter",
];

type Position = { line: number; character: number };
type Range = { start: Position; end: Position };
/** `DocumentSymbol` (nested) or `SymbolInformation` (flat), as servers answer. */
type ServerSymbol = {
  name: string;
  kind: number;
  selectionRange?: Range;
  range?: Range;
  children?: ServerSymbol[];
  location?: { range: Range };
  containerName?: string;
};

/** The server's answer as a flat list in text order; `offset` turns a position into a document offset. */
export function flattenSymbols(answer: unknown, offset: (position: Position) => number): FileSymbol[] {
  const out: FileSymbol[] = [];
  const visit = (symbols: ServerSymbol[], container?: string) => {
    for (const symbol of symbols) {
      const range = symbol.selectionRange ?? symbol.range ?? symbol.location?.range;
      if (!range || typeof symbol.name !== "string") continue;
      out.push({
        name: symbol.name,
        kind: KINDS[symbol.kind] || "symbol",
        container: container ?? (symbol.containerName || undefined),
        from: offset(range.start),
        to: offset(range.end),
      });
      if (Array.isArray(symbol.children)) visit(symbol.children, symbol.name);
    }
  };
  if (Array.isArray(answer)) visit(answer as ServerSymbol[]);
  return out.sort((a, b) => a.from - b.from);
}

/** Asks the server behind `view` for its symbols; null when none serves it or it cannot list them. */
async function serverSymbols(view: EditorView): Promise<FileSymbol[] | null> {
  const plugin = LSPPlugin.get(view);
  const capabilities = plugin?.client.serverCapabilities as Record<string, unknown> | null | undefined;
  if (!plugin || !capabilities?.documentSymbolProvider) return null;
  plugin.client.sync();
  const doc = view.state.doc;
  const answer = await plugin.client.request<unknown, unknown>("textDocument/documentSymbol", {
    textDocument: { uri: plugin.uri },
  });
  return flattenSymbols(answer, (position) => clampedOffset(doc, position));
}

function clampedOffset(doc: Text, { line, character }: Position): number {
  const at = doc.line(Math.min(Math.max(line + 1, 1), doc.lines));
  return Math.min(at.from + Math.max(character, 0), at.to);
}

/** Lines that start a symbol in common languages: a kind and the name's group. */
const OUTLINE: [RegExp, string][] = [
  [/^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\*?\s+([\w$]+)/, "function"],
  [/^\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?class\s+([\w$]+)/, "class"],
  [/^\s*(?:export\s+)?(?:declare\s+)?(interface|type|enum)\s+([\w$]+)/, ""],
  [/^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+"\w+"\s+)?fn\s+(\w+)/, "function"],
  [/^\s*(?:pub(?:\([^)]*\))?\s+)?(struct|enum|trait|mod|union)\s+(\w+)/, ""],
  [/^\s*impl\b(?:<[^>]*>)?\s+([^{]+?)\s*(?:\{.*)?$/, "impl"],
  [/^\s*(?:async\s+)?def\s+(\w+)/, "function"],
];

/** A symbol list read from the text alone, for files no server serves. */
export function outlineSymbols(doc: Text, path: string): FileSymbol[] {
  const markdown = /\.(md|markdown)$/i.test(path);
  const out: FileSymbol[] = [];
  let fenced = false;
  for (let n = 1; n <= doc.lines; n++) {
    const line = doc.line(n);
    const text = line.text;
    if (markdown) {
      if (/^\s*(```|~~~)/.test(text)) fenced = !fenced;
      const heading = fenced ? null : /^(#{1,6})\s+(.*?)\s*#*\s*$/.exec(text);
      if (heading?.[2]) {
        const from = line.from + text.indexOf(heading[2]);
        out.push({ name: heading[2], kind: `heading ${heading[1]?.length}`, from, to: from + heading[2].length });
      }
      continue;
    }
    for (const [pattern, kind] of OUTLINE) {
      const match = pattern.exec(text);
      if (!match) continue;
      // Patterns with an empty kind capture the kind first, then the name.
      const name = (kind ? match[1] : match[2]) ?? "";
      const from = line.from + match.index + match[0].lastIndexOf(name);
      out.push({ name, kind: kind || (match[1] ?? ""), from, to: from + name.length });
      break;
    }
  }
  return out;
}

/** Selects the symbol's name, scrolled to the middle, and focuses the text. */
function jump(view: EditorView, symbol: FileSymbol): void {
  const length = view.state.doc.length;
  const from = Math.min(symbol.from, length);
  view.dispatch({
    selection: { anchor: from, head: Math.min(symbol.to, length) },
    effects: EditorView.scrollIntoView(from, { y: "center" }),
  });
  view.focus();
}

/** The palette list of the symbols in `view`'s file at `path`. */
export async function symbolSource(view: EditorView, path: string): Promise<Source> {
  let symbols: FileSymbol[] | null = null;
  let note = "";
  try {
    symbols = await serverSymbols(view);
  } catch {
    note = " (the language server did not answer; showing an outline)";
  }
  const entries: Entry[] = (symbols ?? outlineSymbols(view.state.doc, path)).map((symbol, i) => ({
    id: `symbol:${i}`,
    label: symbol.name,
    detail: symbol.container ? `${symbol.kind} in ${symbol.container}` : symbol.kind,
    run: () => jump(view, symbol),
  }));
  const name = path.slice(path.lastIndexOf("/") + 1);
  return {
    name: "Go to symbol",
    placeholder: `Go to symbol in ${name}${note}`,
    empty: entries.length === 0 ? `No symbols found in ${name}` : "No matching symbols",
    limit: 200,
    entries: () => entries,
    idle: (all) => all,
  };
}
