import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { registerCommand } from "../keys/commands";
import type { Project, Request } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { mainView } from "../shell/main-view.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { FakeCore } from "../tree/fake-core";
import { type PaletteMode, palette } from "./mode.svelte";
import PaletteHarness from "./PaletteHarness.svelte";

const unused: Transport = {
  call: (() => Promise.reject(new Error("unused"))) as Transport["call"],
  onEvent: () => () => {},
};
const cleanups: (() => void)[] = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
});

async function open(
  mode: PaletteMode = "commands",
  transport: Transport = unused,
  project: Project | null = null,
): Promise<{ workspace: Workspace; target: HTMLElement; input: HTMLInputElement }> {
  const workspace = new Workspace();
  workspace.project = project;
  palette.mode = mode;
  workspace.paletteOpen = true;
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(PaletteHarness, { target, props: { ovra: { transport, workspace } } });
  cleanups.push(() => {
    unmount(app);
    target.remove();
  });
  flushSync();
  await new Promise((resolve) => setImmediate(resolve)); // let the entries load
  flushSync();
  const input = target.querySelector("input");
  if (!input) throw new Error("no input");
  return { workspace, target, input };
}

/** Types and waits for the ranking of the new query. */
async function type(input: HTMLInputElement, text: string): Promise<void> {
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  await settle();
}

async function settle(): Promise<void> {
  await new Promise((resolve) => setImmediate(resolve));
  flushSync();
}

function key(input: HTMLInputElement, name: string): void {
  input.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true }));
  flushSync();
}

const titles = (target: HTMLElement) =>
  [...target.querySelectorAll("[role=option]")].map((li) => li.textContent?.replace(/\s+/g, " ").trim());
const selected = (target: HTMLElement) => target.querySelector("[aria-selected=true]")?.textContent?.trim();

describe("Palette", async () => {
  it("focuses the input, filters, highlights and shows shortcuts", async () => {
    cleanups.push(registerCommand({ id: "t.open", title: "Open file", keys: "Mod+O", run: () => {} }));
    cleanups.push(registerCommand({ id: "t.close", title: "Close editor", run: () => {} }));
    const { target, input } = await open();
    expect(document.activeElement).toBe(input);
    expect(titles(target)).toEqual(["Close editor", "Open file Ctrl+O"]);

    await type(input, "of");
    expect(titles(target)).toEqual(["Open file Ctrl+O"]);
    expect([...target.querySelectorAll("mark")].map((m) => m.textContent)).toEqual(["O", "f"]);

    await type(input, "zzz");
    expect(target.textContent).toContain("No matching commands");
  });

  it("moves the selection with arrows and runs the selection on Enter, closing first", async () => {
    const run = vi.fn();
    cleanups.push(registerCommand({ id: "t.a", title: "Alpha", run: () => {} }));
    cleanups.push(registerCommand({ id: "t.b", title: "Beta", run }));
    const { workspace, target, input } = await open();
    expect(selected(target)).toBe("Alpha");
    key(input, "ArrowDown");
    expect(selected(target)).toBe("Beta");
    key(input, "ArrowDown");
    expect(selected(target)).toBe("Alpha");
    key(input, "ArrowUp");
    key(input, "Enter");
    await vi.waitFor(() => expect(run).toHaveBeenCalledOnce());
    expect(workspace.paletteOpen).toBe(false);
  });

  it("moves with Ctrl+N/P and Ctrl+J/K by physical key, and by a page with PageDown/PageUp", async () => {
    for (const name of "ABCDEFGHIJKL") cleanups.push(registerCommand({ id: `t.${name}`, title: name, run: () => {} }));
    const { target, input } = await open();
    const ctrl = (code: string, key: string) => {
      input.dispatchEvent(new KeyboardEvent("keydown", { key, code, ctrlKey: true, bubbles: true, cancelable: true }));
      flushSync();
    };
    ctrl("KeyN", "n");
    expect(selected(target)).toBe("B");
    ctrl("KeyJ", "\u043e"); // Ukrainian layout
    expect(selected(target)).toBe("C");
    ctrl("KeyP", "p");
    ctrl("KeyK", "k");
    expect(selected(target)).toBe("A");
    key(input, "PageDown");
    expect(selected(target)).toBe("K");
    key(input, "PageDown");
    expect(selected(target)).toBe("L");
    key(input, "PageUp");
    expect(selected(target)).toBe("B");
  });

  it("closes on Escape and when focus leaves it", async () => {
    const first = await open();
    key(first.input, "Escape");
    expect(first.workspace.paletteOpen).toBe(false);

    const second = await open();
    second.input.dispatchEvent(new FocusEvent("focusout", { bubbles: true, relatedTarget: null }));
    flushSync();
    expect(second.workspace.paletteOpen).toBe(false);
  });

  it("runs a command on click", async () => {
    const run = vi.fn();
    cleanups.push(registerCommand({ id: "t.c", title: "Gamma", run }));
    const { target } = await open();
    (target.querySelector("[role=option]") as HTMLElement).click();
    await vi.waitFor(() => expect(run).toHaveBeenCalledOnce());
  });

  it("goes to a file of the open project", async () => {
    const paths = ["README.md", "src/lib.rs", "src/main.rs"];
    const core: Transport = {
      call: ((request: Request) =>
        request.method === "fs.list_files"
          ? Promise.resolve({ type: "fs.files", data: { paths, total: paths.length, version: 1 } })
          : Promise.reject(new Error(request.method))) as Transport["call"],
      onEvent: () => () => {},
    };
    const project = { id: "files-1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
    const { workspace, target, input } = await open("files", core, project);
    expect(input.placeholder).toBe("Go to file in demo");
    await type(input, "srcmain");
    expect(titles(target)).toEqual(["src/main.rs"]);
    key(input, "Enter");
    expect([workspace.paletteOpen, palette.mode]).toEqual([false, "commands"]);
    await vi.waitFor(() => expect(workspace.openPath).toBe("src/main.rs"));
  });

  it("closes a view for a file picked over it, the open one too, and puts focus in the file", async () => {
    const paths = ["README.md"];
    const core: Transport = {
      call: ((request: Request) =>
        request.method === "fs.list_files"
          ? Promise.resolve({ type: "fs.files", data: { paths, total: paths.length, version: 1 } })
          : Promise.reject(new Error(request.method))) as Transport["call"],
      onEvent: () => () => {},
    };
    const panel = document.body.appendChild(document.createElement("div"));
    panel.id = "ovra-tabpanel";
    panel.tabIndex = -1;
    const heading = document.body.appendChild(document.createElement("h2"));
    heading.tabIndex = -1;
    cleanups.push(() => {
      mainView.close(false);
      panel.remove();
      heading.remove();
    });
    heading.focus();
    mainView.show("settings");
    const project = { id: "files-view", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
    const { workspace, input } = await open("files", core, project);
    workspace.openPath = "README.md";
    await type(input, "readme");
    key(input, "Enter");
    await vi.waitFor(() => expect(document.activeElement).toBe(panel));
    expect(mainView.current).toBeNull();
  });

  it("goes back to the start screen's first project when it closes with focus nowhere", async () => {
    const screen = document.body.appendChild(document.createElement("section"));
    screen.dataset.testid = "project-list";
    screen.innerHTML = "<ul><li><button>demo-app</button></li></ul>";
    cleanups.push(() => screen.remove());
    (document.activeElement as HTMLElement | null)?.blur();
    const { input } = await open("search");
    key(input, "Escape");
    await vi.waitFor(() => expect(document.activeElement?.textContent).toBe("demo-app"));
  });

  it("keeps editing and queues an Enter typed before the files load", async () => {
    const paths = ["src/main.rs", "docs/page-1.md", "docs/page-2.md"];
    let answer: (() => void) | undefined;
    const core: Transport = {
      call: (() =>
        new Promise<unknown>((resolve) => {
          answer = () => resolve({ type: "fs.files", data: { paths, total: paths.length, version: 1 } });
        })) as Transport["call"],
      onEvent: () => () => {},
    };
    const project = { id: "files-2", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
    const workspace = new Workspace();
    workspace.project = project;
    workspace.openFile("README.md", "edit");
    palette.mode = "files";
    workspace.paletteOpen = true;
    const target = document.createElement("div");
    document.body.append(target);
    const app = mount(PaletteHarness, { target, props: { ovra: { transport: core, workspace } } });
    cleanups.push(() => {
      unmount(app);
      target.remove();
    });
    flushSync();
    const input = target.querySelector("input") as HTMLInputElement;
    expect(target.textContent).toContain("Loading");

    input.value = "srcmain";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    key(input, "Enter");
    expect(workspace.paletteOpen).toBe(true);
    answer?.();
    await vi.waitFor(() => expect(workspace.openPath).toBe("src/main.rs"));
    expect([workspace.mode, workspace.paletteOpen]).toEqual(["edit", false]);
  });

  it("says how many matches are not shown", async () => {
    const paths = Array.from({ length: 55 }, (_, i) => `docs/page-${i}.md`);
    const core: Transport = {
      call: (() =>
        Promise.resolve({ type: "fs.files", data: { paths, total: paths.length, version: 1 } })) as Transport["call"],
      onEvent: () => () => {},
    };
    const project = { id: "files-3", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
    const { target, input } = await open("files", core, project);
    await type(input, "page");
    expect(target.textContent).toContain("Showing 50 of 55");
    expect(target.querySelector("[role=status]")?.textContent).toBe("55 results");
    await type(input, "page-54");
    expect(target.querySelector("[role=status]")?.textContent).toBe("1 result");
  });

  it("switches to another project", async () => {
    const projects = [
      { id: "a", name: "alpha", root: "/w/alpha", last_opened_ms: null, trusted: false },
      { id: "b", name: "beta", root: "/w/beta", last_opened_ms: null, trusted: false },
    ];
    const core = new FakeCore(undefined, projects);
    const { workspace, target, input } = await open("projects", core, projects[0]);
    workspace.openPath = "a.md";
    expect(titles(target)).toEqual(["alpha /w/alpha", "beta /w/beta", "Open a folder…"]);
    await type(input, "be");
    key(input, "Enter");
    await vi.waitFor(() => expect(workspace.project?.id).toBe("b"));
    expect(workspace.openPath).toBeNull();
  });

  it("ends the project list with Open a folder…, which asks for a path and keeps the project until it opens", async () => {
    const projects = [{ id: "a", name: "alpha", root: "/w/alpha", last_opened_ms: null, trusted: false }];
    const { workspace, target, input } = await open("projects", new FakeCore(undefined, projects), projects[0]);
    await type(input, "open a folder");
    key(input, "Enter");
    await vi.waitFor(() => expect(palette.mode).toBe("folder"));
    await settle();
    const field = target.querySelector("input");
    if (!field) throw new Error("no folder field");
    expect(document.activeElement).toBe(field);
    expect(field.placeholder).toBe("Folder to open, for example /home/you/my-app");
    expect(target.textContent).toContain("alpha stays open until the new folder opens.");

    await type(field, "relative");
    key(field, "Enter");
    await vi.waitFor(() => expect(target.querySelector("[role=status]")?.textContent).toContain("Use the full path"));
    expect(field.getAttribute("aria-invalid")).toBe("true");
    expect([workspace.project?.id, workspace.paletteOpen]).toEqual(["a", true]);

    await type(field, "/w/beta");
    key(field, "Enter");
    await vi.waitFor(() => expect(workspace.project?.root).toBe("/w/beta"));
    expect(workspace.paletteOpen).toBe(false);
  });

  it("searches files and commands, suggests on an empty query and keeps commands after >", async () => {
    localStorage.clear();
    cleanups.push(registerCommand({ id: "inbox.toggle", title: "Review inbox", run: () => {} }));
    cleanups.push(
      registerCommand({
        id: "network.toggle",
        title: "Network",
        category: "Settings",
        keywords: ["proxy"],
        run: () => {},
      }),
    );
    const paths = ["README.md", "net.md"];
    const core: Transport = {
      call: (() =>
        Promise.resolve({ type: "fs.files", data: { paths, total: paths.length, version: 1 } })) as Transport["call"],
      onEvent: () => () => {},
    };
    const project = { id: "search-1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
    const { target, input } = await open("search", core, project);
    expect(input.placeholder).toBe("Search files, or type > for commands");
    const rows = () => [...target.querySelectorAll("li")].map((li) => li.textContent?.replace(/\s+/g, " ").trim());
    expect(rows()).toEqual(["Suggested", "Agents Review inbox"]);

    await type(input, "net");
    expect(rows()).toEqual(["Files", "net.md", "Commands", "Settings Network"]);
    await type(input, ">net");
    expect(rows()).toEqual(["Commands", "Settings Network"]);
    await type(input, "proxy");
    expect(rows()).toEqual(["Commands", "Settings Network"]);
    key(input, "Enter");
    expect(JSON.parse(localStorage.getItem("ovra.palette.recent") ?? "[]")).toEqual(["command:network.toggle"]);
  });

  it("shows why the list could not load", async () => {
    const { target } = await open("projects");
    expect(target.textContent).toContain("unused");
  });
});
