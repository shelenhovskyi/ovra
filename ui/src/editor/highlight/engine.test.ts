// @vitest-environment node
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { Parser } from "web-tree-sitter";
import { type Language, languageSource, languages } from "../languages";
import { HighlightClient } from "./client";
import { HighlightEngine, serve } from "./engine";
import type { FromWorker, LanguageSource, Port, Span, ToWorker } from "./protocol";

const require = createRequire(import.meta.url);
const ready = Parser.init();

async function source(id: string): Promise<LanguageSource> {
  const language = languages.find((l) => l.id === id) as Language;
  const loaded = await languageSource(language);
  return { ...loaded, grammar: readFileSync(require.resolve(language.grammar)) };
}

function named(text: string, spans: Span[]): string[] {
  return spans.map(([from, to, cls]) => `${text.slice(from, to)} ${cls}`);
}

beforeAll(() => ready);

describe("language packages", () => {
  test.each(languages.map((l) => l.id))("%s: grammar loads and every query compiles", async (id) => {
    const engine = new HighlightEngine(ready);
    await engine.open(1, await source(id), "");
    expect(engine.highlight(1, 0, 0)).toEqual([]);
  });
});

describe("HighlightEngine", () => {
  test("highlights a range with hierarchical classes", async () => {
    const engine = new HighlightEngine(ready);
    const text = 'fn main() { println!("hi"); }';
    await engine.open(1, await source("rust"), text);
    expect(named(text, engine.highlight(1, 0, text.length))).toMatchInlineSnapshot(`
      [
        "fn tok-keyword",
        "main tok-function",
        "( tok-punctuation tok-punctuation-bracket",
        ") tok-punctuation tok-punctuation-bracket",
        "{ tok-punctuation tok-punctuation-bracket",
        "println tok-function tok-function-macro",
        "! tok-function tok-function-macro",
        "( tok-punctuation tok-punctuation-bracket",
        ""hi" tok-string",
        ") tok-punctuation tok-punctuation-bracket",
        "; tok-punctuation tok-punctuation-delimiter",
        "} tok-punctuation tok-punctuation-bracket",
      ]
    `);
    expect(named(text, engine.highlight(1, 0, 2))).toEqual(["fn tok-keyword"]);
  });

  test("earlier query files win for the same node", async () => {
    const engine = new HighlightEngine(ready);
    const text = "let x: number = 1;";
    await engine.open(1, await source("typescript"), text);
    // `typescript/highlights.scm` comes first and calls `number` a builtin type.
    expect(named(text, engine.highlight(1, 0, text.length))).toContain("number tok-type tok-type-builtin");
  });

  test("incremental edits give the same tree as a fresh parse", async () => {
    const engine = new HighlightEngine(ready);
    const rust = await source("rust");
    const changes = [
      { from: 3, to: 4, insert: "main" },
      { from: 33, to: 33, insert: "\nfn b() -> u8 { 1 }" },
      { from: 0, to: 0, insert: "// \u2713\n" },
    ];
    let text = 'fn a() {}\nconst S: &str = "\u00e9";\n';
    await engine.open(1, rust, text);
    await engine.edit(1, changes);
    for (const { from, to, insert } of changes) text = text.slice(0, from) + insert + text.slice(to);
    expect(text).toBe('// \u2713\nfn main() {}\nconst S: &str = "\u00e9";\nfn b() -> u8 { 1 }\n');
    await engine.open(2, rust, text);
    expect(engine.syntax(1)).toBe(engine.syntax(2));
    expect(engine.highlight(1, 0, text.length)).toEqual(engine.highlight(2, 0, text.length));
  });
});

describe("serve + HighlightClient", () => {
  const channels: MessageChannel[] = [];
  afterEach(() => {
    for (const channel of channels.splice(0)) {
      channel.port1.close();
      channel.port2.close();
    }
  });

  function connect(engine: HighlightEngine): HighlightClient {
    const channel = new MessageChannel();
    channels.push(channel);
    serve(engine, channel.port1 as unknown as Port<ToWorker, FromWorker>);
    return new HighlightClient(channel.port2 as unknown as Port<FromWorker, ToWorker>);
  }

  test("messages are handled in order across the channel", async () => {
    const client = connect(new HighlightEngine(ready));
    const doc = client.open(await source("json"), "{}");
    doc.edit([{ from: 1, to: 1, insert: '"a": 1' }]);
    const spans = await doc.highlight(0, 100);
    expect(named('{"a": 1}', spans)).toEqual([
      '"a" tok-string tok-string-special tok-string-special-key',
      "1 tok-number",
    ]);
    doc.close();
    await expect(doc.highlight(0, 1)).rejects.toThrow("document 1 is not open");
  });

  test("a grammar that fails to load is reported to highlight requests", async () => {
    const client = connect(new HighlightEngine(ready));
    const doc = client.open({ id: "broken", grammar: new Uint8Array([0, 1, 2]), queries: [] }, "x");
    doc.edit([{ from: 0, to: 1, insert: "y" }]);
    await expect(doc.highlight(0, 1)).rejects.toThrow();
  });
});
