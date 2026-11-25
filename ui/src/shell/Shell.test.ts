import { flushSync, mount, unmount } from "svelte";
import { describe, expect, it } from "vitest";
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
    expect(target.querySelector("[data-testid=status]")?.textContent).toBe("Connected to ovra 0.0.0 (test)");
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
    expect(await status()).toBe("Connected to ovra 0.0.0 (test)");

    router.setStatus("closed");
    expect(await status()).toBe("Connection to core lost, reconnecting…");
    router.setStatus("connecting");
    expect(await status()).toBe("Connection to core lost, reconnecting…");

    router.setStatus("open");
    answerInfo();
    expect(await status()).toBe("Connected to ovra 0.0.0 (test)");
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
    expect(target.querySelector("[aria-labelledby=refused-title] h2")?.textContent).toBe("Ovra core was restarted");
    expect(target.querySelector("[aria-labelledby=refused-title]")?.textContent).toContain("OVRA_URL=");
    unmount(app);
  });

  describe("More menu", () => {
    const key = (el: Element | null, code: string) =>
      el?.dispatchEvent(new KeyboardEvent("keydown", { code, bubbles: true, cancelable: true }));

    function mountShell() {
      const target = document.body.appendChild(document.createElement("div"));
      const app = mount(Shell, { target, props: { transport: fakeTransport() } });
      flushSync();
      const more = target.querySelector<HTMLButtonElement>("[data-testid=more]");
      const menu = () => target.querySelector("[role=menu]");
      const items = () => [...target.querySelectorAll<HTMLElement>("[role^=menuitem]")];
      const done = () => {
        unmount(app);
        target.remove();
      };
      return { target, more, menu, items, done };
    }

    it("keeps Reviews in the header and the other panels behind More, each explained", () => {
      const { target, more, menu, items, done } = mountShell();
      const header = target.querySelector("header");
      expect(
        [...(header?.querySelectorAll(":scope > button, :scope > .more > button") ?? [])].map((b) =>
          b.textContent?.trim(),
        ),
      ).toEqual([expect.stringMatching(/^Reviews/), "More"]);
      expect(target.querySelector("[data-testid=open-audit], [data-testid=open-browser]")).toBeNull();
      expect(target.querySelector("[data-testid=project-name]")?.textContent).toBe("No project open");
      expect(more?.getAttribute("aria-haspopup")).toBe("menu");
      expect(more?.getAttribute("aria-expanded")).toBe("false");

      more?.click();
      flushSync();
      expect(more?.getAttribute("aria-expanded")).toBe("true");
      expect(menu()?.getAttribute("aria-labelledby")).toBe(more?.id);
      const described = items().map((item) => [
        item.getAttribute("aria-label"),
        document.getElementById(item.getAttribute("aria-describedby") ?? "")?.textContent,
      ]);
      expect(described).toEqual([
        ["Audit log", "Everything agents did, and who approved it"],
        ["Policies", "Rules for what agents may do in this project"],
        ["Network", "Which websites agents may reach, and secrets"],
        ["Servers", "Your project's dev servers, run for agents"],
        ["Browser", "The web browser an agent uses"],
      ]);
      done();
    });

    it("moves with the arrow keys and closes on Escape, focus back on More", async () => {
      const { more, menu, items, done } = mountShell();
      key(more, "ArrowDown");
      await new Promise((resolve) => setImmediate(resolve)); // the menu renders, then focus moves

      expect(document.activeElement).toBe(items()[0]);
      key(document.activeElement, "ArrowUp");
      expect(document.activeElement).toBe(items()[4]);
      key(document.activeElement, "ArrowDown");
      expect(document.activeElement).toBe(items()[0]);
      key(document.activeElement, "End");
      expect(document.activeElement).toBe(items()[4]);
      key(document.activeElement, "Escape");
      flushSync();
      expect(menu()).toBeNull();
      expect(document.activeElement).toBe(more);
      done();
    });

    it("closes on a press outside", () => {
      const { more, menu, done } = mountShell();
      more?.click();
      flushSync();
      expect(menu()).not.toBeNull();
      document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      flushSync();
      expect(menu()).toBeNull();
      done();
    });

    it("opens a panel's dialog, which gives focus back to More when it closes", async () => {
      const { target, more, items, done } = mountShell();
      more?.click();
      flushSync();
      items()[0]?.click();
      flushSync();
      await Promise.resolve();
      flushSync();
      const dialog = target.querySelector("[role=dialog][aria-labelledby=audit-title]");
      expect(dialog).not.toBeNull();
      key(dialog, "Escape");
      flushSync();
      expect(target.querySelector("[aria-labelledby=audit-title]")).toBeNull();
      expect(document.activeElement).toBe(more);
      done();
    });

    it("toggles the agent browser and shows that it is open", () => {
      const { target, more, items, done } = mountShell();
      more?.click();
      flushSync();
      items()[4]?.click();
      flushSync();
      expect(target.querySelector("[data-testid=browser]")).not.toBeNull();
      more?.click();
      flushSync();
      expect(items()[4]?.getAttribute("aria-checked")).toBe("true");
      done();
    });
  });
});

const INFO = { type: "info", data: { name: "ovra", version: "0.0.0", mode: "test" } };
