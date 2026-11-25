import { describe, expect, test } from "vitest";
import { grammarUrls } from "./grammars";
import { commentTokens, languageFor, languages } from "./languages";

describe("languages", () => {
  test("comment markers follow the language; JSON and plain text have none", () => {
    const markers = (path: string) => commentTokens(languageFor(path));
    expect([markers("a.py"), markers("a.rs")?.line, markers("a.css")?.line]).toEqual([{ line: "#" }, "//", undefined]);
    expect([markers("a.json"), markers("notes.txt")]).toEqual([null, null]);
  });

  test("every language package names a bundled grammar", () => {
    expect(languages.map((l) => l.id).sort()).toEqual([
      "css",
      "javascript",
      "json",
      "python",
      "rust",
      "tsx",
      "typescript",
    ]);
    for (const language of languages) expect(grammarUrls[language.grammar], language.id).toBeTypeOf("string");
  });

  test.each([
    ["src/main.rs", "rust"],
    ["ui/App.TSX", "tsx"],
    ["types.d.ts", "typescript"],
    ["config/.babelrc", "json"],
    ["README", null],
    [".gitignore", null],
    ["notes.txt", null],
  ])("%s -> %s", (path, id) => {
    expect(languageFor(path)?.id ?? null).toBe(id);
  });
});
