import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { listCommands } from "../keys/commands";
import { closeContextMenu, contextMenu, type MenuItem } from "../menu/context-menu.svelte";
import FileTree from "./FileTree.svelte";
import { dirsFromPaths, FakeCore } from "./fake-core";
import { HOVER_EXPAND_MS } from "./hover-expand";

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

  it("starts a new folder from the header, and cuts, copies, pastes and trashes from the palette", async () => {
    const t = await mountTree(["src/a.ts", "b.ts"]);
    const run = (id: string) =>
      listCommands()
        .find((c) => c.id === id)
        ?.run();
    t.target.querySelector<HTMLButtonElement>('[aria-label="New folder"]')?.click();
    await settle();
    expect(t.row("/")?.getAttribute("aria-label")).toBe("New folder");
    t.press(t.input(), "Escape");

    t.row("b.ts")?.click();
    await run("tree.copy");
    t.row("src")?.click();
    await settle();
    await run("tree.paste");
    await settle();
    expect(t.core.calls.find((c) => c.method === "fs.copy")?.params).toMatchObject({ from: "b.ts", to: "src/b.ts" });
    await run("tree.trash");
    expect(t.target.querySelector("[role=alertdialog]")?.textContent).toContain("Move “b.ts” to Trash?");
    expect(listCommands().find((c) => c.id === "tree.trash")?.keywords).toContain("delete");
  });

  it("opens a row's menu on right-click or Shift+F10 and the project's below the rows", async () => {
    const t = await mountTree(["src/a.ts", "b.ts"]);
    const labels = () =>
      (contextMenu.open?.entries ?? []).map((e) => (e === "separator" ? "-" : `${e.label}${e.danger ? "!" : ""}`));
    t.row("b.ts")?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2 }));
    flushSync();
    expect(t.row("b.ts")?.getAttribute("aria-selected")).toBe("true");
    expect(labels()).toEqual([
      "Open Containing Folder",
      "Open in terminal",
      "-",
      "Cut",
      "Copy",
      "Copy path",
      "Duplicate",
      "Rename…",
      "-",
      "Move to Trash…!",
    ]);
    const duplicate = contextMenu.open?.entries.find((e) => e !== "separator" && e.label === "Duplicate");
    closeContextMenu(false);
    await (duplicate as MenuItem).run();
    await settle();
    expect(t.input()?.value).toBe("b copy.ts");
    t.press(t.input(), "Escape");

    t.tree.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2 }));
    expect(labels()).toEqual([
      "New file…",
      "New folder…",
      "Reveal project folder",
      "Open in terminal",
      "-",
      "Collapse all",
    ]);
    t.tree.dispatchEvent(new KeyboardEvent("keydown", { code: "F10", key: "F10", shiftKey: true, bubbles: true }));
    expect(labels()[7]).toBe("Rename…");
    closeContextMenu(false);
  });

  it("moves a dragged file into the folder under it, never a folder into itself, and opens a folder it rests on", async () => {
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

    // Resting on a collapsed folder opens it.
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    drag("dragstart", "a.ts");
    drag("dragover", "src");
    drag("dragover", "src");
    vi.advanceTimersByTime(HOVER_EXPAND_MS);
    vi.useRealTimers();
    await settle();
    expect(t.row("src")?.getAttribute("aria-expanded")).toBe("true");
    drag("dragend", "a.ts");
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

    // Tab stays on its two buttons; focus going elsewhere drops the question.
    t.press(t.tree, "Delete");
    t.press(document.activeElement, "Tab");
    expect(document.activeElement?.textContent).toBe("Cancel");
    t.press(document.activeElement, "Tab");
    expect(document.activeElement?.textContent).toBe("Move to Trash");
    const elsewhere = document.body.appendChild(document.createElement("button"));
    elsewhere.focus();
    flushSync();
    expect(dialog()).toBeNull();
    expect(document.activeElement).toBe(elsewhere);

    t.tree.focus();
    t.press(t.tree, "Delete");
    (document.activeElement as HTMLButtonElement).click();
    await settle();
    expect(t.row("a.ts")).toBeNull();
    expect(t.core.calls.find((c) => c.method === "fs.trash")?.params).toEqual({ project: "p1", paths: ["a.ts"] });
  });

  it("extends the selection with Shift+End and Shift+Home, and selects every row with Ctrl+A", async () => {
    const t = await mountTree(["a.ts", "b.ts", "c.ts", "d.ts"]);
    const key = (code: string, mods: KeyboardEventInit = {}) => {
      t.tree.dispatchEvent(new KeyboardEvent("keydown", { code, key: code, bubbles: true, ...mods }));
      flushSync();
    };
    const selected = () =>
      [...t.target.querySelectorAll('[aria-selected="true"]')].map((r) => r.getAttribute("data-path"));
    t.row("b.ts")?.click();
    key("End", { shiftKey: true });
    expect(selected()).toEqual(["b.ts", "c.ts", "d.ts"]);
    key("Home", { shiftKey: true });
    expect(selected()).toEqual(["a.ts", "b.ts"]);
    key("KeyA", { ctrlKey: true });
    expect(selected()).toEqual(["a.ts", "b.ts", "c.ts", "d.ts"]);
  });

  it("selects several rows with Ctrl+click, Shift+click and Shift+Arrow, and acts on all of them", async () => {
    const t = await mountTree(["lib/x.ts", "a.ts", "b.ts", "c.ts"]);
    const click = (path: string, mods: MouseEventInit = {}) => {
      t.row(path)?.dispatchEvent(new MouseEvent("click", { bubbles: true, ...mods }));
      flushSync();
    };
    const selectedPaths = () =>
      [...t.target.querySelectorAll("[aria-selected=true]")].map((el) => (el as HTMLElement).dataset.path);
    expect(t.tree.getAttribute("aria-multiselectable")).toBe("true");
    click("a.ts");
    click("c.ts", { ctrlKey: true });
    expect(selectedPaths()).toEqual(["a.ts", "c.ts"]);
    expect(t.opened).toEqual(["a.ts "]);
    click("lib", { shiftKey: true });
    expect(selectedPaths()).toEqual(["lib", "a.ts", "b.ts", "c.ts"]);
    t.press(t.tree, "Escape");
    expect(selectedPaths()).toEqual(["lib"]);

    t.tree.dispatchEvent(
      new KeyboardEvent("keydown", { code: "ArrowDown", key: "ArrowDown", shiftKey: true, bubbles: true }),
    );
    t.tree.dispatchEvent(
      new KeyboardEvent("keydown", { code: "ArrowDown", key: "ArrowDown", shiftKey: true, bubbles: true }),
    );
    flushSync();
    expect(selectedPaths()).toEqual(["lib", "a.ts", "b.ts"]);
    t.row("a.ts")?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2 }));
    const labels = (contextMenu.open?.entries ?? []).map((e) => (e === "separator" ? "-" : e.label));
    expect(labels).toEqual(["Cut", "Copy", "Copy paths", "-", "Move to Trash…"]);
    closeContextMenu(false);

    // Mod+X dims what is cut, and what is in a cut folder, until Escape.
    t.tree.dispatchEvent(new KeyboardEvent("keydown", { code: "KeyX", key: "x", ctrlKey: true, bubbles: true }));
    flushSync();
    const dimmed = () => [...t.target.querySelectorAll(".cut")].map((el) => (el as HTMLElement).dataset.path);
    expect(dimmed()).toEqual(["lib", "a.ts", "b.ts"]);
    t.press(t.tree, "Escape");
    expect(dimmed()).toEqual([]);

    // Dragging one of them moves the others too; nothing goes into a dragged folder.
    const drag = (type: string, path: string) => {
      t.row(path)?.dispatchEvent(new Event(type, { bubbles: true, cancelable: true }));
      flushSync();
    };
    click("a.ts");
    click("b.ts", { ctrlKey: true });
    drag("dragstart", "a.ts");
    drag("drop", "lib");
    await settle();
    expect(t.core.calls.filter((c) => c.method === "fs.rename").map((c) => c.params)).toEqual([
      { project: "p1", from: "a.ts", to: "lib/a.ts" },
      { project: "p1", from: "b.ts", to: "lib/b.ts" },
    ]);

    click("lib/a.ts");
    click("c.ts", { shiftKey: true });
    t.press(t.tree, "Delete");
    expect(t.target.querySelector("[role=alertdialog]")?.textContent).toContain("Move 4 items to Trash?");
  });
});
