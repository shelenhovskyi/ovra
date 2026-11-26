import { expect, it } from "vitest";
import type { CatalogItem } from "../lib/protocol";
import { groups, loadsLine, title, touchesCatalog, withoutHeader } from "./texts";

function item(over: Partial<CatalogItem>): CatalogItem {
  return {
    kind: "skill",
    name: "release",
    description: null,
    path: ".claude/skills/release/SKILL.md",
    agents: ["claude"],
    loads: "when_trusted",
    fields: [],
    files: [],
    problem: null,
    ...over,
  };
}

it("groups by kind in the list's order, with keys that tell same-named hooks apart", () => {
  const hook = item({ kind: "hook", name: "PreToolUse", path: ".claude/settings.json", description: "./a.sh" });
  const shown = groups(
    [item({}), hook, { ...hook, description: "./b.sh" }, item({ kind: "rules", name: "AGENTS.md", path: "AGENTS.md" })],
    "",
  );
  expect(shown.map((g) => [g.title, g.items.map((i) => i.key)])).toEqual([
    ["Instructions", ["rules|AGENTS.md|AGENTS.md|0"]],
    ["Skills", ["skill|.claude/skills/release/SKILL.md|release|0"]],
    ["Hooks", ["hook|.claude/settings.json|PreToolUse|0", "hook|.claude/settings.json|PreToolUse|1"]],
  ]);
});

it("filters on every word, in names, event names in words, descriptions and paths", () => {
  const items = [
    item({ description: "Cuts a release" }),
    item({ kind: "hook", name: "PreToolUse", path: ".claude/settings.json" }),
  ];
  const keys = (filter: string) => groups(items, filter).flatMap((g) => g.items.map((i) => i.item.name));
  expect(keys("release cuts")).toEqual(["release"]);
  expect(keys("before tool")).toEqual(["PreToolUse"]);
  expect(keys("settings.json")).toEqual(["PreToolUse"]);
  expect(keys("nothing")).toEqual([]);
});

it("says when Claude loads an item, and offers trust only where it helps", () => {
  expect(loadsLine("when_trusted", false)).toEqual({
    text: "Claude uses it only once you trust this project.",
    trust: true,
  });
  expect(loadsLine("when_trusted", true)?.trust).toBe(false);
  expect(loadsLine("not_in_ovra", false)?.text).toMatch(/Not used in Ovra/);
  expect(loadsLine("agent", true)).toBeNull();
  expect(title(item({ kind: "hook", name: "Stop" }))).toBe("When the agent finishes");
  expect(title(item({ kind: "hook", name: "SomethingNew" }))).toBe("SomethingNew");
});

it("knows which changes read the list again and strips a Markdown header", () => {
  expect(touchesCatalog(["src/a.ts"])).toBe(false);
  expect(touchesCatalog(["src/a.ts", ".claude/skills/x/SKILL.md"])).toBe(true);
  expect(touchesCatalog(["AGENTS.md"])).toBe(true);
  expect(touchesCatalog([".claudex"])).toBe(false);
  expect(withoutHeader("---\nname: x\n---\n# Body")).toBe("# Body");
  expect(withoutHeader("# No header\n---\n")).toBe("# No header\n---\n");
});
