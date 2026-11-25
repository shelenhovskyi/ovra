import { expect, test } from "vitest";
import { MemoryStorage } from "./fake-core";
import { kindOf, ModeMemory } from "./mode-memory";

test("a kind is the lower-case extension, or the name without one", () => {
  expect([kindOf("docs/README.MD"), kindOf("src/a.test.ts"), kindOf("Makefile"), kindOf(".env")]).toEqual([
    ".md",
    ".ts",
    "makefile",
    ".env",
  ]);
});

test("the last choice per kind survives a new memory over the same storage", () => {
  const storage = new MemoryStorage();
  const memory = new ModeMemory(storage);
  memory.set("a.md", "edit");
  memory.set("b.md", "view");
  memory.set("c.ts", "edit");
  const later = new ModeMemory(storage);
  expect([later.get("x.md"), later.get("y.TS"), later.get("z.rs")]).toEqual(["view", "edit", undefined]);
});

test("broken or failing storage only forgets", () => {
  const storage = new MemoryStorage();
  storage.setItem("ovra.editor.modes", '{".ts": "edit"');
  expect(new ModeMemory(storage).get("a.ts")).toBeUndefined();
  storage.setItem("ovra.editor.modes", '{".md": "sideways", ".ts": "edit"}');
  expect([new ModeMemory(storage).get("a.md"), new ModeMemory(storage).get("a.ts")]).toEqual([undefined, "edit"]);
  const failing = {
    getItem: () => {
      throw new Error("denied");
    },
    setItem: () => {
      throw new Error("quota");
    },
  };
  const memory = new ModeMemory(failing);
  memory.set("a.md", "edit");
  expect(memory.get("b.md")).toBe("edit");
});
