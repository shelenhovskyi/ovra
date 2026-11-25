import { forEachDiagnostic } from "@codemirror/lint";
import { jumpToDefinition, LSPPlugin, renameSymbol } from "@codemirror/lsp-client";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { languageServers } from "../../lsp/servers.svelte";
import { type Buffer, Buffers } from "../buffers.svelte";
import { Drafts } from "../drafts.svelte";
import { MemoryStorage } from "../fake-core";
import { LanguageClients } from "./clients";
import { FakeLspCore } from "./fake-server";

// jsdom has no layout; CodeMirror measures text ranges when it draws the selection.
beforeAll(() => {
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
});

// Queries, styles and the client load on first use: pay for that once, outside the tests' time.
beforeAll(async () => {
  const { show } = setup();
  for (const path of ["src/a b.ts", "src/c.tsx", "main.rs", "notes.txt"]) await show(path);
  for (const cleanup of cleanups.splice(0)) cleanup();
});

const cleanups: (() => void)[] = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
});

const A = "ovra:///src/a%20b.ts";

function setup(core = new FakeLspCore()) {
  core.put("src/a b.ts", "const x = 1;\nx;\n");
  core.put("src/b.ts", "export const y = 2;\n  y;\n");
  core.put("src/c.tsx", "<p/>;\n");
  core.put("main.rs", "fn main() {}\n");
  core.put("notes.txt", "hello\n");
  const buffers = new Buffers({
    transport: core,
    project: "p1",
    drafts: new Drafts(new MemoryStorage()),
    highlighter: () => null,
  });
  const view = new EditorView({ parent: document.body.appendChild(document.createElement("div")) });
  const notes: string[] = [];
  let current: Buffer | null = null;
  /** Shows `path` as Editor.svelte does: detach, attach, then tell the clients. */
  const show = async (path: string): Promise<Buffer> => {
    if (current) {
      clients.hiding(current);
      current.detach();
      view.setState(EditorState.create());
    }
    const buffer = buffers.open(path);
    current = buffer;
    await buffer.loaded;
    buffer.attach(view);
    clients.showing(buffer, view);
    return buffer;
  };
  const clients = new LanguageClients({
    transport: core,
    project: "p1",
    name: "p",
    open: (path) => void show(path),
    note: (text) => notes.push(text),
  });
  cleanups.push(() => {
    clients.close();
    buffers.close();
    view.destroy();
  });
  return { core, view, notes, show };
}

const until = (check: () => void) => vi.waitFor(check, { interval: 1, timeout: 500 });
/** The `lsp.*` calls other than `lsp.send`. */
const methods = (core: FakeLspCore) => core.lspCalls.map((c) => c.method).filter((m) => m !== "lsp.send");

/** Lets pending promises run, for checks that something did not happen. */
async function settle(): Promise<void> {
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

test("starts a server only for a trusted project with the server installed", async () => {
  const { core, show } = setup();
  core.trusted = false;
  await show("src/a b.ts");
  await until(() => expect(methods(core)).toEqual(["lsp.status"]));
  await show("notes.txt");
  core.trusted = true;
  await show("main.rs"); // rust-analyzer is known but not installed
  await settle();
  expect(methods(core)).toEqual(["lsp.status", "lsp.status"]);
  await show("src/c.tsx");
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1));
  expect(core.lspCalls.find((c) => c.method === "lsp.start")?.params).toEqual({
    project: "p1",
    language: "typescript",
  });
  const [init] = core.got("s1", "initialize");
  expect(init?.params).toMatchObject({ rootUri: "ovra:///", workspaceFolders: [{ uri: "ovra:///", name: "p" }] });
  expect(core.got("s1", "textDocument/didOpen")[0]?.params).toMatchObject({
    textDocument: { uri: "ovra:///src/c.tsx", languageId: "typescriptreact", text: "<p/>;\n" },
  });
});

test("a server that is known but missing or not allowed asks the footer for a hint", async () => {
  const { core, show } = setup();
  const status = languageServers(core);
  core.trusted = true;
  await show("main.rs"); // rust-analyzer is known but not installed
  await until(() => expect(status.needed).toContain("rust"));
  await show("notes.txt"); // no server is known for plain text
  await settle();
  expect(status.needed).not.toContain("text");
});

test("diagnostics from the server reach the view", async () => {
  const { core, view, show } = setup();
  await show("src/a b.ts");
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1));
  expect(core.got("s1", "textDocument/didOpen")[0]?.params).toMatchObject({ textDocument: { uri: A } });
  const range = { start: { line: 1, character: 0 }, end: { line: 1, character: 1 } };
  core.push("s1", {
    method: "textDocument/publishDiagnostics",
    params: { uri: A, version: 0, diagnostics: [{ range, severity: 1, message: "x is unused" }] },
  });
  const found: [number, string][] = [];
  forEachDiagnostic(view.state, (d, from) => found.push([from, d.message]));
  expect(found).toEqual([[13, "x is unused"]]);
});

test("go to definition in another project file opens it at the definition", async () => {
  const { core, view, show } = setup();
  core.answers["textDocument/definition"] = {
    uri: "ovra:///src/b.ts",
    range: { start: { line: 1, character: 2 }, end: { line: 1, character: 3 } },
  };
  await show("src/a b.ts");
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1));
  jumpToDefinition(view);
  await until(() => expect(view.state.selection.main.head).toBe(22));
  expect(view.state.doc.toString()).toBe("export const y = 2;\n  y;\n");
  expect(core.got("s1", "textDocument/didClose")[0]?.params).toEqual({ textDocument: { uri: A } });
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(2));
});

test("a definition outside the project is never opened, only noted", async () => {
  const { core, view, notes, show } = setup();
  core.answers["textDocument/definition"] = {
    uri: "file:///usr/lib/rustlib/src/lib.rs",
    range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
  };
  const read = vi.spyOn(core, "call");
  await show("src/a b.ts");
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1));
  jumpToDefinition(view);
  await until(() =>
    expect(notes).toEqual(["/usr/lib/rustlib/src/lib.rs is outside the project and cannot be opened."]),
  );
  expect(read.mock.calls.filter(([r]) => r.method === "fs.read_file")).toHaveLength(1);
  expect(view.state.doc.toString()).toBe("const x = 1;\nx;\n");
});

test("a stopped server drops its client; the next file shown starts a new one", async () => {
  const { core, view, show } = setup();
  await show("src/a b.ts");
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1));
  const range = { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } };
  core.push("s1", {
    method: "textDocument/publishDiagnostics",
    params: { uri: A, diagnostics: [{ range, message: "m" }] },
  });
  core.stopServer("s1");
  expect(LSPPlugin.get(view)).toBeNull();
  let count = 0;
  forEachDiagnostic(view.state, () => count++);
  expect(count).toBe(0);
  await show("src/b.ts");
  await until(() => expect(core.got("s2", "textDocument/didOpen")).toHaveLength(1));
  expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1);
});

test("a server left running by an earlier page is started afresh, since it was initialized already", async () => {
  const core = new FakeLspCore();
  await core.call({ method: "lsp.start", params: { project: "p1", language: "typescript" } });
  core.lspCalls = [];
  const { show } = setup(core);
  await show("src/a b.ts");
  await until(() => expect(core.got("s2", "textDocument/didOpen")).toHaveLength(1));
  expect(methods(core)).toEqual(["lsp.status", "lsp.start", "lsp.stop", "lsp.start"]);
  expect(core.received.get("s1")).toBeUndefined();
});

test("after a resync the server is stopped and started again for the shown file", async () => {
  const { core, show } = setup();
  await show("src/a b.ts");
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1));
  core.emit({ type: "info.resync", data: { skipped: 3 } });
  await until(() => expect(core.got("s2", "textDocument/didOpen")).toHaveLength(1));
  expect(methods(core)).toEqual(["lsp.status", "lsp.start", "lsp.stop", "lsp.status", "lsp.start"]);
  expect(core.got("s2", "initialize")).toHaveLength(1);
});

test("a rename that would change files that are not open is not applied", async () => {
  const { core, view, notes, show } = setup();
  const edit = (line: number) => ({
    range: { start: { line, character: 0 }, end: { line, character: 1 } },
    newText: "z",
  });
  core.answers["textDocument/rename"] = { changes: { [A]: [edit(1)], "ovra:///src/b.ts": [edit(0)] } };
  await show("src/a b.ts");
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1));
  const rename = (name: string) => {
    view.dispatch({ selection: { anchor: 13 } });
    renameSymbol(view);
    const form = view.dom.querySelector<HTMLFormElement>(".cm-lsp-rename-panel form");
    const input = form?.querySelector<HTMLInputElement>("input[name=name]");
    if (!form || !input) throw new Error("no rename form");
    input.value = name;
    form.requestSubmit();
  };
  rename("z");
  await until(() => expect(notes).toEqual([expect.stringContaining("would also change src/b.ts")]));
  expect(view.state.doc.toString()).toBe("const x = 1;\nx;\n");
  core.answers["textDocument/rename"] = { changes: { [A]: [edit(1)] } };
  rename("z");
  await until(() => expect(view.state.doc.toString()).toBe("const x = 1;\nz;\n"));
});

test("references list files the editor does not show, read from disk, and open them on a pick", async () => {
  const { core, view, show } = setup();
  const at = (uri: string, line: number) => ({
    uri,
    range: { start: { line, character: 0 }, end: { line, character: 1 } },
  });
  const B = "ovra:///src/b.ts";
  core.answers["textDocument/references"] = [at(A, 1), at(B, 1), at("file:///etc/passwd", 0)];
  await show("src/a b.ts");
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1));
  view.dispatch({ selection: { anchor: 13 } });
  view.contentDOM.dispatchEvent(new KeyboardEvent("keydown", { key: "F12", code: "F12", shiftKey: true }));
  const panel = () => view.dom.querySelector(".cm-lsp-reference-panel");
  await until(() => expect(panel()?.textContent).toContain("y;"));
  expect(panel()?.textContent).toContain("x;");
  // Read for the list only: the server was never told b.ts was opened.
  expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1);
  const entries = [...(panel()?.querySelectorAll("[role=option]") ?? [])];
  expect(entries).toHaveLength(2);
  entries[1]?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await until(() => expect(view.state.doc.toString()).toBe("export const y = 2;\n  y;\n"));
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(2));
  expect(core.got("s1", "textDocument/didOpen")[1]?.params).toMatchObject({ textDocument: { uri: B } });
});

test("actions the server cannot do say so, and failed requests become a note", async () => {
  const { core, view, notes, show } = setup();
  await show("src/a b.ts");
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1));
  const key = (init: KeyboardEventInit) => view.contentDOM.dispatchEvent(new KeyboardEvent("keydown", init));
  await until(() => expect(LSPPlugin.get(view)?.client.serverCapabilities).toBeTruthy());
  key({ key: "f", code: "KeyF", keyCode: 70, shiftKey: true, altKey: true });
  expect(notes).toEqual(["fake cannot format this file."]);
  expect(core.got("s1", "textDocument/formatting")).toHaveLength(0);
  LSPPlugin.get(view)?.reportError("Rename request failed", new Error("boom"));
  expect(notes.at(-1)).toBe("Rename request failed: boom");
  expect(view.dom.querySelector(".cm-lsp-message")).toBeNull();
});

test("hover and problems are reachable from the keyboard", async () => {
  const { core, view, notes, show } = setup();
  core.answers["textDocument/hover"] = { contents: { kind: "plaintext", value: "const x: 1" } };
  await show("src/a b.ts");
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1));
  view.dispatch({ selection: { anchor: 6 } });
  view.contentDOM.dispatchEvent(new KeyboardEvent("keydown", { key: "k", code: "KeyK", ctrlKey: true }));
  view.contentDOM.dispatchEvent(new KeyboardEvent("keydown", { key: "i", code: "KeyI", ctrlKey: true }));
  await until(() => expect(view.dom.querySelector(".cm-tooltip")?.textContent).toContain("const x: 1"));
  view.contentDOM.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape" }));
  expect(view.dom.querySelector(".cm-lsp-hover-tooltip")).toBeNull();
  const range = { start: { line: 1, character: 0 }, end: { line: 1, character: 1 } };
  core.push("s1", {
    method: "textDocument/publishDiagnostics",
    params: { uri: A, diagnostics: [{ range, message: "m" }] },
  });
  view.dispatch({ selection: { anchor: 0 } });
  view.contentDOM.dispatchEvent(new KeyboardEvent("keydown", { key: "F8", code: "F8" }));
  expect(view.state.selection.main.from).toBe(13);
  expect(notes).toEqual([]);
});

test("an untrusted project says once why its files get no language server", async () => {
  const core = new FakeLspCore();
  core.trusted = false;
  const { notes, show } = setup(core);
  await show("src/a b.ts");
  await until(() => expect(notes).toEqual([expect.stringContaining("only once you trust the project")]));
  await show("src/b.ts");
  await until(() => expect(methods(core)).toEqual(["lsp.status", "lsp.status"]));
  expect(notes).toHaveLength(1);
});

test("links in docs open through the core, never in this window", async () => {
  const { core, view, show } = setup();
  await show("src/a b.ts");
  await until(() => expect(LSPPlugin.get(view)).not.toBeNull());
  const tooltip = view.dom.appendChild(document.createElement("div"));
  tooltip.innerHTML = '<a href="https://docs.rs/x">docs</a><a>no target</a>';
  const clicks = [...tooltip.querySelectorAll("a")].map((a) =>
    a.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true })),
  );
  expect(clicks).toEqual([false, false]); // both default actions prevented
  expect(core.opened).toEqual(["https://docs.rs/x"]);
});
