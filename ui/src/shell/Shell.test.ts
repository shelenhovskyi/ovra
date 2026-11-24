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
    await new Promise((resolve) => setTimeout(resolve)); // let the reply resolve
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
    await new Promise((resolve) => setTimeout(resolve));
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
    await new Promise((resolve) => setTimeout(resolve));
    flushSync();
    expect(target.querySelector("[role=status]")?.textContent).toBe(
      "Core unavailable: it no longer accepts this address",
    );
    expect(target.querySelector("[aria-labelledby=refused-title] h2")?.textContent).toBe("Ovra core was restarted");
    expect(target.querySelector("[aria-labelledby=refused-title]")?.textContent).toContain("OVRA_URL=");
    unmount(app);
  });
});

const INFO = { type: "info", data: { name: "ovra", version: "0.0.0", mode: "test" } };
