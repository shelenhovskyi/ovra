import { describe, expect, it } from "vitest";
import { absolutePath, copyName, keyAction, movedPath, moveTarget, nameProblem, revealLabel, stemRange } from "./ops";

const siblings = [
  { name: "src", kind: "dir" },
  { name: "README.md", kind: "file" },
];

describe("nameProblem", () => {
  it("refuses empty names, slashes at the ends, dot parts and .git", () => {
    const check = (name: string) => nameProblem(name, siblings, { nested: true });
    expect(check(" ")).toBe("Enter a name");
    expect(check("/a")).toBe("No / at the start or end");
    expect(check("a/")).toBe("No / at the start or end");
    expect(check("a//b")).toBe("Not a valid name");
    expect(check("../a")).toBe("Not a valid name");
    expect(check("a/.GIT/b")).toBe("Not allowed: .git");
    expect(check("a/b/c.ts")).toBeNull();
  });

  it("knows existing siblings: a file blocks, a folder may hold the new path", () => {
    expect(nameProblem("readme.md", siblings, { nested: true })).toBe("“README.md” exists");
    expect(nameProblem("src/new.ts", siblings, { nested: true })).toBeNull();
    expect(nameProblem("README.md/x", siblings, { nested: true })).toBe("“README.md” is a file");
  });

  it("lets a rename change only the case, and keeps it to one name", () => {
    expect(nameProblem("readme.md", siblings, { nested: false, current: "README.md" })).toBeNull();
    expect(nameProblem("src", siblings, { nested: false, current: "README.md" })).toBe("“src” exists");
    expect(nameProblem("a/b", siblings, { nested: false, current: "README.md" })).toBe("No / in a name");
  });
});

describe("names and paths", () => {
  it("selects a file's name without its extension, all of a folder's or dotfile's", () => {
    expect(stemRange("model.test.ts", false)).toEqual([0, 10]);
    expect(stemRange(".env", false)).toEqual([0, 4]);
    expect(stemRange("v1.2", true)).toEqual([0, 4]);
  });

  it("names copies like Finder", () => {
    expect(copyName("a.ts", false, ["a.ts"])).toBe("a copy.ts");
    expect(copyName("a.ts", false, ["a.ts", "a copy.ts", "A Copy 2.ts"])).toBe("a copy 3.ts");
    expect(copyName("lib.d", true, ["lib.d"])).toBe("lib.d copy");
  });

  it("joins the root with its own separator", () => {
    expect(absolutePath("/home/me/app", "src/a.ts")).toBe("/home/me/app/src/a.ts");
    expect(absolutePath("C:\\code\\app\\", "src/a.ts")).toBe("C:\\code\\app\\src\\a.ts");
    expect(absolutePath("/home/me/app", "")).toBe("/home/me/app");
  });

  it("maps paths through a move and refuses moves into itself or in place", () => {
    expect(movedPath("src/a/b.ts", "src/a", "lib/a")).toBe("lib/a/b.ts");
    expect(movedPath("src/ab.ts", "src/a", "lib/a")).toBeNull();
    expect(moveTarget("src/a.ts", "lib")).toBe("lib/a.ts");
    expect(moveTarget("src/a.ts", "src")).toBeNull();
    expect(moveTarget("src", "src/inner")).toBeNull();
    expect(moveTarget("src/a", "")).toBe("a");
  });
});

describe("platform words and keys", () => {
  it("names the file manager per system", () => {
    expect(revealLabel("mac")).toBe("Reveal in Finder");
    expect(revealLabel("windows")).toBe("Reveal in File Explorer");
    expect(revealLabel("linux")).toBe("Open Containing Folder");
  });

  it("matches F2 and the trash keys on the physical key", () => {
    const key = (code: string, mods: Partial<KeyboardEvent> = {}) => ({
      code,
      ctrlKey: false,
      metaKey: false,
      altKey: false,
      shiftKey: false,
      ...mods,
    });
    expect(keyAction(key("F2"), false)).toBe("rename");
    expect(keyAction(key("F2", { shiftKey: true }), false)).toBeNull();
    expect(keyAction(key("Delete"), false)).toBe("trash");
    expect(keyAction(key("Backspace", { metaKey: true }), true)).toBe("trash");
    expect(keyAction(key("Backspace", { metaKey: true }), false)).toBeNull();
    expect(keyAction(key("Backspace"), true)).toBeNull();
  });
});
