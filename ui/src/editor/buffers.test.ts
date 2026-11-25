import { toggleComment, undoDepth } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { afterEach, beforeAll, describe, expect, test, vi } from "vitest";
import { AUTO_SAVE_DELAY_MS, AutoSaver } from "./autosave";
import { type Buffer, Buffers } from "./buffers.svelte";
import { Drafts } from "./drafts.svelte";
import { FakeCore, MemoryStorage } from "./fake-core";
import type { HighlightClient } from "./highlight/client";
import { editorPrefs } from "./prefs.svelte";

// jsdom has no layout; CodeMirror measures text ranges when it draws the selection.
beforeAll(() => {
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
});

const views: EditorView[] = [];
afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
});

function setup(highlighter: HighlightClient | null = null, storage = new MemoryStorage(), autoSave?: AutoSaver) {
  const core = new FakeCore();
  const drafts = new Drafts(storage);
  const buffers = new Buffers({ transport: core, project: "p1", drafts, highlighter: () => highlighter, autoSave });
  const view = new EditorView({ parent: document.body.appendChild(document.createElement("div")) });
  views.push(view);
  /** Opens `path` in the view, as the editor does when its tab is active. */
  const show = async (path: string): Promise<Buffer> => {
    const buffer = buffers.open(path);
    await buffer.loaded;
    buffer.attach(view);
    return buffer;
  };
  const type = (insert: string) => view.dispatch({ changes: { from: view.state.doc.length, insert } });
  return { core, drafts, storage, buffers, view, show, type };
}

describe("Buffer", () => {
  test("loads, tracks changes and saves with the loaded mtime", async () => {
    const { core, show, type } = setup();
    core.put("a.txt", "hello");
    const a = await show("a.txt");
    expect([a.status, a.text, a.dirty]).toEqual(["ready", "hello", false]);
    type(" world");
    expect(a.dirty).toBe(true);
    expect(await a.save()).toBe(true);
    expect(core.writes).toEqual([{ path: "a.txt", text: "hello world", expected: 101 }]);
    expect(a.dirty).toBe(false);
  });

  test("a CRLF file saves with CRLF, after an outside change too", async () => {
    const { core, show, type } = setup();
    core.put("a.txt", "one\r\ntwo");
    const a = await show("a.txt");
    expect([a.text, a.lineEnding]).toEqual(["one\ntwo", "CRLF"]);
    type("\nthree");
    await a.save();
    expect(core.writes.at(-1)?.text).toBe("one\r\ntwo\r\nthree");
    core.put("a.txt", "lf\nonly");
    await a.diskChanged();
    type("!");
    await a.save();
    expect([core.writes.at(-1)?.text, a.lineEnding]).toEqual(["lf\nonly!", "LF"]);
  });

  test("names the text box after its file", async () => {
    const { core, view, show } = setup();
    core.put("src/a.txt", "hello");
    await show("src/a.txt");
    expect(view.contentDOM.getAttribute("aria-label")).toBe("Edit src/a.txt");
  });

  test("Mod+/ comments and uncomments lines with the language's marker", async () => {
    const { core, view, show } = setup();
    core.put("a.ts", "let a = 1;");
    await show("a.ts");
    toggleComment(view);
    expect(view.state.doc.toString()).toBe("// let a = 1;");
    toggleComment(view);
    expect(view.state.doc.toString()).toBe("let a = 1;");
  });

  test("wraps lines while word wrap is on, keeping the text and its undo history", async () => {
    const { core, view, show, type } = setup();
    core.put("a.txt", "hello");
    const a = await show("a.txt");
    type("!");
    const wrapped = () => view.contentDOM.classList.contains("cm-lineWrapping");
    expect(wrapped()).toBe(false);
    editorPrefs.wordWrap = true;
    a.applyPrefs();
    expect([wrapped(), a.text, undoDepth(view.state)]).toEqual([true, "hello!", 1]);
    editorPrefs.wordWrap = false;
    editorPrefs.tabSize = 2;
    a.applyPrefs();
    expect([wrapped(), view.state.tabSize]).toEqual([false, 2]);
    editorPrefs.tabSize = 4;
    a.applyPrefs();
  });

  test("undo history and unsaved text survive switching to another tab and back", async () => {
    const { core, view, show, type } = setup();
    core.put("a.txt", "one");
    core.put("b.txt", "two");
    const a = await show("a.txt");
    type(" more");
    a.detach();
    await show("b.txt");
    expect(view.state.doc.toString()).toBe("two");
    const b = await show("a.txt");
    expect([b, b.text, b.dirty]).toEqual([a, "one more", true]);
    const { undo } = await import("@codemirror/commands");
    undo(view);
    expect([a.text, a.dirty]).toEqual(["one", false]);
  });

  test("an outside change replaces clean text, keeping the cursor, and resets undo", async () => {
    const { core, view, show, type } = setup();
    core.put("a.txt", "v1 long line");
    const a = await show("a.txt");
    type("!");
    await a.save();
    view.dispatch({ selection: { anchor: 5 } });
    core.put("a.txt", "v2 from an agent");
    core.changed("a.txt");
    await vi.waitFor(() => expect(a.text).toBe("v2 from an agent"));
    expect(view.state.selection.main.head).toBe(5);
    const { undo } = await import("@codemirror/commands");
    expect(undo(view)).toBe(false);
    expect(a.dirty).toBe(false);
  });

  test("auto save writes through the normal save and stops at a conflict", async () => {
    const { core, show, type } = setup(null, new MemoryStorage(), new AutoSaver(() => "after_delay"));
    core.put("a.txt", "base");
    const a = await show("a.txt");
    vi.useFakeTimers();
    try {
      type("!");
      await vi.advanceTimersByTimeAsync(AUTO_SAVE_DELAY_MS);
      expect(core.writes).toEqual([{ path: "a.txt", text: "base!", expected: 101 }]);
      core.put("a.txt", "theirs");
      type("?");
      await a.diskChanged();
      expect(a.conflict).toBe(true);
      type("?");
      await vi.advanceTimersByTimeAsync(AUTO_SAVE_DELAY_MS);
      expect([core.writes.length, a.conflict]).toEqual([1, true]);
    } finally {
      vi.useRealTimers();
    }
  });

  test("an outside change to unsaved text is a conflict, in the background too", async () => {
    const { core, show, type } = setup();
    core.put("a.txt", "base");
    core.put("b.txt", "other");
    const a = await show("a.txt");
    type(" mine");
    a.detach();
    await show("b.txt");
    core.put("a.txt", "theirs");
    core.changed("a.txt");
    await vi.waitFor(() => expect(a.conflict).toBe(true));
    expect([a.text, a.dirty]).toEqual(["base mine", true]);

    expect(await a.save()).toBe(false);
    expect(await a.save(true)).toBe(true);
    expect(core.files.get("a.txt")?.body).toEqual({ kind: "text", text: "base mine" });
    expect(a.conflict).toBe(false);
  });

  test("after a reconnect, clean buffers reread and dirty ones learn of outside changes", async () => {
    const { core, show, type } = setup();
    core.put("a.txt", "a1");
    core.put("b.txt", "b1");
    const a = await show("a.txt");
    a.detach();
    const b = await show("b.txt");
    type(" mine");
    core.setStatus("closed");
    core.put("a.txt", "a2");
    core.put("b.txt", "b2");
    core.setStatus("open");
    await vi.waitFor(() => expect([a.text, b.conflict]).toEqual(["a2", true]));
    expect(b.text).toBe("b1 mine");
  });

  test("revert drops edits for the disk version", async () => {
    const { core, show, type } = setup();
    core.put("a.txt", "one");
    const a = await show("a.txt");
    type(" mine");
    core.put("a.txt", "theirs");
    await a.revert();
    expect([a.text, a.dirty, a.conflict]).toEqual(["theirs", false, false]);
  });

  test("a deleted file is reported, and saving it needs an explicit overwrite", async () => {
    const { core, show, type } = setup();
    core.put("a.txt", "one");
    const a = await show("a.txt");
    type(" mine");
    core.files.delete("a.txt");
    core.changed("a.txt");
    await vi.waitFor(() => expect(a.deleted).toBe(true));
    expect(a.conflict).toBe(true);
    expect(await a.save()).toBe(false);
    expect(core.writes.at(-1)?.expected).toBe(101);
    expect(await a.save(true)).toBe(true);
    expect([a.deleted, a.conflict]).toEqual([false, false]);
  });

  test("binary files and read errors are not editable", async () => {
    const { core, buffers } = setup();
    core.files.set("img.png", { body: { kind: "binary", mime: "image/png", base64: "" }, mtime: 1 });
    const image = buffers.open("img.png");
    await image.loaded;
    expect([image.status, image.text]).toEqual(["binary", null]);
    const missing = buffers.open("gone.txt");
    await missing.loaded;
    expect([missing.status, missing.error]).toEqual(["failed", "not_found: gone.txt not found"]);
  });

  test("paints spans from the highlighter and sends edits to it", async () => {
    const edits: unknown[] = [];
    const highlighter = {
      open: vi.fn(() => ({
        edit: (changes: unknown) => edits.push(changes),
        highlight: async () => [[0, 2, "tok-keyword"]] as [number, number, string][],
        close: () => {},
      })),
    } as unknown as HighlightClient;
    const { core, view, show, type } = setup(highlighter);
    core.put("main.rs", "fn main() {}");
    const buffer = await show("main.rs");
    await vi.waitFor(() => expect(view.dom.querySelector(".tok-keyword")?.textContent).toBe("fn"));
    expect(buffer.language?.id).toBe("rust");
    expect(highlighter.open).toHaveBeenCalledWith(expect.objectContaining({ id: "rust" }), "fn main() {}");
    type("\n");
    expect(edits).toEqual([[{ from: 12, to: 12, insert: "\n" }]]);
  });

  test("a minified or very large file opens as plain text, without the highlighter", async () => {
    const highlighter = { open: vi.fn() } as unknown as HighlightClient;
    const { core, show } = setup(highlighter);
    core.put("small.js", "let a = 1;\n");
    core.put("app.min.js", `let a = 1;\n${"x".repeat(10_001)}\n`);
    core.put("big.js", `${"x".repeat(9_999)}\n`.repeat(110));
    const small = await show("small.js");
    expect(small.heavy).toBe(false);
    expect(highlighter.open).toHaveBeenCalledTimes(1);
    for (const path of ["app.min.js", "big.js"]) {
      const buffer = await show(path);
      expect([path, buffer.status, buffer.heavy]).toEqual([path, "ready", true]);
    }
    expect(highlighter.open).toHaveBeenCalledTimes(1);
  });
});

describe("drafts", () => {
  test("unsaved text is stored and comes back in a new page with the tab marked dirty", async () => {
    const storage = new MemoryStorage();
    const first = setup(null, storage);
    first.core.put("a.txt", "base");
    const a = await first.show("a.txt");
    first.type(" draft");
    expect(first.buffers.flushDrafts()).toBe(true);

    // A reload: new page, same storage and disk.
    const second = setup(null, storage);
    second.core.files = first.core.files;
    expect(second.buffers.isDirty("a.txt")).toBe(true);
    const restored = await second.show("a.txt");
    expect([restored.text, restored.dirty, restored.conflict]).toEqual(["base draft", true, false]);
    await restored.save();
    expect(second.drafts.has("p1", "a.txt")).toBe(false);
    expect(a.path).toBe("a.txt");
  });

  test("a draft for a file that changed on disk since is a conflict", async () => {
    const storage = new MemoryStorage();
    const first = setup(null, storage);
    first.core.put("a.txt", "base");
    await first.show("a.txt");
    first.type(" draft");
    first.buffers.flushDrafts();

    const second = setup(null, storage);
    second.core.files = first.core.files;
    second.core.files.set("a.txt", { body: { kind: "text", text: "agent wrote this" }, mtime: 999 });
    const restored = await second.show("a.txt");
    expect([restored.text, restored.dirty, restored.conflict]).toEqual(["base draft", true, true]);
  });

  test("drafts are written shortly after typing and dropped when the text is clean again", async () => {
    vi.useFakeTimers();
    try {
      const { core, drafts, show, type, view } = setup();
      core.put("a.txt", "x");
      const a = await show("a.txt");
      type("y");
      expect(drafts.has("p1", "a.txt")).toBe(false);
      vi.advanceTimersByTime(300);
      expect(drafts.get("p1", "a.txt")).toEqual({ text: "xy", mtime: 101 });
      view.dispatch({ changes: { from: 1, to: 2 } });
      expect([a.dirty, drafts.has("p1", "a.txt")]).toEqual([false, false]);
    } finally {
      vi.useRealTimers();
    }
  });

  test("discarding forgets the buffer and its draft", async () => {
    const { core, drafts, buffers, show, type } = setup();
    core.put("a.txt", "x");
    await show("a.txt");
    type("y");
    buffers.flushDrafts();
    buffers.discard("a.txt");
    expect([buffers.isDirty("a.txt"), drafts.has("p1", "a.txt")]).toEqual([false, false]);
  });

  test("storage that refuses writes reports failure so the page can warn", () => {
    const broken = new MemoryStorage();
    broken.setItem = () => {
      throw new Error("quota");
    };
    expect(new Drafts(broken).set("p1", "a.txt", { text: "x", mtime: 1 })).toBe(false);
    expect(new Drafts(null).set("p1", "a.txt", { text: "x", mtime: 1 })).toBe(false);
  });
});
