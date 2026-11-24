import { EditorView } from "@codemirror/view";
import { afterEach, beforeAll, describe, expect, test, vi } from "vitest";
import type { CoreEvent, FileBody, Request } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import type { HighlightClient } from "./highlight/client";
import { EditorSession } from "./session.svelte";

/** An in-memory core with `fs.read_file` / `fs.write_file` semantics. */
class FakeCore implements Transport {
  files = new Map<string, { body: FileBody; mtime: number }>();
  writes: { path: string; text: string; expected: number | null }[] = [];
  private clock = 100;
  private listeners = new Set<(event: CoreEvent) => void>();

  put(path: string, text: string): void {
    this.files.set(path, { body: { kind: "text", text }, mtime: ++this.clock });
  }

  emit(event: CoreEvent): void {
    for (const listener of this.listeners) listener(event);
  }

  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  // biome-ignore lint/suspicious/noExplicitAny: test double for a generic method
  async call(request: Request): Promise<any> {
    if (request.method === "fs.read_file") {
      const file = this.files.get(request.params.path);
      if (!file) throw new CoreError({ code: "not_found", message: `${request.params.path} not found` });
      return {
        type: "fs.file",
        data: { path: request.params.path, size: 0, mtime_ms: file.mtime, body: file.body },
      };
    }
    if (request.method === "fs.write_file") {
      const { path, text, expected_mtime_ms: expected } = request.params;
      this.writes.push({ path, text, expected });
      const file = this.files.get(path);
      if (expected !== null && file && file.mtime !== expected) {
        throw new CoreError({ code: "conflict", message: `${path} changed on disk` });
      }
      this.put(path, text);
      return { type: "fs.written", data: { mtime_ms: this.clock } };
    }
    throw new Error(`unexpected ${request.method}`);
  }
}

// jsdom has no layout; CodeMirror measures text ranges when it draws the selection.
beforeAll(() => {
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
});

const sessions: EditorSession[] = [];
afterEach(() => {
  for (const session of sessions.splice(0)) session.destroy();
  document.body.innerHTML = "";
});

async function open(core: FakeCore, path: string, highlighter: HighlightClient | null = null) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const session = new EditorSession({ transport: core, project: "p1", path, parent, highlighter: () => highlighter });
  sessions.push(session);
  await session.load();
  const dom = parent.querySelector<HTMLElement>(".cm-editor");
  const view = dom ? EditorView.findFromDOM(dom) : null;
  const type = (insert: string) => view?.dispatch({ changes: { from: view.state.doc.length, insert } });
  return { session, parent, view, type };
}

/** Runs pending promise callbacks; the fake core answers at once. */
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("EditorSession", () => {
  test("loads text, tracks changes and saves with the loaded mtime", async () => {
    const core = new FakeCore();
    core.put("src/a.txt", "hello");
    const { session, type } = await open(core, "src/a.txt");
    expect(session.status).toBe("ready");
    expect(session.text).toBe("hello");
    expect(session.dirty).toBe(false);

    type(" world");
    expect(session.dirty).toBe(true);
    await session.save();
    expect(core.writes).toEqual([{ path: "src/a.txt", text: "hello world", expected: 101 }]);
    expect(session.dirty).toBe(false);

    type("!");
    await session.save();
    expect(core.writes[1]).toEqual({ path: "src/a.txt", text: "hello world!", expected: 102 });
  });

  test("a file changed on disk is a conflict: overwrite or reload", async () => {
    const core = new FakeCore();
    core.put("a.txt", "one");
    const { session, type } = await open(core, "a.txt");
    core.put("a.txt", "theirs");
    type(" mine");

    await session.save();
    expect(session.conflict).toBe(true);
    expect(core.files.get("a.txt")?.body).toEqual({ kind: "text", text: "theirs" });

    await session.save(true);
    expect(session.conflict).toBe(false);
    expect(core.writes.at(-1)).toEqual({ path: "a.txt", text: "one mine", expected: null });

    core.put("a.txt", "theirs again");
    await session.reload();
    expect(session.text).toBe("theirs again");
    expect(session.dirty).toBe(false);
  });

  test("fs.changed refreshes a clean file and leaves unsaved changes alone", async () => {
    const core = new FakeCore();
    core.put("a.txt", "v1");
    const { session, type } = await open(core, "a.txt");
    const changed: CoreEvent = { type: "fs.changed", data: { project: "p1", paths: ["a.txt"] } };

    core.put("a.txt", "v2");
    core.emit(changed);
    await vi.waitFor(() => expect(session.text).toBe("v2"));

    type(" local");
    core.put("a.txt", "v3");
    core.emit(changed);
    await flush();
    expect(session.text).toBe("v2 local");
  });

  test("unsaved text survives switching files unless the file changed", async () => {
    const core = new FakeCore();
    core.put("a.txt", "base");
    const first = await open(core, "a.txt");
    first.type(" draft");
    first.session.destroy();

    const second = await open(core, "a.txt");
    expect(second.session.text).toBe("base draft");
    expect(second.session.dirty).toBe(true);
    second.session.destroy();

    core.put("a.txt", "new on disk");
    const third = await open(core, "a.txt");
    expect(third.session.text).toBe("new on disk");
    expect(third.session.dirty).toBe(false);
  });

  test("binary files and read errors are not editable", async () => {
    const core = new FakeCore();
    core.files.set("img.png", { body: { kind: "binary", mime: "image/png", base64: "" }, mtime: 1 });
    const binary = await open(core, "img.png");
    expect(binary.session.status).toBe("binary");
    expect(binary.view).toBeNull();

    const missing = await open(core, "gone.txt");
    expect(missing.session.status).toBe("failed");
    expect(missing.session.error).toBe("not_found: gone.txt not found");
  });

  test("paints spans from the highlighter and sends edits to it", async () => {
    const core = new FakeCore();
    core.put("main.rs", "fn main() {}");
    const edits: unknown[] = [];
    const highlighter = {
      open: vi.fn(() => ({
        edit: (changes: unknown) => edits.push(changes),
        highlight: async () => [[0, 2, "tok-keyword"]] as [number, number, string][],
        close: () => {},
      })),
    } as unknown as HighlightClient;
    const { session, parent, type } = await open(core, "main.rs", highlighter);
    await vi.waitFor(() => expect(parent.querySelector(".tok-keyword")).not.toBeNull());

    expect(session.language?.id).toBe("rust");
    expect(highlighter.open).toHaveBeenCalledWith(expect.objectContaining({ id: "rust" }), "fn main() {}");
    expect(parent.querySelector(".tok-keyword")?.textContent).toBe("fn");
    type("\n");
    expect(edits).toEqual([[{ from: 12, to: 12, insert: "\n" }]]);
  });
});
