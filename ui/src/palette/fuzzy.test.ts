import { describe, expect, it } from "vitest";
import type { Command } from "../keys/commands";
import { filterCommands, fuzzyMatch, segments } from "./fuzzy";

const command = (id: string, title: string): Command => ({ id, title, run: () => {} });

describe("fuzzy matching", () => {
  it("matches characters in order, ignoring case and spaces in the query", () => {
    expect(fuzzyMatch("scp", "Show command palette")?.positions).toEqual([0, 5, 13]);
    expect(fuzzyMatch("show pal", "Show command palette")).not.toBeNull();
    expect(fuzzyMatch("pcs", "Show command palette")).toBeNull();
    expect(fuzzyMatch("", "anything")).toEqual({ score: 0, positions: [] });
  });

  it("prefers word starts and runs over scattered letters", () => {
    // "of" should hit "Open File", not the o and f inside "Toggle folding".
    expect(fuzzyMatch("of", "Open File")?.positions).toEqual([0, 5]);
    const open = fuzzyMatch("of", "Open File")?.score ?? 0;
    const toggle = fuzzyMatch("of", "Toggle Profile")?.score ?? 0;
    expect(open).toBeGreaterThan(toggle);
  });

  it("ranks matches best first and keeps all commands for an empty query", () => {
    const commands = [command("a", "Toggle profile"), command("b", "Open file"), command("c", "Close")];
    expect(filterCommands(commands, "of").map((item) => item.command.id)).toEqual(["b", "a"]);
    expect(filterCommands(commands, "").map((item) => item.command.id)).toEqual(["a", "b", "c"]);
  });

  it("splits a title into highlighted runs", () => {
    expect(segments("Open", [0, 1, 3])).toEqual([
      { text: "Op", hit: true },
      { text: "e", hit: false },
      { text: "n", hit: true },
    ]);
  });
});
