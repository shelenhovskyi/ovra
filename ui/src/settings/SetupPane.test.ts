import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it } from "vitest";
import type { Request, SetupReport } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { mainView } from "../shell/main-view.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { openSettings } from "./commands";
import { settings } from "./panes.svelte";
import TestHost from "./TestHost.svelte";

const report: SetupReport = {
  agents: [
    {
      id: "claude",
      name: "Claude",
      setup: {
        path: null,
        executable: null,
        version: null,
        state: { state: "signed_out" },
        billing: null,
        api_key_in_env: false,
      },
    },
  ],
  git: { state: "checked", found: { installed: true, version: "2.47.1" } },
  gh: { state: "skipped" },
  sandbox: { state: "available", name: "bubblewrap", gaps: ["DNS lookups"] },
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

it("lists each check with its fix, checks again and opens the pane that fixes it", async () => {
  const calls: Request[] = [];
  const transport: Transport = {
    call: ((request: Request) => {
      calls.push(request);
      if (request.method === "setup.check") return Promise.resolve({ type: "setup.report", data: report });
      if (request.method === "info.get")
        return Promise.resolve({ type: "info", data: { name: "ovra", version: "1", mode: "headless" } });
      return Promise.reject(new Error(`unexpected ${request.method}`));
    }) as Transport["call"],
    onEvent: () => () => {},
  };
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(TestHost, { target, props: { transport, workspace: new Workspace() } });
  cleanup.push(() => {
    unmount(app);
    target.remove();
  });

  openSettings("setup");
  await settle();
  const pane = () => target.querySelector("[data-testid=setup-check]");
  expect(pane()?.querySelector("[role=status]")?.textContent?.trim()).toBe("1 problem to look at.");
  const claude = pane()?.querySelector("[data-testid='setup-agent:claude']");
  expect(claude?.querySelector("[role=img]")?.getAttribute("aria-label")).toBe("Problem");
  expect(claude?.querySelector("code")?.textContent).toBe("claude auth login");
  expect(pane()?.querySelector("[data-testid=setup-sandbox]")?.textContent).toContain("It does not cover DNS lookups.");
  expect(calls.filter((c) => c.method === "setup.check")).toEqual([
    { method: "setup.check", params: { project: null } },
  ]);

  const button = (name: string) =>
    [...(pane()?.querySelectorAll("button") ?? [])].find((b) => b.textContent === name) as HTMLButtonElement;
  button("Check again").click();
  await settle();
  expect(calls.filter((c) => c.method === "setup.check").length).toBe(2);

  button("Open Agents settings").click();
  flushSync();
  expect(settings.pane).toBe("claude");
});
