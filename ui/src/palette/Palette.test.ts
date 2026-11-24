import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { registerCommand } from "../keys/commands";
import type { Project, Request } from "../lib/protocol";
import type { Transport } from "../lib/transport";
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
  await new Promise((resolve) => setTimeout(resolve)); // let the entries load
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
  await new Promise((resolve) => setTimeout(resolve));
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

  it("keeps editing and queues an Enter typed before the files load", async () => {
    const paths = ["src/main.rs", ...Array.from({ length: 150 }, (_, i) => `docs/page-${i}.md`)];
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
    const paths = Array.from({ length: 150 }, (_, i) => `docs/page-${i}.md`);
    const core: Transport = {
      call: (() =>
        Promise.resolve({ type: "fs.files", data: { paths, total: paths.length, version: 1 } })) as Transport["call"],
      onEvent: () => () => {},
    };
    const project = { id: "files-3", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
    const { target, input } = await open("files", core, project);
    await type(input, "page");
    expect(target.textContent).toContain("Showing 100 of 150");
    expect(target.querySelector("[role=status]")?.textContent).toBe("150 results");
    await type(input, "page-149");
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
    expect(titles(target)).toEqual(["alpha /w/alpha", "beta /w/beta"]);
    await type(input, "be");
    key(input, "Enter");
    await vi.waitFor(() => expect(workspace.project?.id).toBe("b"));
    expect(workspace.openPath).toBeNull();
  });

  it("shows why the list could not load", async () => {
    const { target } = await open("projects");
    expect(target.textContent).toContain("unused");
  });
});
