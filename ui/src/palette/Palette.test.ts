import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { registerCommand } from "../keys/commands";
import type { Transport } from "../lib/transport";
import { Workspace } from "../shell/workspace.svelte";
import PaletteHarness from "./PaletteHarness.svelte";

const transport: Transport = {
  call: (() => Promise.reject(new Error("unused"))) as Transport["call"],
  onEvent: () => () => {},
};
const cleanups: (() => void)[] = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
});

function open(): { workspace: Workspace; target: HTMLElement; input: HTMLInputElement } {
  const workspace = new Workspace();
  workspace.paletteOpen = true;
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(PaletteHarness, { target, props: { ovra: { transport, workspace } } });
  cleanups.push(() => {
    unmount(app);
    target.remove();
  });
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

describe("Palette", () => {
  it("focuses the input, filters, highlights and shows shortcuts", () => {
    cleanups.push(registerCommand({ id: "t.open", title: "Open file", keys: "Mod+O", run: () => {} }));
    cleanups.push(registerCommand({ id: "t.close", title: "Close editor", run: () => {} }));
    const { target, input } = open();
    expect(document.activeElement).toBe(input);
    expect(titles(target)).toEqual(["Close editor", "Open file Ctrl+O"]);

    type(input, "of");
    expect(titles(target)).toEqual(["Open file Ctrl+O"]);
    expect([...target.querySelectorAll("mark")].map((m) => m.textContent)).toEqual(["O", "f"]);

    type(input, "zzz");
    expect(target.textContent).toContain("No matching commands");
  });

  it("moves the selection with arrows and runs the selection on Enter, closing first", () => {
    const run = vi.fn();
    cleanups.push(registerCommand({ id: "t.a", title: "Alpha", run: () => {} }));
    cleanups.push(registerCommand({ id: "t.b", title: "Beta", run }));
    const { workspace, target, input } = open();
    expect(selected(target)).toBe("Alpha");
    key(input, "ArrowDown");
    expect(selected(target)).toBe("Beta");
    key(input, "ArrowDown");
    expect(selected(target)).toBe("Alpha");
    key(input, "ArrowUp");
    key(input, "Enter");
    expect(run).toHaveBeenCalledOnce();
    expect(workspace.paletteOpen).toBe(false);
  });

  it("closes on Escape and when focus leaves it", () => {
    const first = open();
    key(first.input, "Escape");
    expect(first.workspace.paletteOpen).toBe(false);

    const second = open();
    second.input.dispatchEvent(new FocusEvent("focusout", { bubbles: true, relatedTarget: null }));
    flushSync();
    expect(second.workspace.paletteOpen).toBe(false);
  });

  it("runs a command on click", () => {
    const run = vi.fn();
    cleanups.push(registerCommand({ id: "t.c", title: "Gamma", run }));
    const { target } = open();
    (target.querySelector("[role=option]") as HTMLElement).click();
    expect(run).toHaveBeenCalledOnce();
  });
});
