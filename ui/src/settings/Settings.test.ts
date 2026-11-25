import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it } from "vitest";
import { listCommands, registerCommand } from "../keys/commands";
import type { Request } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { mainView } from "../shell/main-view.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { registerSettingsCommands } from "./commands";
import TestHost from "./TestHost.svelte";

const transport: Transport = {
  call: ((request: Request) =>
    request.method === "info.get"
      ? Promise.resolve({ type: "info", data: { name: "ovra", version: "1.2.3", mode: "headless" } })
      : Promise.reject(new Error(`unexpected ${request.method}`))) as Transport["call"],
  onEvent: () => () => {},
};

let cleanup: (() => void)[] = [];

afterEach(() => {
  mainView.close(false);
  for (const off of cleanup) off();
  cleanup = [];
});

async function settle(): Promise<void> {
  flushSync();
  await new Promise((resolve) => setImmediate(resolve));
  flushSync();
}

it("opens on a pane from its command, moves with the arrow keys and reopens on the last pane", async () => {
  const target = document.createElement("div");
  document.body.append(target);
  const workspace = new Workspace();
  const app = mount(TestHost, { target, props: { transport, workspace } });
  cleanup.push(
    registerSettingsCommands(),
    registerCommand({ id: "t.save", title: "Save", keys: "Mod+S", run: () => {} }),
  );
  cleanup.push(() => {
    unmount(app);
    target.remove();
  });
  const run = (id: string) =>
    listCommands()
      .find((c) => c.id === id)
      ?.run();

  await run("settings.keyboard");
  await settle();
  const pane = () => target.querySelector("[data-testid=settings] section.pane");
  expect(pane()?.querySelector("h3")?.textContent).toBe("Keyboard");
  expect(pane()?.querySelector("tr")?.textContent).toBe("SaveCtrl+S");
  // The frame's title takes focus; the list is one tab stop, and arrows move in it.
  expect(document.activeElement?.textContent).toBe("Settings");
  const current = () => target.querySelector<HTMLElement>("nav [aria-current=page]");
  current()?.dispatchEvent(new KeyboardEvent("keydown", { code: "ArrowDown", bubbles: true }));
  flushSync();
  expect(current()?.textContent).toBe("Accessibility");
  expect(document.activeElement).toBe(current());
  current()?.dispatchEvent(new KeyboardEvent("keydown", { code: "End", bubbles: true }));
  await settle();
  expect(pane()?.textContent).toContain("ovra 1.2.3");

  mainView.close(false);
  await run("settings.open");
  flushSync();
  expect(current()?.textContent).toBe("About");
});
