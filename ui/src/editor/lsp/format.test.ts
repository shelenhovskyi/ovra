import { LSPPlugin } from "@codemirror/lsp-client";
import { EditorView } from "@codemirror/view";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { Buffers } from "../buffers.svelte";
import { Drafts } from "../drafts.svelte";
import { MemoryStorage } from "../fake-core";
import { editorPrefs } from "../prefs.svelte";
import { LanguageClients } from "./clients";
import { FakeLspCore } from "./fake-server";
import { FORMAT_ON_SAVE_TIMEOUT_MS } from "./format";

beforeAll(() => {
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
});

const cleanups: (() => void)[] = [];
afterEach(() => {
  vi.useRealTimers();
  editorPrefs.formatOnSave = false;
  for (const cleanup of cleanups.splice(0)) cleanup();
});

const until = (check: () => void) => vi.waitFor(check, { interval: 1, timeout: 500 });

/** Shows an edited `a.ts` with a server that can format; returns the buffer and what the disk holds. */
async function edited(configure: (core: FakeLspCore) => void = () => {}) {
  const core = new FakeLspCore();
  core.capabilities = { documentFormattingProvider: true };
  core.answers["textDocument/formatting"] = [
    { range: { start: { line: 0, character: 0 }, end: { line: 0, character: 9 } }, newText: "let x = 1;" },
  ];
  configure(core);
  core.put("a.ts", "let  x=1;\n");
  const buffers = new Buffers({
    transport: core,
    project: "p1",
    drafts: new Drafts(new MemoryStorage()),
    highlighter: () => null,
  });
  const view = new EditorView({ parent: document.body.appendChild(document.createElement("div")) });
  const clients = new LanguageClients({
    transport: core,
    project: "p1",
    name: "p",
    open: () => {},
    note: () => {},
    buffers,
    keep: () => {},
  });
  cleanups.push(() => {
    clients.close();
    buffers.close();
    view.destroy();
  });
  const buffer = buffers.open("a.ts");
  await buffer.loaded;
  buffer.attach(view);
  clients.showing(buffer, view);
  await until(() => expect(LSPPlugin.get(view)?.client.serverCapabilities).toBeTruthy());
  buffer.edit({ changes: { from: view.state.doc.length, insert: "\n" } });
  const disk = () => {
    const file = core.files.get("a.ts")?.body;
    return file?.kind === "text" ? file.text : null;
  };
  return { core, buffer, disk };
}

test("with format on save off, saving does not ask the server to format", async () => {
  const { core, buffer, disk } = await edited();
  expect(await buffer.save()).toBe(true);
  expect(disk()).toBe("let  x=1;\n\n");
  expect(core.got("s1", "textDocument/formatting")).toHaveLength(0);
});

test("with format on save on, the server's formatting is saved", async () => {
  editorPrefs.formatOnSave = true;
  const { buffer, disk } = await edited();
  expect(await buffer.save()).toBe(true);
  expect(disk()).toBe("let x = 1;\n\n");
  expect(buffer.dirty).toBe(false);
});

test("a server error or a server that cannot format saves the text unformatted", async () => {
  editorPrefs.formatOnSave = true;
  const failing = await edited((core) => {
    core.failures["textDocument/formatting"] = "boom";
  });
  expect(await failing.buffer.save()).toBe(true);
  expect(failing.disk()).toBe("let  x=1;\n\n");

  const unable = await edited((core) => {
    core.capabilities = {};
  });
  expect(await unable.buffer.save()).toBe(true);
  expect(unable.disk()).toBe("let  x=1;\n\n");
  expect(unable.core.got("s1", "textDocument/formatting")).toHaveLength(0);
});

test("a server that does not answer in time does not hold the save", async () => {
  editorPrefs.formatOnSave = true;
  const { core, buffer, disk } = await edited((c) => c.unanswered.add("textDocument/formatting"));
  vi.useFakeTimers();
  const saved = buffer.save();
  await vi.advanceTimersByTimeAsync(FORMAT_ON_SAVE_TIMEOUT_MS);
  expect(await saved).toBe(true);
  expect(core.got("s1", "textDocument/formatting")).toHaveLength(1);
  expect(disk()).toBe("let  x=1;\n\n");
});
