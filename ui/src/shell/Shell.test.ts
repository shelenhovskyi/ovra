import { flushSync, mount, unmount } from "svelte";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { listCommands, registerCommand } from "../keys/commands";
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

    it("keeps Reviews and More in the rail and the other panels behind More, each explained", () => {
      const { target, more, menu, items } = shell();
      const rail = target.querySelector("nav[aria-label=Views]");
      expect([...(rail?.querySelectorAll("button") ?? [])].map((b) => b.getAttribute("aria-label"))).toEqual([
        "Reviews",
        "More",
      ]);
      // A press hides the tooltip until the pointer leaves.
      more?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      expect(more?.hasAttribute("data-hush")).toBe(true);
      more?.dispatchEvent(new PointerEvent("pointerleave"));
      expect(more?.hasAttribute("data-hush")).toBe(false);

      expect(target.querySelector("[data-testid=open-audit], [data-testid=open-browser]")).toBeNull();
      expect(target.querySelector("[data-testid=project-name]")?.textContent).toBe("No project open");
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
        ["Toggle terminal", "Ctrl+`", "A shell in the project folder, under the open file"],
        ["Audit log", null, "Everything agents did, and who approved it"],
        ["Project policies", null, "Rules for what agents may do in this project"],
        ["Network and secrets", null, "Which websites agents may reach, and secrets"],
        ["Servers", "Ctrl+Shift+S", "Your project's dev servers, run for agents"],
        ["Agent browser", "Ctrl+Shift+B", "The web browser an agent uses"],
        ["Swap side panels", null, "Agents and files change sides"],
      ]);
      expect(items()[4]?.getAttribute("aria-keyshortcuts")).toBe("Mod+Shift+S");
      expect(items().map((item) => item.dataset.testid)).toEqual([
        "open-terminal",
        "open-audit",
        "open-policy",
        "open-network",
        "open-servers",
        "open-browser",
        "open-swap",
      ]);
    });

    it("moves with the arrow keys and closes on Escape, focus back on More", async () => {
      const { more, menu, items } = shell();
      key(more, "ArrowDown");
      await new Promise((resolve) => setImmediate(resolve)); // the menu renders, then focus moves

      expect(document.activeElement).toBe(items()[0]);
      key(document.activeElement, "ArrowUp");
      expect(document.activeElement).toBe(items()[6]);
      key(document.activeElement, "ArrowDown");
      expect(document.activeElement).toBe(items()[0]);
      key(document.activeElement, "End");
      expect(document.activeElement).toBe(items()[6]);
      // First-letter typeahead: P is Project policies, A goes on to Agent browser.
      key(document.activeElement, "KeyP");
      expect(document.activeElement).toBe(items()[2]);
      key(document.activeElement, "KeyA");
      expect(document.activeElement).toBe(items()[5]);
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

    it("opens a panel's dialog, which gives focus back to More when it closes", async () => {
      const { target, more, items } = shell();
      more?.click();
      flushSync();
      items()[1]?.click();
      flushSync();
      await Promise.resolve();
      flushSync();
      const dialog = target.querySelector("[role=dialog][aria-labelledby=audit-title]");
      expect(dialog).not.toBeNull();
      key(dialog, "Escape");
      flushSync();
      expect(target.querySelector("[aria-labelledby=audit-title]")).toBeNull();
      expect(document.activeElement).toBe(more);
    });

    it("opens the terminal, a mouse way that stays once a file is open", () => {
      const { more, items } = shell();
      // The shell's own terminal command comes back afterwards, for the tests after this one.
      const own = listCommands().find((c) => c.id === "terminal.toggle");
      const run = vi.fn();
      const off = registerCommand({ id: "terminal.toggle", title: "Toggle terminal", run });
      more?.click();
      flushSync();
      items()[0]?.click();
      expect(run).toHaveBeenCalledOnce();
      off();
      if (own) registerCommand(own);
    });

    it("toggles the agent browser and shows that it is open", () => {
      const { target, more, items } = shell();
      more?.click();
      flushSync();
      items()[5]?.click();
      flushSync();
      expect(target.querySelector("[data-testid=browser]")).not.toBeNull();
      more?.click();
      flushSync();
      expect(items()[5]?.getAttribute("aria-checked")).toBe("true");
      items()[5]?.click();
      flushSync();
      expect(target.querySelector("[data-testid=browser]")).toBeNull();
    });
  });
});

const INFO = { type: "info", data: { name: "ovra", version: "0.0.0", mode: "test" } };
