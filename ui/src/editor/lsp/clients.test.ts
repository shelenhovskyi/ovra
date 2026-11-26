import { forEachDiagnostic } from "@codemirror/lint";
import { jumpToDefinition, LSPPlugin, renameSymbol } from "@codemirror/lsp-client";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { languageServers } from "../../lsp/servers.svelte";
import { closeContextMenu, contextMenu } from "../../menu/context-menu.svelte";
import { type Buffer, Buffers } from "../buffers.svelte";
import { Drafts } from "../drafts.svelte";
import { MemoryStorage } from "../fake-core";
import { LanguageClients } from "./clients";
import { FakeLspCore } from "./fake-server";
import { problems } from "./problems.svelte";

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
  const kept: string[] = [];
  const clients = new LanguageClients({
    transport: core,
    project: "p1",
    name: "p",
    open: (path) => void show(path),
    note: (text) => notes.push(text),
    buffers,
    keep: (path) => kept.push(path),
  });
  cleanups.push(() => {
    clients.close();
    buffers.close();
    view.destroy();
  });
  return { core, view, notes, show, buffers, kept };
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

test("a rename that changes files that are not open edits their buffers, unsaved, or no file at all", async () => {
  const { core, view, notes, show, buffers, kept } = setup();
  const edit = (line: number, character = 0) => ({
    range: { start: { line, character }, end: { line, character: character + 1 } },
    newText: "z",
  });
  const B = "ovra:///src/b.ts";
  await show("src/a b.ts");
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1));
  const rename = () => {
    view.dispatch({ selection: { anchor: 13 } });
    renameSymbol(view);
    const form = view.dom.querySelector<HTMLFormElement>(".cm-lsp-rename-panel form");
    const input = form?.querySelector<HTMLInputElement>("input[name=name]");
    if (!form || !input) throw new Error("no rename form");
    input.value = "z";
    form.requestSubmit();
  };
  // An edit past the end of b.ts: nothing changes anywhere.
  core.answers["textDocument/rename"] = { changes: { [A]: [edit(1)], [B]: [edit(9)] } };
  rename();
  await until(() => expect(notes).toEqual(["Rename not applied: the server's edits do not fit src/b.ts."]));
  expect(view.state.doc.toString()).toBe("const x = 1;\nx;\n");
  expect(buffers.get("src/b.ts")).toBeUndefined();
  core.answers["textDocument/rename"] = { changes: { [A]: [edit(1)], [B]: [edit(1, 2)], "file:///etc/x": [] } };
  rename();
  await until(() => expect(view.state.doc.toString()).toBe("const x = 1;\nz;\n"));
  const b = buffers.get("src/b.ts");
  expect(b?.text).toBe("export const y = 2;\n  z;\n");
  expect(b?.dirty).toBe(true);
  expect(kept).toEqual(["src/b.ts"]);
  expect(notes.at(-1)).toBe(
    "Renamed in 2 files, none saved yet: this file, src/b.ts. Review each tab, then save, or undo there (Mod+Z).",
  );
  // Unsaved text the server never saw: refused.
  rename();
  await until(() =>
    expect(notes.at(-1)).toBe(
      "Rename not applied: save or revert src/b.ts first; the server only sees saved text there.",
    ),
  );
  expect(b?.text).toBe("export const y = 2;\n  z;\n");
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

test("the problems list keeps every file's diagnostics until their server stops", async () => {
  const { core, show } = setup();
  await show("src/a b.ts");
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1));
  const range = { start: { line: 1, character: 2 }, end: { line: 1, character: 3 } };
  const publish = (uri: string) =>
    core.push("s1", {
      method: "textDocument/publishDiagnostics",
      params: { uri, diagnostics: [{ range, severity: 2, message: "unused" }] },
    });
  publish("ovra:///src/b.ts"); // not shown in the editor
  publish("file:///etc/passwd"); // outside the project: never listed
  expect(problems.files("p1")).toEqual([
    {
      path: "src/b.ts",
      problems: [
        { severity: "warning", message: "unused", source: null, line: 2, column: 3, spot: { line: 2, from: 2, to: 3 } },
      ],
    },
  ]);
  core.stopServer("s1");
  expect(problems.files("p1")).toEqual([]);
});

test("an untrusted project says once why its files get no language server", async () => {
  const core = new FakeLspCore();
  core.trusted = false;
  const { notes, show } = setup(core);
  await show("src/a b.ts");
  await until(() => expect(notes).toEqual([expect.stringContaining("only once you trust the project.")]));

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

test("quick fixes at the cursor land as unsaved edits; command-only fixes are not offered", async () => {
  const { core, view, notes, show, buffers, kept } = setup();
  const B = "ovra:///src/b.ts";
  const edit = (line: number, character: number) => ({
    range: { start: { line, character }, end: { line, character: character + 1 } },
    newText: "z",
  });
  core.capabilities = { codeActionProvider: { resolveProvider: true } };
  core.answers["textDocument/codeAction"] = [
    { title: "Run a server command", command: { title: "x", command: "server.fix" } },
    { title: "Rename x everywhere", kind: "refactor", data: { id: 7 } },
    { title: "Disabled one", disabled: { reason: "no" }, edit: { changes: {} } },
    { title: "Use z here", kind: "quickfix", isPreferred: true, edit: { changes: { [A]: [edit(1, 0)] } } },
  ];
  core.answers["codeAction/resolve"] = {
    title: "Rename x everywhere",
    edit: { changes: { [A]: [edit(1, 0)], [B]: [edit(1, 2)] } },
  };
  await show("src/a b.ts");
  await until(() => expect(core.got("s1", "textDocument/didOpen")).toHaveLength(1));
  await until(() => expect(LSPPlugin.get(view)?.client.serverCapabilities).toBeTruthy());
  const range = { start: { line: 1, character: 0 }, end: { line: 1, character: 1 } };
  const diagnostic = { range, severity: 1, message: "x is unused" };
  const elsewhere = { range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } }, message: "far" };
  core.push("s1", {
    method: "textDocument/publishDiagnostics",
    params: { uri: A, diagnostics: [diagnostic, elsewhere] },
  });
  view.dispatch({ selection: { anchor: 13 } });
  const fix = () =>
    view.contentDOM.dispatchEvent(new KeyboardEvent("keydown", { key: ".", code: "Period", ctrlKey: true }));
  fix();
  await until(() => expect(contextMenu.open).not.toBeNull());
  expect(core.got("s1", "textDocument/codeAction")[0]?.params).toMatchObject({
    textDocument: { uri: A },
    context: { diagnostics: [diagnostic] },
  });
  const items = () => (contextMenu.open?.entries ?? []).filter((e) => e !== "separator");
  expect(items().map((e) => e.label)).toEqual(["Use z here", "Rename x everywhere"]);
  await items()[0]?.run();
  closeContextMenu(false);
  await until(() => expect(view.state.doc.toString()).toBe("const x = 1;\nz;\n"));
  // A fix the server fills in on resolve, across files.
  view.dispatch({ changes: { from: 13, to: 14, insert: "x" }, selection: { anchor: 13 } });
  fix();
  await until(() => expect(items()).toHaveLength(2));
  await items()[1]?.run();
  closeContextMenu(false);
  await until(() => expect(buffers.get("src/b.ts")?.text).toBe("export const y = 2;\n  z;\n"));
  expect(core.got("s1", "codeAction/resolve")[0]?.params).toMatchObject({ data: { id: 7 } });
  expect(view.state.doc.toString()).toBe("const x = 1;\nz;\n");
  expect(kept).toEqual(["src/b.ts"]);
  expect(notes.at(-1)).toBe(
    "Changed 2 files, none saved yet: this file, src/b.ts. Review each tab, then save, or undo there (Mod+Z).",
  );
  core.answers["textDocument/codeAction"] = [];
  fix();
  await until(() => expect(notes.at(-1)).toBe("No quick fixes here."));
});
