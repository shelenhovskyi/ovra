import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { listCommands } from "../keys/commands";
import type { Project } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { Workspace } from "../shell/workspace.svelte";
import { FakeTerminals } from "./fake-terminals";
import TerminalsHarness from "./TerminalsHarness.svelte";

// xterm.js needs a real layout engine; this stand-in records what the view does with it.
const xterms = vi.hoisted(() => [] as FakeXterm[]);
type FakeXterm = {
  written: string;
  type: (data: string) => void;
  keyHandler: (event: KeyboardEvent) => boolean;
  textarea: HTMLTextAreaElement;
};

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    options: Record<string, unknown> = {};
    record: FakeXterm = {
      written: "",
      type: () => {},
      keyHandler: () => true,
      textarea: document.createElement("textarea"),
    };
    constructor() {
      xterms.push(this.record);
    }
    loadAddon() {}
    open(host: HTMLElement) {
      host.append(this.record.textarea);
    }
    attachCustomKeyEventHandler(handler: (event: KeyboardEvent) => boolean) {
      this.record.keyHandler = handler;
    }
    onData(listener: (data: string) => void) {
      this.record.type = listener;
      return { dispose() {} };
    }
    onResize() {
      return { dispose() {} };
    }
    resize(cols: number, rows: number) {
      this.cols = cols;
      this.rows = rows;
    }
    write(data: string) {
      this.record.written += data;
    }
    reset() {
      this.record.written = "";
    }
    get textarea() {
      return this.record.textarea;
    }
    focus() {
      this.record.textarea.focus();
    }
    dispose() {
      this.record.textarea.remove();
    }
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    proposeDimensions() {
      return { cols: 100, rows: 20 };
    }
  },
}));

const PROJECT: Project = { id: "p1", name: "demo", root: "/w/demo", last_opened_ms: null, trusted: false };
const cleanups: (() => void)[] = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
  xterms.length = 0;
});

const settle = async () => {
  await new Promise((resolve) => setImmediate(resolve));
  flushSync();
};

function setup(project: Project | null = PROJECT) {
  const core = new FakeTerminals();
  const workspace = new Workspace();
  workspace.project = project;
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(TerminalsHarness, { target, props: { ovra: { transport: core, workspace } } });
  cleanups.push(() => {
    unmount(app);
    target.remove();
  });
  flushSync();
  const panel = target.querySelector<HTMLElement>("[data-testid=terminals]");
  const outside = target.querySelector<HTMLElement>("[data-testid=outside]");
  if (!panel || !outside) throw new Error("not mounted");
  const tabs = () => [...panel.querySelectorAll<HTMLElement>("[role=tab]")];
  return { core, workspace, target, panel, outside, tabs };
}

async function run(id: string): Promise<void> {
  const command = listCommands().find((c) => c.id === id);
  if (!command) throw new Error(`no command ${id}`);
  await command.run();
  await settle();
}

describe("Terminals", () => {
  it("opens a terminal on toggle, focuses it and echoes typing", async () => {
    const { core, panel, outside, tabs } = setup();
    expect(panel.hidden).toBe(true);
    outside.focus();

    await run("terminal.toggle");
    expect(panel.hidden).toBe(false);
    expect(core.open.get("t1")).toMatchObject({ project: "p1", size: { cols: 100, rows: 20 } });
    expect(tabs().map((t) => t.textContent?.trim())).toEqual(["Terminal 1"]);
    expect(tabs()[0]?.getAttribute("aria-selected")).toBe("true");
    const xterm = xterms[0];
    if (!xterm) throw new Error("no xterm");
    expect(document.activeElement).toBe(xterm.textarea);

    xterm.type("echo hi\r");
    await settle();
    expect(xterm.written).toBe("echo hi\r");

    // Toggle from inside the terminal hides the panel and gives focus back.
    await run("terminal.toggle");
    expect(panel.hidden).toBe(true);
    expect(document.activeElement).toBe(outside);
    expect(core.open.size).toBe(1);
  });

  it("moves between tabs with arrow keys and closes with Delete", async () => {
    const { core, tabs } = setup();
    await run("terminal.toggle");
    await run("terminal.new");
    expect(tabs().map((t) => t.getAttribute("tabindex"))).toEqual(["-1", "0"]);

    tabs()[1]?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    await settle();
    expect(tabs()[0]?.getAttribute("aria-selected")).toBe("true");
    expect(document.activeElement).toBe(tabs()[0]);

    tabs()[0]?.dispatchEvent(new KeyboardEvent("keydown", { key: "Delete", bubbles: true }));
    await settle();
    expect(tabs().map((t) => t.textContent?.trim())).toEqual(["Terminal 2"]);
    expect(document.activeElement).toBe(tabs()[0]);
    expect([...core.open.keys()]).toEqual(["t2"]);
  });

  it("hides the panel when its last terminal closes, and marks exited shells", async () => {
    const { core, panel, outside, tabs } = setup();
    outside.focus();
    await run("terminal.toggle");
    core.exit("t1", 2);
    flushSync();
    expect(tabs()[0]?.textContent?.trim()).toBe("Terminal 1 (exited)");
    expect(xterms[0]?.written).toContain("Process exited with code 2.");
    expect(panel.querySelector(".ended [role=status]")?.textContent?.trim()).toBe(
      "Terminal 1: Process exited with code 2.",
    );

    await run("terminal.close");
    expect(panel.hidden).toBe(true);
    expect(document.activeElement).toBe(outside);
    // The ended shell is closed too, so the core forgets its scrollback.
    expect(core.methods()).toEqual(["terminals.open", "terminals.close"]);
    expect(core.open.size).toBe(0);
  });

  it("explains a failed start in an alert", async () => {
    const { core, panel } = setup();
    core.openError = new CoreError({ code: "unavailable", message: "terminals are not implemented yet (task 2.A)" });
    await run("terminal.toggle");
    expect(panel.querySelector("[role=alert]")?.textContent).toBe(
      "Could not start the terminal: terminals are not implemented yet (task 2.A)",
    );
  });

  it("asks for a project before starting a shell", async () => {
    const { core, panel } = setup(null);
    await run("terminal.toggle");
    expect(panel.hidden).toBe(false);
    expect(panel.textContent).toContain("No project open.");
    expect(core.calls).toEqual([]);
  });

  it("offers a new terminal when the panel shows a project without one", async () => {
    const { core, workspace, panel, tabs } = setup(null);
    await run("terminal.toggle");
    workspace.project = PROJECT;
    flushSync();
    expect(panel.textContent).toContain("No terminals.");
    panel.querySelector<HTMLButtonElement>(".link")?.click();
    await settle();
    expect(tabs().map((t) => t.textContent?.trim())).toEqual(["Terminal 1"]);
    expect(core.open.size).toBe(1);
  });

  it("lets command keys through to the window and keeps Ctrl+C for the shell", async () => {
    setup();
    await run("terminal.toggle");
    const handler = xterms[0]?.keyHandler;
    if (!handler) throw new Error("no xterm");
    const key = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);
    expect(handler(key({ key: "`", code: "Backquote", ctrlKey: true }))).toBe(false);
    expect(handler(key({ key: "c", code: "KeyC", ctrlKey: true }))).toBe(true);
  });

  it("leaves the terminal on Shift+Tab and says so", async () => {
    const { panel, tabs } = setup();
    await run("terminal.toggle");
    const xterm = xterms[0];
    if (!xterm) throw new Error("no xterm");
    const hint = panel.querySelector(`#${xterm.textarea.getAttribute("aria-describedby")}`);
    expect(hint?.textContent).toBe("Shift+Tab leaves the terminal. Ctrl+` hides the panel.");
    const event = new KeyboardEvent("keydown", { key: "Tab", code: "Tab", shiftKey: true, cancelable: true });
    expect(xterm.keyHandler(event)).toBe(false);
    expect(event.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(tabs()[0]);
  });

  it("keeps screen reader mode across panels and confirms the change", async () => {
    localStorage.clear();
    const first = setup();
    const toggle = () => first.panel.querySelector<HTMLButtonElement>(".reader");
    toggle()?.click();
    flushSync();
    expect(toggle()?.getAttribute("aria-pressed")).toBe("true");
    expect(first.panel.textContent).toContain("Screen reader mode on");
    for (const cleanup of cleanups.splice(0)) cleanup();
    expect(setup().panel.querySelector(".reader")?.getAttribute("aria-pressed")).toBe("true");
    localStorage.clear();
  });

  it("restarts an ended shell in the same tab", async () => {
    const { core, panel, tabs } = setup();
    await run("terminal.toggle");
    core.exit("t1", 0);
    flushSync();
    panel.querySelector<HTMLButtonElement>(".ended button")?.click();
    await settle();
    expect(tabs().map((t) => t.textContent?.trim())).toEqual(["Terminal 1"]);
    expect([...core.open.keys()]).toEqual(["t2"]);
    expect(panel.querySelector(".ended")).toBeNull();
    expect(document.activeElement).toBe(xterms[1]?.textarea);
  });
});
