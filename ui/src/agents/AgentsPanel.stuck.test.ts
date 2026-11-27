import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it } from "vitest";
import { Workspace } from "../shell/workspace.svelte";
import { FakeAgentCore } from "./fake-core";
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
});

async function settle() {
  for (let i = 0; i < 3; i++) {
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
  }
  flushSync();
}

async function open(core: FakeAgentCore) {
  const workspace = new Workspace();
  workspace.project = {
    id: "p1",
    name: "demo",
    root: "/demo",
    last_opened_ms: null,
    trusted: false,
  };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
}

const row = (id: string) => target.querySelector<HTMLButtonElement>(`[data-session=${id}]`);

it("hints that a working agent may be stuck, in its row and under its transcript, until it moves on", async () => {
  const core = new FakeAgentCore();
  core.addSession("p1", "Fix the build", { state: "working" });
  await open(core);
  const stuck = {
    kind: "repeating" as const,
    name: "Bash",
    call: { kind: "execute" as const, command: "make" },
    times: 3,
  };
  core.emit({ type: "agents.stuck", data: { session: "s1", stuck } });
  await settle();
  expect(row("s1")?.querySelector(".pill")?.textContent).toBe("Stuck?");
  row("s1")?.click();
  await settle();
  expect(target.querySelector("[data-testid=stuck-hint]")?.textContent).toBe(
    "It may be going in circles: it ran make 3 times with the same result. Stop it, or send it a hint.",
  );
  core.emit({ type: "agents.stuck", data: { session: "s1", stuck: null } });
  await settle();
  expect(target.querySelector("[data-testid=stuck-hint]")).toBeNull();
});

it("continues what Ovra's quit cut off, all at once from the list or one from its session", async () => {
  const core = new FakeAgentCore();
  for (const title of ["Plan the API", "Write the docs", "Finished one"]) {
    const s = core.addSession("p1", title, { state: "stopped" });
    Object.assign(s, {
      resumable: true,
      interrupted: title !== "Finished one",
    });
  }
  await open(core);
  expect(target.querySelector("[data-testid=agents-cut-off] p")?.textContent).toBe(
    "2 agents stopped when Ovra closed.",
  );
  expect(row("s1")?.querySelector(".state")?.textContent).toBe("Stopped when Ovra closed");
  expect(row("s3")?.querySelector(".state")?.textContent).toBe("Stopped");

  row("s2")?.click();
  await settle();
  target.querySelector<HTMLButtonElement>("[data-testid=agent-cut-off] button")?.click();
  await settle();
  expect(core.sent("agents.resume_interrupted").map((c) => c.params)).toEqual([{ project: "p1", session: "s2" }]);
  expect(target.querySelector("[data-testid=agent-cut-off]")).toBeNull();

  target.querySelector<HTMLButtonElement>("[data-testid=agent-session] .back")?.click();
  await settle();
  expect(target.querySelector("[data-testid=agents-cut-off] p")?.textContent).toBe("1 agent stopped when Ovra closed.");
  target.querySelector<HTMLButtonElement>("[data-testid=agents-cut-off] button")?.click();
  await settle();
  expect(core.sent("agents.resume_interrupted").at(-1)?.params).toEqual({
    project: "p1",
  });
  expect(target.querySelector("[data-testid=agents-cut-off]")).toBeNull();
});
