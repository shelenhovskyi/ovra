import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it } from "vitest";
import type { Worker } from "../lib/protocol";
import { settings } from "../settings/panes.svelte";
import { settingsWindow } from "../settings/window.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { FakeAgentCore } from "./fake-core";
import { agentPrefs } from "./prefs.svelte";
import TestHost from "./TestHost.svelte";

const GB = 2 ** 30;
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
  flushSync();
  await new Promise((resolve) => setImmediate(resolve));
  flushSync();
}

async function open(core: FakeAgentCore) {
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
}

/** A task whose coordinator started the given workers, one session each. */
function task(...workers: (Partial<Worker> & { name: string })[]): FakeAgentCore {
  const core = new FakeAgentCore();
  const coordinator = core.addSession("p1", "Write the notes", { state: "idle" });
  core.tasks.push({
    id: "t1",
    project: "p1",
    coordinator: coordinator.id,
    workers: workers.map((w) => ({
      session: core.addSession("p1", `Write the ${w.name} notes.`, { state: "queued" }).id,
      branch: `ovra/t1/${w.name}`,
      worktree: `/wt/${w.name}`,
      kept: null,
      ...w,
    })),
    limits: { max_workers: 3, budget_micro_usd: null },
    spent_micro_usd: 0,
  });
  return core;
}

const row = (title: string) =>
  [...target.querySelectorAll(".sessions .row-button")].find((b) => b.textContent?.includes(title));
const button = (name: string) =>
  [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim() === name);

it("shows a preparing worker's step and, on request, the end of its output", async () => {
  const core = task({
    name: "docs",
    setup: { step: 1, steps: 3, command: "pnpm install", log: ["Resolving", "Linking"], failed: null },
  });
  const docs = core.tasks[0]?.workers[0]?.session ?? "";
  for (const s of core.sessions) if (s.id === docs) s.status = { state: "preparing" };
  await open(core);
  expect(row("docs")?.querySelector(".state")?.textContent).toBe("Preparing worktree · 2 of 3: pnpm install");
  // The coordinator waits while its worker is prepared.
  expect(row("Write the notes")?.querySelector(".state")?.textContent).toBe("Workers are on it");
  const setup = target.querySelector("[data-testid=worker-setup]");
  expect(setup?.querySelector("[data-testid=worker-setup-log]")).toBeNull();
  // Nothing failed: nothing to retry.
  expect(button("Retry")).toBeUndefined();
  button("Show output")?.click();
  flushSync();
  expect(setup?.querySelector("[data-testid=worker-setup-log]")?.textContent).toBe("Resolving\nLinking");
  expect(button("Hide output")?.getAttribute("aria-expanded")).toBe("true");

  // Setup ends: the worker starts and its setup goes.
  for (const w of core.tasks[0]?.workers ?? []) w.setup = null;
  core.emit({ type: "orchestration.changed", data: { project: "p1", task: "t1" } });
  core.status(docs, { state: "starting" });
  await settle();
  expect(row("docs")?.querySelector(".state")?.textContent).toBe("Starting");
  expect(target.querySelector("[data-testid=worker-setup]")).toBeNull();
});

it("names a failed step with Retry and Start anyway", async () => {
  const core = task({
    name: "docs",
    setup: { step: 0, steps: 2, command: "pnpm install", log: ["ERR_PNPM_FETCH_404"], failed: "exited with code 1" },
  });
  const docs = core.tasks[0]?.workers[0]?.session ?? "";
  for (const s of core.sessions) if (s.id === docs) s.status = { state: "preparing" };
  await open(core);
  expect(row("docs")?.querySelector(".state")?.textContent).toBe("Setup failed at 1 of 2: pnpm install");
  expect(row("Write the notes")?.querySelector(".state")?.textContent).toBe("A worker's setup failed");
  expect(target.querySelector("[data-testid=worker-setup-failed]")?.textContent).toBe(
    "Step 1 of 2: pnpm install exited with code 1",
  );
  button("Retry")?.click();
  await settle();
  button("Start anyway")?.click();
  await settle();
  expect(core.sent("orchestration.retry_setup").map((c) => c.params)).toEqual([
    { session: docs, skip_setup: false },
    { session: docs, skip_setup: true },
  ]);
});

it("points out copied files that look like secrets and where tokens go instead", async () => {
  const core = task(
    {
      name: "docs",
      setup: { step: 0, steps: 1, command: "pnpm install", log: [], failed: null },
      included: [".env", "README.local.md", "certs/dev.pem"],
    },
    { name: "tests", included: ["node_modules/a/index.js"] },
  );
  for (const s of core.sessions) if (s.id !== core.tasks[0]?.coordinator) s.status = { state: "preparing" };
  await open(core);
  const notes = [...target.querySelectorAll("[data-testid=worker-setup-secrets]")].map((n) =>
    n.textContent?.replace(/\s+/g, " ").trim(),
  );
  // Only the worker with secret-looking copies gets the note.
  expect(notes).toEqual([
    "Copied into this worker's folder, where its agent can read them: .env, dev.pem. For tokens, use project secrets instead.",
  ]);
  button("project secrets")?.click();
  expect([settingsWindow.open, settings.pane]).toEqual([true, "network"]);
  settingsWindow.close();
});

it("shows each worktree's disk use, the project's total, and why a worker waits", async () => {
  const core = task(
    { name: "docs", disk: { bytes: 1.2 * GB, complete: true } },
    { name: "tests", disk: { bytes: 0.8 * GB, complete: false } },
    {
      name: "lint",
      waiting:
        "waits for disk space: the volume of Ovra's data folder has 3.1 GiB free, and a new worktree needs at least 5 GiB",
    },
  );
  await open(core);
  const disk = (title: string) => row(title)?.querySelector("[data-testid=agent-disk]")?.textContent;
  expect([disk("docs"), disk("tests"), disk("lint")]).toEqual([
    "up to 1.2 GB on disk",
    "more than 819 MB on disk",
    undefined,
  ]);
  // The project's total is a resource number: shown only when the person turned those on.
  expect(target.querySelector("[data-testid=agents-disk]")).toBeNull();
  agentPrefs.showResources = true;
  flushSync();
  expect(target.querySelector("[data-testid=agents-disk]")?.textContent).toBe("more than 2.0 GB on disk");
  agentPrefs.showResources = false;
  expect(row("lint")?.querySelector(".state")?.textContent).toBe("Waiting for disk space");
});

it("lists leftover worktrees only when there are any, and removes one only after a confirmation", async () => {
  const empty = new FakeAgentCore();
  await open(empty);
  expect(target.querySelector("[data-testid=leftovers]")).toBeNull();
  if (app) unmount(app);
  app = null;

  const core = new FakeAgentCore();
  core.leftovers = [
    { path: "/data/worktrees/p1/old-docs", disk: { bytes: 2 * GB, complete: true } },
    { path: "/data/worktrees/p1/crashed", disk: null },
  ];
  await open(core);
  const list = target.querySelector("[data-testid=leftovers]");
  expect(list?.querySelector("h3")?.textContent).toBe("Leftover worktrees");
  expect([...(list?.querySelectorAll("li") ?? [])].map((li) => li.textContent?.replace(/\s+/g, " ").trim())).toEqual([
    "old-docs up to 2.0 GB on disk Remove",
    "crashed size unknown Remove",
  ]);
  list?.querySelector<HTMLButtonElement>("[aria-label='Remove old-docs']")?.click();
  await settle();
  const confirm = target.querySelector("[data-testid=leftover-confirm]");
  expect(confirm?.textContent).toContain("Delete old-docs and everything in it?");
  expect(document.activeElement?.textContent).toBe("Delete");
  // Escape keeps it.
  confirm?.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true }));
  flushSync();
  expect(target.querySelector("[data-testid=leftover-confirm]")).toBeNull();
  expect(core.sent("orchestration.remove_leftover")).toEqual([]);

  list?.querySelector<HTMLButtonElement>("[aria-label='Remove old-docs']")?.click();
  await settle();
  button("Delete")?.click();
  await settle();
  expect(core.sent("orchestration.remove_leftover").map((c) => c.params)).toEqual([
    { project: "p1", path: "/data/worktrees/p1/old-docs" },
  ]);
  expect(target.querySelectorAll("[data-testid=leftovers] li")).toHaveLength(1);
});
