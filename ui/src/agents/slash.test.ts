import { describe, expect, it } from "vitest";
import type { CatalogItem } from "../lib/protocol";
import { applyRow, slashCommands, slashQuery, slashRows } from "./slash";

const item = (over: Partial<CatalogItem>): CatalogItem => ({
  kind: "command",
  name: "/ship",
  description: null,
  path: ".claude/commands/ship.md",
  agents: ["claude"],
  loads: "when_trusted",
  fields: [],
  files: [],
  problem: null,
  ...over,
});

describe("slash menu", () => {
  it("opens only for a / at the very start, while the caret is in that word", () => {
    expect(slashQuery("/", 1)).toBe("");
    expect(slashQuery("/rev", 4)).toBe("rev");
    expect(slashQuery("/rev more", 9)).toBeNull();
    expect(slashQuery("see src/lib", 11)).toBeNull();
    expect(slashQuery(" /x", 3)).toBeNull();
  });

  it("lists Claude's project commands and skills Ovra loads, once each, without the slash", () => {
    const commands = slashCommands([
      item({}),
      item({ kind: "skill", name: "review", description: "Reviews the diff", loads: "always" }),
      item({ name: "/ship" }),
      item({ name: "/gem", agents: ["gemini"] }),
      item({ name: "/mcp", loads: "not_in_ovra" }),
      item({ kind: "rules", name: "CLAUDE.md" }),
    ]);
    expect(commands.map((c) => [c.name, c.whenTrusted])).toEqual([
      ["ship", true],
      ["review", false],
    ]);
  });

  it("puts templates before commands and fills the box from the pick", () => {
    const templates = [{ id: "t1", name: "Review for bugs", text: "Review the diff for bugs.\nBe brief." }];
    const commands = slashCommands([item({ kind: "skill", name: "review", description: null, loads: "always" })]);
    const rows = slashRows("rev", templates, commands);
    expect(rows.map((r) => [r.kind, r.label, r.detail])).toEqual([
      ["template", "Review for bugs", "Review the diff for bugs."],
      ["command", "/review", "Skill"],
    ]);
    const [template, command] = rows;
    if (!template || !command) throw new Error("no rows");
    expect(applyRow(template, "/rev", "rev")).toEqual({ value: "Review the diff for bugs.\nBe brief.", caret: 35 });
    expect(applyRow(command, "/rev  the login", "rev")).toEqual({ value: "/review the login", caret: 8 });
    expect(slashRows("zzz", templates, commands)).toEqual([]);
  });
});
