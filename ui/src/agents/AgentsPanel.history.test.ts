import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { registerCommand } from "../keys/commands";
import { Workspace } from "../shell/workspace.svelte";
import { FakeAgentCore } from "./fake-core";
import { reveal } from "./reveal.svelte";
import TestHost from "./TestHost.svelte";

let app: Record<string, unknown> | null = null;
let target: HTMLElement;

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  target.remove();
  localStorage.clear();
  reveal.session = null;
});

async function settle() {
  flushSync();
  await new Promise((resolve) => setImmediate(resolve));
  flushSync();
}

describe("AgentsPanel and History", () => {
  it("opens the session History asks for, back on the list if the person had closed it", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the login bug", { state: "stopped" });
    core.addSession("p1", "Write the README", { state: "stopped" });
    localStorage.setItem("ovra.agents.closed.p1", JSON.stringify(["s1"]));
    const workspace = new Workspace();
    workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
    app = mount(TestHost, { target, props: { transport: core, workspace } });
    await settle();
    expect(target.querySelector("[data-testid=agent-session]")).toBeNull();

    reveal.session = "s1";
    await settle();
    expect(target.querySelector("[data-testid=agent-session]")?.textContent).toContain("Fix the login bug");
    expect(reveal.session).toBeNull();
    expect(localStorage.getItem("ovra.agents.closed.p1")).toBe("[]");

    // The header's History button runs the shell's command.
    let ran = 0;
    const off = registerCommand({ id: "agents.history", title: "Agent history", run: () => void ran++ });
    target.querySelector<HTMLButtonElement>("[data-testid=agents-history]")?.click();
    off();
    expect(ran).toBe(1);
  });
});
