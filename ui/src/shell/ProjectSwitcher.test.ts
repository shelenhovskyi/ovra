import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import type { Project } from "../lib/protocol";
import { registerPaletteCommands } from "../palette/commands";
import { FakeCore } from "../tree/fake-core";
import SwitcherHarness from "./SwitcherHarness.svelte";
import { Workspace } from "./workspace.svelte";

// jsdom has no ResizeObserver, which the file tree's `bind:clientHeight` uses.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const project = (id: string, name: string): Project => ({
  id,
  name,
  root: `/work/${name}`,
  last_opened_ms: null,
  trusted: false,
});

/** The desktop window: the system's folder dialog picks `/work/picked`. */
class DesktopCore extends FakeCore {
  async pickFolder(): Promise<string | null> {
    return "/work/picked";
  }
}

let cleanup: (() => void) | null = null;
afterEach(() => {
  cleanup?.();
  cleanup = null;
  document.body.innerHTML = "";
});

async function settle(): Promise<void> {
  for (let i = 0; i < 3; i++) {
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
  }
  flushSync();
}

async function mountSwitcher(
  core = new FakeCore(new Map([["", []]]), [project("p1", "demo-app"), project("p2", "empty")]),
) {
  const workspace = new Workspace();
  workspace.project = core.projects[0] ?? null;
  const target = document.body.appendChild(document.createElement("div"));
  const app = mount(SwitcherHarness, {
    target,
    props: { ovra: { transport: core, workspace } },
  });
  const unregister = registerPaletteCommands(workspace);
  cleanup = () => {
    unregister();
    unmount(app);
  };
  await settle();
  const button = () => target.querySelector<HTMLButtonElement>("[data-testid=project-name]");
  const items = () => [...target.querySelectorAll<HTMLElement>("[role=menuitem]")];
  const key = (code: string) =>
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { code, bubbles: true, cancelable: true }));
  return { core, workspace, target, button, items, key };
}

describe("ProjectSwitcher", () => {
  it("is a menu button listing the other recent projects, a folder opener and Close project", async () => {
    const { button, items, target } = await mountSwitcher();
    expect(button()?.textContent).toMatch(/Project\s+demo-app/);
    expect(button()?.getAttribute("aria-haspopup")).toBe("menu");
    button()?.click();
    await settle();
    expect(button()?.getAttribute("aria-expanded")).toBe("true");
    expect(items().map((item) => item.getAttribute("aria-label") ?? item.textContent?.trim())).toEqual([
      "empty",
      "Open a folder…",
      "Close project",
    ]);
    expect(target.querySelector("[role=group]")?.textContent).toContain("/work/empty");
    expect(document.activeElement).toBe(items()[0]);
  });

  it("switches to a recent project and gives focus back to the button", async () => {
    const { workspace, button, items } = await mountSwitcher();
    workspace.openPath = "README.md";
    button()?.click();
    await settle();
    items()[0]?.click();
    await settle();
    expect(workspace.project?.id).toBe("p2");
    expect(workspace.openPath).toBeNull();
    expect(items()).toEqual([]);
    expect(document.activeElement).toBe(button());
  });

  it("keeps the menu open with the reason when a project does not open", async () => {
    const { core, workspace, button, items, target } = await mountSwitcher();
    button()?.click();
    await settle();
    core.projects.splice(1, 1); // removed meanwhile: projects.open answers not_found
    items()[0]?.click();
    await settle();
    expect(workspace.project?.id).toBe("p1");
    expect(target.querySelector("[role=menu] [role=alert]")?.textContent).toBe("Folder not found: /work/empty");
  });

  it("opens with the arrow keys, moves by first letter and closes on Escape", async () => {
    const { button, items, key } = await mountSwitcher();
    button()?.focus();
    key("ArrowUp");
    await settle();
    expect(document.activeElement).toBe(items()[2]);
    key("KeyO");
    expect(document.activeElement).toBe(items()[1]);
    key("Escape");
    flushSync();
    expect(items()).toEqual([]);
    expect(document.activeElement).toBe(button());
  });

  it("without a folder dialog, Open a folder… asks for a path and keeps the project until it opens", async () => {
    const { workspace, button, items, target, key } = await mountSwitcher();
    const field = () => target.querySelector<HTMLInputElement>("#project-menu-path");
    const submit = async (path: string) => {
      const input = field();
      if (!input) throw new Error("no path field");
      input.value = path;
      input.dispatchEvent(new Event("input", { bubbles: true }));
      input.form?.requestSubmit();
      await settle();
    };
    button()?.click();
    await settle();
    expect(items()[1]?.querySelector("kbd")).toBeNull();
    items()[1]?.click();
    await settle();
    expect(workspace.project?.id).toBe("p1");
    expect(document.activeElement).toBe(field());
    // Escape goes back to the project as it was.
    key("Escape");
    flushSync();
    expect(field()).toBeNull();
    expect(document.activeElement).toBe(button());

    button()?.click();
    await settle();
    items()[1]?.click();
    await settle();
    await submit("relative");
    expect(target.querySelector("form [role=alert]")?.textContent).toContain("Use the full path");
    expect(workspace.project?.id).toBe("p1");
    await submit("/work/new");
    expect(workspace.project?.root).toBe("/work/new");
    expect(field()).toBeNull();
  });

  it("with a folder dialog, Open a folder… shows its shortcut and opens the picked folder", async () => {
    const core = new DesktopCore(new Map([["", []]]), [project("p1", "demo-app")]);
    const { workspace, button, items } = await mountSwitcher(core);
    button()?.click();
    await settle();
    // No other project: just the two actions.
    expect(items().map((item) => item.textContent?.trim().replace(/\s+/g, " "))).toEqual([
      "Open a folder… Ctrl+O",
      "Close project",
    ]);
    items()[0]?.click();
    await settle();
    expect(workspace.project?.root).toBe("/work/picked");
  });

  it("closes the project", async () => {
    const { workspace, button, items } = await mountSwitcher();
    button()?.click();
    await settle();
    items()[2]?.click();
    await settle();
    expect(workspace.project).toBeNull();
  });
});
