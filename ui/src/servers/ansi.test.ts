import { describe, expect, it } from "vitest";
import { parseAnsi } from "./ansi";

const strip = (line: string) =>
  parseAnsi(line)
    .map((run) => run.text)
    .join("");

describe("parseAnsi", () => {
  it("keeps colours and bold as runs", () => {
    expect(parseAnsi("\x1b[32mVITE\x1b[0m ready in \x1b[1;93m300\x1b[22m ms")).toEqual([
      { text: "VITE", fg: 2 },
      { text: " ready in " },
      { text: "300", fg: 11, bold: true },
      { text: " ms", fg: 11 },
    ]);
  });

  it("keeps the low 256 colours and drops true colour", () => {
    expect(parseAnsi("\x1b[38;5;1ma\x1b[38;5;200mb\x1b[48;2;1;2;3mc")).toEqual([
      { text: "a", fg: 1 },
      { text: "b" },
      { text: "c" },
    ]);
  });

  it("removes other escapes and control characters", () => {
    const line = "\x1b]8;;http://x\x07link\x1b]8;;\x07 \x1b[2K\x1b[1Gdone\r\x07";
    expect(strip(line)).toBe("link done");
  });

  it("leaves markup as text", () => {
    expect(parseAnsi("<img src=x onerror=alert(1)>")).toEqual([{ text: "<img src=x onerror=alert(1)>" }]);
  });
});
