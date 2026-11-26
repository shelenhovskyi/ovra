import { flushSync, mount, unmount } from "svelte";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";
import { listCommands } from "../keys/commands";
import { listContextKeys } from "../keys/context-keys";
import type { Request } from "../lib/protocol";
import { FrameRouter, type Transport } from "../lib/transport";
import Shell from "./Shell.svelte";

/** A transport that answers info.get and nothing else. */
function fakeTransport(): Transport {
  return {
    call: ((request: Request) =>
      request.method === "info.get"
        ? Promise.resolve({ type: "info", data: { name: "ovra", version: "0.0.0", mode: "test" } })
        : Promise.reject(new Error(`unexpected ${request.method}`))) as Transport["call"],
    onEvent: () => () => {},
  };
}

describe("Shell", () => {
  it("mounts its panels and shows the core status", async () => {
    const target = document.createElement("div");
    const app = mount(Shell, { target, props: { transport: fakeTransport() } });
    flushSync(); // run effects: the shell asks the core for info.get
    await new Promise((resolve) => setImmediate(resolve)); // let the reply resolve
    flushSync();
    expect(target.querySelector("[data-testid=sidebar]")?.textContent).toContain("File tree");
    const status = target.querySelector("[data-testid=status]");
    expect(status?.textContent).toBe("Connected");
    // Said to screen readers only: a working connection needs nothing from a person.
    expect(status?.classList.contains("quiet")).toBe(true);
    // The version is for whoever needs it, not the first thing a person reads.
    expect(status?.getAttribute("title")).toBe("ovra 0.0.0 (test)");
    unmount(app);
  });

  it("tells the truth about the connection and asks again after a reconnect", async () => {
    const sent: { id: number; method: string }[] = [];
    const router = new FrameRouter((frame) => sent.push(JSON.parse(frame)), "connecting");
    const answerInfo = () => {
      for (const { id, method } of sent.splice(0)) {
        if (method === "info.get") router.deliver(JSON.stringify({ id, result: INFO }));
      }
    };
    const target = document.createElement("div");
    const app = mount(Shell, { target, props: { transport: router } });
    const status = async () => {
      await Promise.resolve();
      flushSync();
      return target.querySelector("[role=status]")?.textContent;
    };

    expect(await status()).toBe("Connecting…");
    router.setStatus("open");
    answerInfo();
    expect(await status()).toBe("Connected");

    router.setStatus("closed");
    expect(await status()).toBe("Connection to core lost, reconnecting…");
    expect(target.querySelector("[data-testid=status]")?.classList.contains("quiet")).toBe(false);
    router.setStatus("connecting");
    expect(await status()).toBe("Connection to core lost, reconnecting…");

    router.setStatus("open");
    answerInfo();
    expect(await status()).toBe("Connected");
    unmount(app);
  });

  it("says the core is unavailable when the connection never opens", async () => {
    const router = new FrameRouter(() => {}, "connecting");
    const target = document.createElement("div");
    const app = mount(Shell, { target, props: { transport: router } });
    flushSync();
    router.setStatus("closed");
    await new Promise((resolve) => setImmediate(resolve));
    flushSync();
    expect(target.querySelector("[role=status]")?.textContent).toBe("Core unavailable: could not connect, retrying…");
    router.setStatus("connecting");
    flushSync();
    expect(target.querySelector("[role=status]")?.textContent).toBe("Core unavailable: could not connect, retrying…");
    unmount(app);
  });

  it("opens the project of the last visit again", async () => {
    globalThis.ResizeObserver ??= class {
      observe() {}
      unobserve() {}
      disconnect() {}
    };
    localStorage.setItem("ovra.project", "p1");
    const demo = { id: "p1", name: "demo-app", root: "/work/demo-app", last_opened_ms: null, trusted: false };
    const transport = fakeTransport();
    const base = transport.call;
    transport.call = ((request: Request) =>
      request.method === "projects.open"
        ? Promise.resolve({ type: "projects.project", data: demo })
        : base(request)) as Transport["call"];
    const target = document.createElement("div");
    const app = mount(Shell, { target, props: { transport } });
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
    flushSync();
    expect(target.querySelector("[data-testid=project-name]")?.textContent).toMatch(/^\s*demo-app\s*$/);
    expect(localStorage.getItem("ovra.project")).toBe("p1");
    unmount(app);
    localStorage.clear();
  });

  it("keeps the tab row under a view, shows the view as a tab, and lets the rail's panels close it", async () => {
    globalThis.ResizeObserver ??= class {
      observe() {}
      unobserve() {}
      disconnect() {}
    };
    localStorage.setItem("ovra.project", "p1");
    const demo = { id: "p1", name: "demo-app", root: "/work/demo-app", last_opened_ms: null, trusted: false };
    const transport = fakeTransport();
    const base = transport.call;
    transport.call = ((request: Request) =>
      request.method === "projects.open"
        ? Promise.resolve({ type: "projects.project", data: demo })
        : base(request)) as Transport["call"];
    const target = document.body.appendChild(document.createElement("div"));
    const app = mount(Shell, { target, props: { transport } });
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
    flushSync();
    const rail = (name: string) =>
      target.querySelector<HTMLButtonElement>(`nav[aria-label=Views] [aria-label=${name}]`);
    const viewTab = () => target.querySelector("[data-testid=view-tab]");
    expect(rail("Files")?.getAttribute("aria-pressed")).toBe("true");
    // Settings > Keyboard lists the panels' own keys too, grouped as it shows them.
    const places = [...new Set(listContextKeys().map((k) => k.where))].sort((a, b) => a.localeCompare(b));
    expect(
      places.flatMap((where) =>
        listContextKeys()
          .filter((k) => k.where === where)
          .map((k) => `${where}: ${k.keys} ${k.title}`),
      ),
    ).toMatchInlineSnapshot(`
      [
        "Agent browser: Alt+Shift+P Pick an element on the page",
        "Agents: Enter Start the task, or send the reply",
        "Agents: Shift+Enter New line",
        "Approvals: Y Allow once",
        "Approvals: N Deny",
        "Approvals: Shift+N Deny and tell Claude why",
        "Approvals: Up/Down Previous or next request",
        "Approvals: Escape Leave the request waiting",
        "Diffs: N Next change",
        "Diffs: P Previous change",
        "Editor: Shift+F10 Open the menu at the cursor (also the Menu key)",
        "Editor: Shift+F8 Go to previous problem",
        "Editor: Ctrl+Shift+[ Fold at cursor",
        "Editor: Ctrl+Shift+] Unfold at cursor",
        "Files: Enter Open the file, or keep its tab when it is shown",
        "Files: F2 Rename",
        "Files: Delete Move to the trash",
        "Files: Shift+Up/Down Select a range of rows (Mod+click adds one)",
        "Files: Shift+Home/End/PageUp/PageDown Select up to the first or last row, or by a page",
        "Files: Mod+A Select every shown row",
        "Files: Mod+X Cut",
        "Files: Mod+C Copy, also as paths for other apps",
        "Files: Mod+V Paste into the folder",
        "Files: Escape Clear the selection and the cut",
        "Files: Mod+Z Undo the last file operation",
        "Files: Mod+Shift+Z Redo the file operation",
        "Files: Shift+F10 Open the row's menu (also the Menu key)",
        "Git: S Stage",
        "Git: U Unstage",
        "Git: O Open the file",
        "Git: Enter Show the diff",
        "Git: Delete Discard changes…",
        "Git: Shift+F10 Open the row's menu (also the Menu key)",
        "Git: Mod+Enter Commit",
        "Reviews: Up/Down Previous or next item or file",
        "Reviews: Mod+Enter Send the comment",
        "Reviews: Escape Leave the comment or the discard step",
        "Search or run: Up/Down Previous or next result",
        "Search or run: Ctrl+N Next result (also Ctrl+J)",
        "Search or run: Ctrl+P Previous result (also Ctrl+K)",
        "Search or run: PageUp/PageDown Move by a page",
        "Search or run: Enter Run or open the result",
        "Search or run: Escape Close",
        "Tabs: Shift+F10 Open the tab's menu (also the Menu key)",
        "Tabs: Left/Right Previous or next tab",
        "Tabs: Delete Close the tab",
        "Views: Escape Close Settings, Reviews or the Audit log",
      ]
    `);

    rail("Settings")?.click();
    flushSync();
    expect(viewTab()?.textContent?.trim()).toBe("Settings");
    expect(target.querySelector(".tab-row")?.closest("[hidden]")).toBeNull();
    // One chosen icon at a time: the gear, not Files.
    expect(rail("Files")?.getAttribute("aria-pressed")).toBe("false");
    expect(rail("Settings")?.getAttribute("aria-pressed")).toBe("true");

    // Files brings the files back with the tree beside them, without folding it.
    rail("Files")?.click();
    flushSync();
    expect(target.querySelector("[data-testid=settings]")).toBeNull();
    expect(viewTab()).toBeNull();
    expect(rail("Files")?.getAttribute("aria-pressed")).toBe("true");
    expect(target.querySelector("[data-testid=sidebar] > .body")?.hasAttribute("hidden")).toBe(false);

    // The view's tab closes it too.
    rail("Settings")?.click();
    flushSync();
    viewTab()?.querySelector<HTMLButtonElement>("button[aria-label='Close Settings']")?.click();
    flushSync();
    expect(target.querySelector("[data-testid=settings]")).toBeNull();
    unmount(app);
    target.remove();
    localStorage.clear();
  });

  it("folds Agents while a request waits, and says so in the title bar until a new one comes", async () => {
    const request = { id: "r1", session: "s1", project: "p1", tool: "Bash", input: {}, title: null };
    const listeners: ((event: unknown) => void)[] = [];
    const transport = fakeTransport();
    const base = transport.call;
    transport.call = ((r: Request) =>
      r.method === "permissions.pending"
        ? Promise.resolve({ type: "permissions.pending", data: [request] })
        : base(r)) as Transport["call"];
    transport.onEvent = (listener) => {
      listeners.push(listener as (event: unknown) => void);
      return () => {};
    };
    const target = document.createElement("div");
    const app = mount(Shell, { target, props: { transport } });
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
    flushSync();
    const agents = () => target.querySelector("[data-testid=agents] > .body");
    const chip = () => target.querySelector<HTMLButtonElement>("[data-testid=approvals-waiting]");
    expect(agents()?.hasAttribute("hidden")).toBe(false);
    expect(chip()).toBeNull();

    await listCommands()
      .find((c) => c.id === "layout.toggle_agents")
      ?.run();
    flushSync();
    expect(agents()?.hasAttribute("hidden")).toBe(true);
    expect(chip()?.textContent).toBe("1 waiting");
    expect(target.querySelector(".strip button")?.getAttribute("aria-label")).toBe(
      "Show Agents, 1 waiting for approval",
    );

    // A new request brings the column back.
    for (const listener of listeners) listener({ type: "permissions.requested", data: { ...request, id: "r2" } });
    flushSync();
    expect(agents()?.hasAttribute("hidden")).toBe(false);
    expect(chip()).toBeNull();
    unmount(app);
    localStorage.clear();
  });

  it("checks Claude again on the start screen when the window comes back, and warns of an API key", async () => {
    const transport = fakeTransport();
    const base = transport.call;
    let checks = 0;
    transport.call = ((r: Request) => {
      if (r.method !== "agents.setup") return base(r);
      checks++;
      // An API key set meanwhile: Claude is ready, but the key pays.
      return Promise.resolve({ type: "agents.setup", data: { ...READY, api_key_in_env: checks > 1 } });
    }) as Transport["call"];
    const target = document.createElement("div");
    const app = mount(Shell, { target, props: { transport } });
    const settled = async () => {
      flushSync();
      await new Promise((resolve) => setImmediate(resolve));
      flushSync();
    };
    await settled();
    expect(checks).toBe(1);
    expect(target.querySelector("[data-testid=claude-api-key]")).toBeNull();
    window.dispatchEvent(new FocusEvent("focus"));
    await settled();
    expect(checks).toBe(2);
    expect(target.querySelector("[data-testid=claude-api-key]")?.textContent).toContain("ANTHROPIC_API_KEY");
    unmount(app);
  });

  it("explains a refused address instead of retrying", async () => {
    const router = new FrameRouter(() => {}, "connecting");
    const target = document.createElement("div");
    const app = mount(Shell, { target, props: { transport: router } });
    flushSync();
    router.setStatus("refused");
    await new Promise((resolve) => setImmediate(resolve));
    flushSync();
    expect(target.querySelector("[role=status]")?.textContent).toBe(
      "Core unavailable: it no longer accepts this address",
    );
    expect(target.querySelector("[aria-labelledby=refused-title] h2")?.textContent).toBe("Ovra was restarted");
    expect(target.querySelector("[aria-labelledby=refused-title]")?.textContent).toContain("OVRA_URL=");
    unmount(app);
  });

  describe("More menu", () => {
    const key = (el: Element | null, code: string) =>
      el?.dispatchEvent(new KeyboardEvent("keydown", { code, bubbles: true, cancelable: true }));

    // One shell for the menu's tests, mounted once: each test starts with the
    // menu closed, and afterEach closes it again.
    let target: HTMLElement;
    let app: Record<string, unknown>;
    beforeAll(() => {
      target = document.body.appendChild(document.createElement("div"));
      app = mount(Shell, { target, props: { transport: fakeTransport() } });
      flushSync();
    });
    afterEach(() => {
      document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      flushSync();
    });
    afterAll(() => {
      unmount(app);
      target.remove();
    });

    function shell() {
      const more = target.querySelector<HTMLButtonElement>("[data-testid=more]");
      const menu = () => target.querySelector("[role=menu]");
      const items = () => [...target.querySelectorAll<HTMLElement>("[role^=menuitem]")];
      return { target, more, menu, items };
    }

    it("keeps only More in the start screen's title bar and the other panels behind it, each explained", () => {
      const { target, more, menu, items } = shell();
      // No project: no rail, no search field, no panel toggles; Reviews stays mounted for its shortcut.
      expect(target.querySelector("nav[aria-label=Views] button")).toBeNull();
      const title = target.querySelector("header.title");
      const shown = [...(title?.querySelectorAll("button") ?? [])].filter((b) => !b.closest("[hidden]"));
      expect(shown.map((b) => b.getAttribute("aria-label"))).toEqual(["More"]);
      // A press hides the tooltip until the pointer leaves.
      more?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      expect(more?.hasAttribute("data-hush")).toBe(true);
      more?.dispatchEvent(new PointerEvent("pointerleave"));
      expect(more?.hasAttribute("data-hush")).toBe(false);

      expect(target.querySelector("[data-testid=open-audit], [data-testid=open-browser]")).toBeNull();
      expect(more?.getAttribute("aria-haspopup")).toBe("menu");
      expect(more?.getAttribute("aria-expanded")).toBe("false");

      more?.click();
      flushSync();
      expect(more?.getAttribute("aria-expanded")).toBe("true");
      expect(menu()?.getAttribute("aria-labelledby")).toBe(more?.id);
      // The palette's names, so searching for what the menu says finds it; the shortcut on the right.
      const described = items().map((item) => [
        item.getAttribute("aria-label"),
        item.querySelector("kbd")?.textContent ?? null,
        document.getElementById(item.getAttribute("aria-describedby") ?? "")?.textContent,
      ]);
      expect(described).toEqual([
        ["Settings…", "Ctrl+,", "Rules, network, agents and more"],
        ["Audit log", "Ctrl+Alt+L", "Everything agents did, and who approved it"],
        ["Agent browser", "Ctrl+Shift+B", "The web browser an agent uses"],
        ["Keyboard shortcuts", null, "Every command with a shortcut"],
      ]);
      expect(items()[0]?.getAttribute("aria-keyshortcuts")).toBe("Mod+,");
      expect(items().map((item) => item.dataset.testid)).toEqual([
        "open-settings",
        "open-audit",
        "open-browser",
        "open-shortcuts",
      ]);
    });

    it("moves with the arrow keys and closes on Escape, focus back on More", async () => {
      const { more, menu, items } = shell();
      key(more, "ArrowDown");
      await new Promise((resolve) => setImmediate(resolve)); // the menu renders, then focus moves

      expect(document.activeElement).toBe(items()[0]);
      key(document.activeElement, "ArrowUp");
      expect(document.activeElement).toBe(items()[3]);
      key(document.activeElement, "ArrowDown");
      expect(document.activeElement).toBe(items()[0]);
      key(document.activeElement, "End");
      expect(document.activeElement).toBe(items()[3]);
      // First-letter typeahead: A is Audit log, A again goes on to Agent browser.
      key(document.activeElement, "KeyA");
      expect(document.activeElement).toBe(items()[1]);
      key(document.activeElement, "KeyA");
      expect(document.activeElement).toBe(items()[2]);
      key(document.activeElement, "Escape");
      flushSync();
      expect(menu()).toBeNull();
      expect(document.activeElement).toBe(more);
    });

    it("closes on a press outside", () => {
      const { more, menu } = shell();
      more?.click();
      flushSync();
      expect(menu()).not.toBeNull();
      document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      flushSync();
      expect(menu()).toBeNull();
    });

    it("opens the audit log in the main area, which gives focus back to More when it closes", async () => {
      const { target, more, items } = shell();
      more?.click();
      flushSync();
      items()[1]?.click();
      flushSync();
      // The start screen makes way for the view, which takes the focus.
      const view = target.querySelector("main [data-testid=audit]");
      expect(target.querySelector("main")?.hidden).toBe(false);
      expect(target.querySelector(".shell")?.classList.contains("viewing")).toBe(true);
      expect(document.activeElement?.textContent).toBe("Audit log");
      key(view, "Escape");
      flushSync();
      expect(target.querySelector("[data-testid=audit]")).toBeNull();
      expect(target.querySelector("main")?.hidden).toBe(true);
      expect(document.activeElement).toBe(more);
    });

    it("opens Settings from Settings…, with Ovra's panes only while no project is open", async () => {
      const { target, more, items } = shell();
      more?.click();
      flushSync();
      items()[0]?.click();
      flushSync();
      const view = target.querySelector("main [data-testid=settings]");
      const panes = [...(view?.querySelectorAll("nav button") ?? [])].map((b) => b.textContent);
      expect(panes).toEqual([
        "Editor",
        "Agents",
        "Setup check",
        "Notifications",
        "Phone",
        "Keyboard",
        "Accessibility",
        "About",
      ]);
      key(view, "Escape");
      flushSync();
      expect(target.querySelector("[data-testid=settings]")).toBeNull();
    });

    it("toggles the agent browser and shows that it is open", () => {
      const { target, more, items } = shell();
      more?.click();
      flushSync();
      items()[2]?.click();
      flushSync();
      expect(target.querySelector("[data-testid=browser]")).not.toBeNull();
      more?.click();
      flushSync();
      expect(items()[2]?.getAttribute("aria-checked")).toBe("true");
      items()[2]?.click();
      flushSync();
      expect(target.querySelector("[data-testid=browser]")).toBeNull();
    });
  });
});

const INFO = { type: "info", data: { name: "ovra", version: "0.0.0", mode: "test" } };

const READY = {
  path: null,
  executable: "/usr/local/bin/claude",
  version: "2.1.0",
  state: { state: "ready" },
  billing: { kind: "subscription" },
  api_key_in_env: false,
} as const;
