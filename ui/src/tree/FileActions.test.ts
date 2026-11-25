import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import { listCommands } from "../keys/commands";
import FileTree from "./FileTree.svelte";
import { dirsFromPaths, FakeCore } from "./fake-core";

globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

let cleanup: (() => void) | null = null;
afterEach(() => {
  cleanup?.();
  cleanup = null;
  document.body.innerHTML = "";
});

async function settle(): Promise<void> {
  for (let i = 0; i < 3; i++) {
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
  }
  flushSync();
}

async function mountTree(paths: string[]) {
  const core = new FakeCore(dirsFromPaths(paths));
  const opened: string[] = [];
  const target = document.body.appendChild(document.createElement("div"));
  const app = mount(FileTree, {
    target,
    props: { transport: core, project: "p1", onopen: (p, mode) => opened.push(`${p} ${mode ?? ""}`) },
  });
  cleanup = () => unmount(app);
  await settle();
  const tree = target.querySelector<HTMLElement>("[role=tree]") as HTMLElement;
  const row = (path: string) => target.querySelector<HTMLElement>(`[data-path="${path}"]`);
  const input = () => target.querySelector<HTMLInputElement>("[role=treeitem] input");
  const press = (el: Element | null, code: string) => {
    el?.dispatchEvent(new KeyboardEvent("keydown", { code, key: code, bubbles: true }));
    flushSync();
  };
  const type = (text: string) => {
    const field = input() as HTMLInputElement;
    field.value = text;
    field.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
  };
  return { core, opened, target, tree, row, input, press, type };
}

describe("file actions in the tree", () => {
  it("renames in place with F2: the name without its extension is selected, Escape cancels", async () => {
    const t = await mountTree(["notes.md", "z.md"]);
    t.press(t.tree, "ArrowDown");
    t.press(t.tree, "F2");
    expect(document.activeElement).toBe(t.input());
    expect([t.input()?.selectionStart, t.input()?.selectionEnd]).toEqual([0, 5]);
    t.press(t.input(), "Escape");
    expect(t.input()).toBeNull();
    expect(document.activeElement).toBe(t.tree);

    t.press(t.tree, "F2");
    t.type("z.md");
    expect(t.target.querySelector("[role=alert]")?.textContent).toBe("“z.md” exists");
    t.type("todo.md");
    t.press(t.input(), "Enter");
    await settle();
    expect(t.row("todo.md")?.getAttribute("aria-selected")).toBe("true");
    expect(t.row("notes.md")).toBeNull();
  });

  it("names a new file in a draft row from the palette and opens it in the editor", async () => {
    const t = await mountTree(["src/a.ts"]);
    await listCommands()
      .find((c) => c.id === "tree.new_file")
      ?.run();
    await settle();
    expect(t.row("/")?.getAttribute("aria-label")).toBe("New file");
    t.type("src/b/c.ts");
    t.press(t.input(), "Enter");
    await settle();
    expect(t.opened).toEqual(["src/b/c.ts edit"]);
    expect(t.row("src/b/c.ts")?.getAttribute("aria-selected")).toBe("true");
    expect(t.row("/")).toBeNull();
  });

  it("moves a dragged file into the folder under it, never a folder into itself", async () => {
    const t = await mountTree(["lib/x.ts", "src/inner/y.ts", "a.ts"]);
    const drag = (type: string, path: string | null) => {
      (path === null ? t.tree : t.row(path))?.dispatchEvent(new Event(type, { bubbles: true, cancelable: true }));
      flushSync();
    };
    drag("dragstart", "src");
    drag("dragover", "src");
    expect(t.target.querySelector(".drop, .drop-root")).toBeNull();
    drag("dragend", "src");

    drag("dragstart", "a.ts");
    drag("dragover", "lib");
    expect(t.row("lib")?.classList.contains("drop")).toBe(true);
    drag("drop", "lib");
    await settle();
    expect(t.row("lib/a.ts")?.getAttribute("aria-selected")).toBe("true");

    drag("dragstart", "lib/a.ts");
    drag("dragover", null);
    expect(t.tree.classList.contains("drop-root")).toBe(true);
    drag("drop", null);
    await settle();
    expect(t.row("a.ts")).not.toBeNull();
  });

  it("asks before Delete moves a file to the trash", async () => {
    const t = await mountTree(["a.ts", "b.ts"]);
    t.press(t.tree, "ArrowDown");
    t.press(t.tree, "Delete");
    const dialog = () => t.target.querySelector("[role=alertdialog]");
    expect(dialog()?.textContent).toContain("Move “a.ts” to Trash?");
    expect(document.activeElement?.textContent).toBe("Move to Trash");
    t.press(document.activeElement, "Escape");
    expect(dialog()).toBeNull();

    t.press(t.tree, "Delete");
    (document.activeElement as HTMLButtonElement).click();
    await settle();
    expect(t.row("a.ts")).toBeNull();
    expect(t.core.calls.find((c) => c.method === "fs.trash")?.params).toEqual({ project: "p1", paths: ["a.ts"] });
  });
});
