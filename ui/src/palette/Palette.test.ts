import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { registerCommand } from "../keys/commands";
import type { Project } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { Workspace } from "../shell/workspace.svelte";
import { dirsFromPaths, FakeCore } from "../tree/fake-core";
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

function type(input: HTMLInputElement, text: string): void {
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
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

    type(input, "of");
    expect(titles(target)).toEqual(["Open file Ctrl+O"]);
    expect([...target.querySelectorAll("mark")].map((m) => m.textContent)).toEqual(["O", "f"]);

    type(input, "zzz");
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
    const core = new FakeCore(dirsFromPaths(["README.md", "src/main.rs", "src/lib.rs"]));
    const project = { id: "files-1", name: "demo", root: "/demo", last_opened_ms: null };
    const { workspace, target, input } = await open("files", core, project);
    expect(input.placeholder).toBe("Go to file in demo");
    type(input, "srcmain");
    expect(titles(target)).toEqual(["src/main.rs"]);
    key(input, "Enter");
    expect([workspace.paletteOpen, palette.mode]).toEqual([false, "commands"]);
    await vi.waitFor(() => expect(workspace.openPath).toBe("src/main.rs"));
  });

  it("switches to another project", async () => {
    const projects = [
      { id: "a", name: "alpha", root: "/w/alpha", last_opened_ms: null },
      { id: "b", name: "beta", root: "/w/beta", last_opened_ms: null },
    ];
    const core = new FakeCore(undefined, projects);
    const { workspace, target, input } = await open("projects", core, projects[0]);
    workspace.openPath = "a.md";
    expect(titles(target)).toEqual(["alpha /w/alpha", "beta /w/beta"]);
    type(input, "be");
    key(input, "Enter");
    await vi.waitFor(() => expect(workspace.project?.id).toBe("b"));
    expect(workspace.openPath).toBeNull();
  });

  it("shows why the list could not load", async () => {
    const { target } = await open("projects");
    expect(target.textContent).toContain("unused");
  });
});
