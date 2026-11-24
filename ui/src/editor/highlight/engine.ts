// Parses documents with tree-sitter and answers highlight queries. Runs
// inside the highlight worker; free of worker APIs so tests can drive it.

import { Edit, Language, Parser, Query, type Tree } from "web-tree-sitter";
import type { FromWorker, LanguageSource, Port, Span, TextChange, ToWorker } from "./protocol";

type Loaded = { language: Language; query: Query };
type Doc = { text: string; tree: Tree; loaded: Loaded };

export class HighlightEngine {
  private readonly parser: Promise<Parser>;
  private readonly languages = new Map<string, Promise<Loaded>>();
  private readonly docs = new Map<number, Doc>();
  private readonly classes = new Map<string, string>();

  /** `ready` resolves once `Parser.init()` is done. */
  constructor(ready: Promise<void>) {
    this.parser = ready.then(() => new Parser());
  }

  async open(doc: number, source: LanguageSource, text: string): Promise<void> {
    const [parser, loaded] = await Promise.all([this.parser, this.load(source)]);
    parser.setLanguage(loaded.language);
    const tree = parser.parse(text);
    if (!tree) throw new Error("parse failed");
    this.close(doc);
    this.docs.set(doc, { text, tree, loaded });
  }

  async edit(doc: number, changes: TextChange[]): Promise<void> {
    const state = this.doc(doc);
    for (const { from, to, insert } of changes) {
      const text = state.text.slice(0, from) + insert + state.text.slice(to);
      state.tree.edit(
        new Edit({
          startIndex: from,
          oldEndIndex: to,
          newEndIndex: from + insert.length,
          startPosition: point(state.text, from),
          oldEndPosition: point(state.text, to),
          newEndPosition: point(text, from + insert.length),
        }),
      );
      state.text = text;
    }
    const parser = await this.parser;
    parser.setLanguage(state.loaded.language);
    const tree = parser.parse(state.text, state.tree);
    if (!tree) throw new Error("parse failed");
    state.tree.delete();
    state.tree = tree;
  }

  /** Spans in `[from, to)`, sorted by start; for equal ranges the earliest pattern wins. */
  highlight(doc: number, from: number, to: number): Span[] {
    const { tree, loaded } = this.doc(doc);
    const captures = loaded.query.captures(tree.rootNode, { startIndex: from, endIndex: to });
    const best = new Map<string, { patternIndex: number; span: Span }>();
    for (const { name, node, patternIndex } of captures) {
      if (name.startsWith("_") || node.startIndex === node.endIndex) continue;
      const key = `${node.startIndex}:${node.endIndex}`;
      const seen = best.get(key);
      if (seen && seen.patternIndex <= patternIndex) continue;
      best.set(key, { patternIndex, span: [node.startIndex, node.endIndex, this.classesFor(name)] });
    }
    return [...best.values()].map((b) => b.span).sort((a, b) => a[0] - b[0] || b[1] - a[1]);
  }

  /** The tree as an S-expression; for tests. */
  syntax(doc: number): string {
    return this.doc(doc).tree.rootNode.toString();
  }

  close(doc: number): void {
    this.docs.get(doc)?.tree.delete();
    this.docs.delete(doc);
  }

  private doc(doc: number): Doc {
    const state = this.docs.get(doc);
    if (!state) throw new Error(`document ${doc} is not open`);
    return state;
  }

  private load(source: LanguageSource): Promise<Loaded> {
    let loaded = this.languages.get(source.id);
    if (!loaded) {
      loaded = this.parser.then(async () => {
        const language = await Language.load(source.grammar);
        return { language, query: new Query(language, source.queries.join("\n")) };
      });
      this.languages.set(source.id, loaded);
      loaded.catch(() => this.languages.delete(source.id));
    }
    return loaded;
  }

  private classesFor(capture: string): string {
    let classes = this.classes.get(capture);
    if (!classes) {
      const parts = capture.split(".");
      classes = parts.map((_, i) => `tok-${parts.slice(0, i + 1).join("-")}`).join(" ");
      this.classes.set(capture, classes);
    }
    return classes;
  }
}

function point(text: string, index: number): { row: number; column: number } {
  let row = 0;
  let lineStart = 0;
  for (let i = text.indexOf("\n"); i !== -1 && i < index; i = text.indexOf("\n", i + 1)) {
    row++;
    lineStart = i + 1;
  }
  return { row, column: index - lineStart };
}

/**
 * Answers editor messages on `port`, one at a time and in order, so an
 * `edit` never races the `open` before it. A failure is reported to the
 * next `highlight` request of that document.
 */
export function serve(engine: HighlightEngine, port: Port<ToWorker, FromWorker>): void {
  let queue = Promise.resolve();
  const failed = new Map<number, string>();
  port.onmessage = ({ data: message }) => {
    queue = queue.then(async () => {
      if (message.type === "highlight") {
        const error = failed.get(message.doc);
        if (error !== undefined) return port.postMessage({ type: "failed", id: message.id, message: error });
        try {
          port.postMessage({
            type: "spans",
            id: message.id,
            spans: engine.highlight(message.doc, message.from, message.to),
          });
        } catch (err) {
          port.postMessage({ type: "failed", id: message.id, message: String(err) });
        }
        return;
      }
      try {
        if (message.type === "open") {
          failed.delete(message.doc);
          await engine.open(message.doc, message.language, message.text);
        } else if (message.type === "edit") {
          if (!failed.has(message.doc)) await engine.edit(message.doc, message.changes);
        } else {
          failed.delete(message.doc);
          engine.close(message.doc);
        }
      } catch (err) {
        failed.set(message.doc, err instanceof Error ? err.message : String(err));
      }
    });
  };
}
