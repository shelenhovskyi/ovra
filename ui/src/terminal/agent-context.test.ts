import { expect, it } from "vitest";
import { tailBytes, terminalText } from "./agent-context";

function fake(lines: string[], selection = "") {
  return {
    getSelection: () => selection,
    buffer: {
      active: { length: lines.length, getLine: (i: number) => ({ translateToString: () => lines[i] ?? "" }) },
    },
  } as unknown as Parameters<typeof terminalText>[0];
}

it("takes the terminal's selection, else its last lines without the empty ones at the end", () => {
  expect(terminalText(fake(["a", "b"], "picked"))).toEqual({ text: "picked", selected: true });
  expect(terminalText(fake(["$ cargo test", "test a ... FAILED", "", ""]))).toEqual({
    text: "$ cargo test\ntest a ... FAILED",
    selected: false,
  });
  const many = Array.from({ length: 300 }, (_, i) => `line ${i}`);
  const last = terminalText(fake(many)).text.split("\n");
  expect(last).toHaveLength(80);
  expect(last.at(-1)).toBe("line 299");
});

it("keeps the end of long output, from a line start", () => {
  expect(tailBytes("first\nsecond\nthird", 12)).toBe("third");
  expect(tailBytes("ok", 13)).toBe("ok");
});
