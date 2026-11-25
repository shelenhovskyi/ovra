import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { FakeAgentCore } from "../agents/fake-core";
import type { PermissionRequest } from "../lib/protocol";
import { Workspace } from "../shell/workspace.svelte";
import TestHost from "./TestHost.svelte";

const bash = (id: string): PermissionRequest => ({
  id,
  session: "s1",
  project: "p1",
  cwd: "/demo",
  tool: "Bash",
  input: { command: "ls" },
  title: null,
});

let app: Record<string, unknown> | null = null;
let target: HTMLElement;

beforeEach(() => {
  document.title = "Ovra";
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  target.remove();
});

async function settle() {
  for (let i = 0; i < 3; i++) {
    flushSync();
    await Promise.resolve();
  }
  flushSync();
}

describe("Notifications", () => {
  it("counts waiting requests in the window title", async () => {
    const core = new FakeAgentCore();
    app = mount(TestHost, { target, props: { transport: core, workspace: new Workspace() } });
    await settle();
    expect(document.title).toBe("Ovra");
    core.ask(bash("perm1"));
    core.ask(bash("perm2"));
    await settle();
    expect(document.title).toBe("(2) Ovra");
    core.emit({ type: "permissions.resolved", data: { id: "perm1", decision: "allow" } });
    await settle();
    expect(document.title).toBe("(1) Ovra");
    core.emit({ type: "permissions.resolved", data: { id: "perm2", decision: "deny" } });
    await settle();
    expect(document.title).toBe("Ovra");
  });
});
