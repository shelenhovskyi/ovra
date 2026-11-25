import { describe, expect, it } from "vitest";
import type { Span } from "./protocol";
import { lineSegments, sliceSegments } from "./static";

describe("lineSegments", () => {
  it("cuts lines at newlines and lets the innermost span win", () => {
    const text = 'let s = "a\\n";\r\n// x';
    const spans: Span[] = [
      [0, 3, "tok-keyword"],
      [8, 13, "tok-string"],
      [10, 12, "tok-string-escape"],
      [16, 20, "tok-comment"],
    ];
    const lines = lineSegments(text, spans);
    expect(lines).toEqual([
      [
        { text: "let", cls: "tok-keyword" },
        { text: " s = ", cls: "" },
        { text: '"a', cls: "tok-string" },
        { text: "\\n", cls: "tok-string-escape" },
        { text: '"', cls: "tok-string" },
        { text: ";", cls: "" },
      ],
      [{ text: "// x", cls: "tok-comment" }],
    ]);
    expect(sliceSegments(lines[0] ?? [], 2, 9)).toEqual([
      { text: "t", cls: "tok-keyword" },
      { text: " s = ", cls: "" },
      { text: '"', cls: "tok-string" },
    ]);
  });
});
