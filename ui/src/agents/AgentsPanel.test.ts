import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { browserPicks } from "../browser/picks.svelte";
import { focusMode } from "../focus/mode.svelte";
import { handleOpenFor } from "../inbox/open";
import { listCommands, registerCommand } from "../keys/commands";
import type { BoardEntry, FileChange, Pick, ReviewItem } from "../lib/protocol";
import { CoreError, type WindowDrop } from "../lib/transport";
import { closeContextMenu, contextMenu } from "../menu/context-menu.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { askAgent } from "./context.svelte";
import { FakeAgentCore } from "./fake-core";
import { agentPrefs } from "./prefs.svelte";
import TestHost from "./TestHost.svelte";

let app: Record<string, unknown> | null = null;
let target: HTMLElement;
let workspace: Workspace;

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

async function setup(core = new FakeAgentCore()) {
  workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  return core;
}

function type(field: HTMLTextAreaElement | null, text: string) {
  if (!field) throw new Error("no field");
  field.focus();
  field.value = text;
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

const modEnter = () =>
  document.activeElement?.dispatchEvent(
    new KeyboardEvent("keydown", { code: "Enter", key: "Enter", ctrlKey: true, bubbles: true, cancelable: true }),
  );

/** Picks how the next task runs from the mode menu, as clicks would. */
function choose(label: string) {
  target.querySelector<HTMLButtonElement>("[data-testid=agent-mode]")?.click();
  flushSync();
  const option = [...target.querySelectorAll<HTMLButtonElement>("[role=menuitemradio]")].find(
    (b) => b.querySelector(".name")?.textContent === label,
  );
  option?.click();
  flushSync();
}

const modeLabel = () => target.querySelector("[data-testid=agent-mode]")?.textContent?.trim();

const startButton = () =>
  [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Start");

const panel = () => target.querySelector<HTMLElement>("[data-testid=agents-panel]");
const transcript = () =>
  [...target.querySelectorAll("[data-testid=transcript] > ol > li:not(.live)")].map((li) =>
    li.textContent?.replace(/\s+/g, " ").trim(),
  );

/** Opens the transcript of the session whose row title is `title`. */
function selectRow(title: string) {
  const row = [...target.querySelectorAll<HTMLButtonElement>(".sessions .row-button")].find(
    (b) => b.querySelector(".title")?.textContent === title,
  );
  if (!row) throw new Error(`no row ${title}`);
  row.click();
  flushSync();
}

/** Back from a transcript to the list. */
function back() {
  button("Tasks")?.click();
  flushSync();
}

const button = (name: string) =>
  [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim() === name);

describe("AgentsPanel", () => {
  it("offers one Start button and a mode menu that says what each way to work means", async () => {
    await setup();
    const buttons = [...target.querySelectorAll("form.new button")].map(
      (b) => b.textContent?.trim() || b.getAttribute("aria-label"),
    );
    expect(buttons).toEqual(["One agent, here", "Add context", "Start"]);
    // Who pays, in the footer line (it opens Claude's setup).
    expect(target.querySelector("[data-testid=agents-footer]")?.textContent).toBe("Claude · Claude plan");
    expect(target.querySelector("label[for=agent-prompt]")?.textContent).toBe("Describe a task");
    const hint = () => target.querySelector("#agent-mode-hint")?.textContent;
    expect(hint()).toBe("Works in this folder. Asks before risky steps.");
    target.querySelector<HTMLButtonElement>("[data-testid=agent-mode]")?.click();
    await settle();
    const options = [...target.querySelectorAll("[role=menuitemradio]")].map((o) => [
      o.textContent,
      o.getAttribute("aria-checked"),
    ]);
    expect(options).toEqual([
      ["One agent, hereWorks in this folder. Asks before risky steps.", "true"],
      ["One agent, own copyWorks on its own copy of the project. You review before anything is merged.", "false"],
      ["Plan and splitUp to 3 agents, each on its own copy. You review before anything is merged.", "false"],
      ["ResearchReads the project and searches the web. Changes nothing.", "false"],
    ]);
    expect(document.activeElement?.getAttribute("aria-checked")).toBe("true");
    // Arrow keys move in the menu; the pick closes it.
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { code: "ArrowDown", bubbles: true }));
    expect(document.activeElement?.textContent).toMatch(/^One agent, own copy/);
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { code: "ArrowDown", bubbles: true }));
    expect(document.activeElement?.textContent).toMatch(/^Plan and split/);
    (document.activeElement as HTMLElement).click();
    flushSync();
    expect(target.querySelector("[role=menu]")).toBeNull();
    expect(modeLabel()).toBe("Plan and split");
    expect(hint()).toBe("Up to 3 agents on their own copies. You review first.");
    expect(panel()?.querySelector("[data-testid=agents-empty]")?.textContent?.trim()).toBe("No tasks yet.");
  });

  it("plans and splits a task when asked, from Mod+Enter too, and keeps the choice", async () => {
    const core = await setup();
    choose("Plan and split");
    type(target.querySelector("#agent-prompt"), "Write the notes");
    modEnter();
    await settle();
    expect(core.sent("orchestration.start")).toHaveLength(1);
    expect(core.sent("agents.start")).toEqual([]);
    back();
    expect(modeLabel()).toBe("Plan and split");
    expect(panel()?.querySelector("[data-testid=agents-empty]")).toBeNull();
    // A reload offers the project's last mode again.
    if (app) unmount(app);
    await setup(core);
    expect(modeLabel()).toBe("Plan and split");
  });

  it("starts from the mode picker with Mod+Enter, and asks for a task when the field is empty", async () => {
    const core = await setup();
    type(target.querySelector("#agent-prompt"), "Split this work");
    // In the open menu: the option in focus is the mode it starts with.
    target.querySelector<HTMLButtonElement>("[data-testid=agent-mode]")?.click();
    await settle();
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { code: "ArrowDown", bubbles: true }));
    modEnter();
    await settle();
    expect(core.sent("orchestration.start")).toHaveLength(1);
    back();
    // On the mode button, with nothing typed: the field gets focus and says why.
    target.querySelector<HTMLButtonElement>("[data-testid=agent-mode]")?.focus();
    modEnter();
    await settle();
    expect(core.sent("orchestration.start")).toHaveLength(1);
    expect(document.activeElement?.id).toBe("agent-prompt");
    expect(target.querySelector("[data-testid=agent-type-first]")?.textContent).toBe("Type a task first.");
    type(target.querySelector("#agent-prompt"), "N");
    flushSync();
    expect(target.querySelector("[data-testid=agent-type-first]")).toBeNull();
  });

  it("moves between task rows with arrows, closes with Delete and opens a menu with Shift+F10", async () => {
    const core = new FakeAgentCore();
    for (const title of ["First", "Second", "Third"]) core.addSession("p1", title, { state: "idle" });
    await setup(core);
    const rows = () => [...target.querySelectorAll<HTMLButtonElement>(".sessions .row-button")];
    const key = (code: string, init: KeyboardEventInit = {}) =>
      document.activeElement?.dispatchEvent(
        new KeyboardEvent("keydown", { code, bubbles: true, cancelable: true, ...init }),
      );
    // One Tab stop for the list; Close is reached by Delete or the menu.
    expect(rows().map((r) => r.tabIndex)).toEqual([0, -1, -1]);
    expect([...target.querySelectorAll<HTMLButtonElement>(".sessions .close")].map((b) => b.tabIndex)).toEqual([
      -1, -1, -1,
    ]);
    rows()[0]?.focus();
    key("ArrowDown");
    flushSync();
    expect(document.activeElement).toBe(rows()[1]);
    expect(rows().map((r) => r.tabIndex)).toEqual([-1, 0, -1]);
    key("End");
    expect(document.activeElement).toBe(rows()[2]);
    key("ArrowUp");
    expect(document.activeElement).toBe(rows()[1]);
    key("F10", { shiftKey: true });
    flushSync();
    expect(contextMenu.open?.entries.map((e) => (e === "separator" ? e : e.label))).toEqual([
      "Open",
      "separator",
      "Close",
    ]);
    closeContextMenu(false);
    key("Delete");
    await settle();
    expect(rows().map((r) => r.querySelector(".title")?.textContent)).toEqual(["First", "Third"]);
    // Focus stays in the list, on the row that took its place.
    expect(document.activeElement).toBe(rows()[1]);
  });

  it("disables Start with a reason when Plan and split has no git repository", async () => {
    const core = new FakeAgentCore();
    core.noRepo = true;
    await setup(core);
    type(target.querySelector("#agent-prompt"), "Write the notes");
    choose("Plan and split");
    await settle();
    expect(target.querySelector("[data-testid=agent-needs-git]")?.textContent?.replace(/\s+/g, " ").trim()).toBe(
      "Needs git: Plan and split works on copies of a git repository. Set up git",
    );
    expect(startButton()?.disabled).toBe(true);
    // The Git area's command runs `git init`; the composer asks again.
    const off = registerCommand({
      id: "git.init",
      title: "Git: set up git",
      run: () => {
        core.noRepo = false;
      },
    });
    button("Set up git")?.click();
    await settle();
    off();
    expect(target.querySelector("[data-testid=agent-needs-git]")).toBeNull();
    expect(startButton()?.disabled).toBe(false);
    expect(modeLabel()).toBe("Plan and split");
  });

  it("starts on Enter, not on Shift+Enter or an Enter that ends an IME composition", async () => {
    const core = await setup();
    type(target.querySelector("#agent-prompt"), "Fix the typo");
    const press = (init: KeyboardEventInit) =>
      document.activeElement?.dispatchEvent(
        new KeyboardEvent("keydown", { code: "Enter", key: "Enter", bubbles: true, cancelable: true, ...init }),
      );
    press({ shiftKey: true });
    press({ isComposing: true });
    await settle();
    expect(core.sent("agents.start")).toEqual([]);
    press({});
    await settle();
    expect(core.sent("agents.start")).toHaveLength(1);
  });

  it("starts a task whose coordinator splits the work among workers", async () => {
    const core = await setup();
    choose("Plan and split");
    type(target.querySelector("#agent-prompt"), "Write the notes");
    flushSync();
    startButton()?.click();
    await settle();
    expect(core.sent("orchestration.start").map((c) => c.params)).toEqual([
      { project: "p1", prompt: "Write the notes" },
    ]);
    expect(core.sent("agents.start")).toEqual([]);
    // It opens; the start box is empty for the next one.
    expect(panel()?.querySelector("[data-testid=agent-session] h3")?.textContent).toBe("Task: Write the notes");
    back();
    expect(target.querySelector<HTMLTextAreaElement>("#agent-prompt")?.value).toBe("");
  });

  it("shows what each agent takes of the computer and why queued workers wait", async () => {
    const core = await setup();
    choose("Plan and split");
    type(target.querySelector("#agent-prompt"), "Write the notes");
    flushSync();
    startButton()?.click();
    await settle();
    back();
    const docs = core.addSession("p1", "Write the docs notes.");
    const later = core.addSession("p1", "Write the tests.");
    core.tasks[0]?.workers.push(
      {
        session: docs.id,
        name: "docs",
        branch: "ovra/t1/docs",
        worktree: "/wt/docs",
        kept: null,
        disk: { bytes: 2.4 * 2 ** 30, complete: true },
      },
      { session: later.id, name: "tests", branch: "ovra/t1/tests", worktree: "/wt/tests", kept: null },
    );
    core.emit({ type: "orchestration.changed", data: { project: "p1", task: "t1" } });
    core.capacity = { working: 1, limit: 3, queued: 1, research_working: 0, research_limit: 2, waiting: [] };
    core.status(docs.id, { state: "working" });
    core.status(later.id, { state: "queued" });
    await settle();
    const MB = 2 ** 20;
    core.emit({
      type: "resources.sample",
      data: {
        project: "p1",
        system: { memory_total_bytes: 16 * 2 ** 30, memory_available_bytes: 500 * MB, cpu_percent: 40 },
        pressure: "low_memory",
        sessions: [
          {
            session: docs.id,
            agent: { memory_bytes: 900 * MB, cpu_percent: 30, processes: 5 },
            servers: { memory_bytes: 300 * MB, cpu_percent: 5, processes: 2 },
            browser: null,
          },
          { session: later.id, agent: null, servers: null, browser: null },
        ],
      },
    });
    await settle();
    const row = (title: string) =>
      [...target.querySelectorAll(".sessions .row-button")].find((b) => b.textContent?.includes(title));
    // Hidden until the person asks for it in Settings; why a worker waits shows either way.
    expect(row("docs")?.querySelector("[data-testid=agent-resources]")).toBeNull();
    expect(row("tests")?.querySelector(".state")?.textContent).toBe("Waiting: low memory");
    agentPrefs.showResources = true;
    await settle();
    const taken = row("docs")?.querySelector("[data-testid=agent-resources]");
    expect(taken?.textContent).toBe("1.2 GB · 35% CPU");
    expect(taken?.getAttribute("title")?.split("\n")).toEqual([
      "About 1.2 GB of memory and 35% CPU (100% is one core)",
      "Agent and its commands: 900 MB, 30% CPU, 5 processes",
      "Dev servers: 300 MB, 5% CPU, 2 processes",
      "Worktree: up to 2.4 GB on disk",
    ]);
    // A queued worker runs nothing yet, and says why it waits with a place free.
    expect(row("tests")?.querySelector("[data-testid=agent-resources]")).toBeNull();
    expect(row("tests")?.querySelector(".state")?.textContent).toBe("Waiting: low memory");
    const count = () => target.querySelector("[data-testid=agents-capacity]")?.textContent?.trim();
    expect(count()).toBe("1 of 3 · 1 queued · 1 planning · memory low");

    // Memory is back; an ended session's line goes.
    core.emit({
      type: "resources.sample",
      data: {
        project: "p1",
        system: { memory_total_bytes: 16 * 2 ** 30, memory_available_bytes: 8 * 2 ** 30, cpu_percent: 40 },
        pressure: null,
        sessions: [],
      },
    });
    await settle();
    expect(row("tests")?.querySelector(".state")?.textContent).toBe("Waiting for a free agent");
    expect(row("docs")?.querySelector("[data-testid=agent-resources]")).toBeNull();
    expect(count()).toBe("1 of 3 · 1 queued · 1 planning");
    agentPrefs.showResources = false;
  });

  it("names a task's workers and lists them under their coordinator", async () => {
    const core = await setup();
    choose("Plan and split");
    type(target.querySelector("#agent-prompt"), "Write the notes");
    flushSync();
    startButton()?.click();
    await settle();
    back();
    const docs = core.addSession("p1", "Write the docs notes.");
    core.addSession("p1", "A plain session");
    core.tasks[0]?.workers.push({
      session: docs.id,
      name: "docs",
      branch: "ovra/t1/docs",
      worktree: "/wt/docs",
      kept: null,
    });
    core.emit({ type: "orchestration.changed", data: { project: "p1", task: "t1" } });
    core.capacity = { working: 3, limit: 3, queued: 1, research_working: 0, research_limit: 2, waiting: [] };
    core.status(docs.id, { state: "queued" });
    await settle();
    // The coordinator takes no place, but the count does not hide that it works.
    const count = () => target.querySelector("[data-testid=agents-capacity]")?.textContent?.trim();
    expect(count()).toBe("3 of 3 · 1 queued · 1 planning");
    expect(target.querySelectorAll(".sessions .state")[1]?.textContent).toBe("Waiting for a free agent");
    core.status("s1", { state: "idle" });
    await settle();
    expect(count()).toBe("3 of 3 · 1 queued");
    // A review asked for or decided changes the count without a change of state.
    core.capacity = { working: 2, limit: 3, queued: 1, research_working: 0, research_limit: 2, waiting: [] };
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    expect(count()).toBe("2 of 3 · 1 queued");
    // So does a new limit from Settings.
    core.capacity = { working: 2, limit: 5, queued: 1, research_working: 0, research_limit: 2, waiting: [] };
    core.emit({ type: "agents.limit_changed", data: { limit: 5, default: 3, min: 1, max: 8 } });
    await settle();
    expect(count()).toBe("2 of 5 · 1 queued");
    core.capacity = { working: 3, limit: 3, queued: 1, research_working: 0, research_limit: 2, waiting: [] };
    // Working sessions come before a task whose worker only waits.
    expect(target.querySelectorAll(".sessions .state")[1]?.textContent).toBe("Workers are waiting for a free agent");
    core.status(docs.id, { state: "working" });
    await settle();
    const rows = [...target.querySelectorAll(".sessions .row-button")];
    expect(rows.map((b) => b.querySelector(".title")?.textContent)).toEqual([
      "Write the notes",
      // Under its task: its own name and what it was asked to do, not the task's title again.
      "docs · Write the docs notes.",
      "A plain session",
    ]);
    expect(rows[1]?.closest("li")?.classList.contains("worker")).toBe(true);
    // The whole name shows on hover, should it be cut.
    expect(rows.slice(0, 2).map((b) => b.getAttribute("title"))).toEqual([
      "Task: Write the notes",
      "docs: Write the docs notes.\nBranch ovra/t1/docs",
    ]);
    // A coordinator that waits while its worker runs is not done.
    const state = () => [...target.querySelectorAll(".sessions .state")].map((s) => s.textContent);
    core.status("s1", { state: "idle" });
    await settle();
    expect(state()).toEqual(["Workers are on it", "Working", "Starting"]);
    core.status(docs.id, { state: "idle" });
    await settle();
    expect(state()).toEqual(["Starting", "Done", "Done"]);
  });

  it("closes a task with its workers after asking once; finished workers stay Done", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Write the notes", { state: "idle" });
    for (const name of ["docs", "lint"]) {
      const worker = core.addSession("p1", `${name} notes`, { state: "stopped" });
      core.transcripts.set(worker.id, [
        { kind: "turn_end", usage: { input_tokens: 1, output_tokens: 1, cost_micro_usd: null } },
      ]);
    }
    core.tasks.push({
      id: "t1",
      project: "p1",
      coordinator: "s1",
      workers: ["s2", "s3"].map((session, n) => ({
        session,
        name: n === 0 ? "docs" : "lint",
        branch: `ovra/t1/${n}`,
        worktree: `/wt/${n}`,
        kept: null,
      })),
      limits: { max_workers: 3, budget_micro_usd: null },
      spent_micro_usd: 0,
    });
    // lint's newest item is decided, an older one still waits: the inbox counts it, so does the row.
    core.reviews = [
      { ...review("s3", []), id: "new", state: "approved" },
      review("s3", [file("NOTES.md", "agent")]),
      review("s2", [file("NOTES.md", "agent")]),
    ];
    await setup(core);
    const rows = () =>
      [...target.querySelectorAll(".sessions .row-button")].map((b) => b.textContent?.replace(/\s+/g, " ").trim());
    // One Review on the task, with how many; its workers do not repeat it.
    expect(rows()).toEqual([
      "Write the notes Done 2 of 2 done Review 2",
      "docs · docs notes Done",
      "lint · lint notes Done",
    ]);
    target.querySelector<HTMLButtonElement>("[aria-label='Close Task: Write the notes']")?.click();
    await settle();
    expect(core.sent("agents.stop")).toEqual([]);
    expect(target.querySelector("[data-testid=agents-close-confirm]")?.textContent?.replace(/\s+/g, " ").trim()).toBe(
      "Close this task and its 2 workers? Their changes stay in Reviews. Cancel Close task",
    );
    button("Close task")?.click();
    await settle();
    expect(core.sent("agents.stop").map((c) => c.params)).toEqual([{ session: "s1" }]);
    expect(rows()).toEqual([]);
  });

  it("shows a question for the person quietly on the task's row and Board, and answers it there", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Rename the flag", { state: "idle" });
    core.tasks.push({
      id: "t1",
      project: "p1",
      coordinator: "s1",
      workers: [],
      limits: { max_workers: 3, budget_micro_usd: null },
      spent_micro_usd: 0,
    });
    const lint = { kind: "worker", name: "lint" } as const;
    core.boards.set("t1", [
      { id: "b1", author: lint, body: { kind: "claim", worker: "lint", paths: ["src/flag.ts"], note: "" } },
    ]);
    await setup(core);
    const row = () => target.querySelector(".sessions .row-button")?.textContent?.replace(/\s+/g, " ").trim();
    expect(row()).toBe("Rename the flag Done");
    // A worker asks the person: the row says so, nothing pops up.
    const question: BoardEntry = {
      id: "b2",
      author: lint,
      body: { kind: "question", to: { kind: "person" }, text: "Keep the old flag?" },
    };
    core.boards.get("t1")?.push(question);
    core.emit({ type: "board.changed", data: { project: "p1", task: "t1", entry: question } });
    await settle();
    expect(row()).toBe("Rename the flag Done Answer");
    target.querySelector<HTMLButtonElement>(".sessions .row-button")?.click();
    await settle();
    target.querySelector<HTMLButtonElement>("[data-testid=agent-board-tab]")?.click();
    await settle();
    expect(target.querySelector("[data-testid=agent-board-tab]")?.textContent).toBe("Board (1)");
    // The task working again keeps the Board open.
    core.status("s1", { state: "working" });
    await settle();
    expect(target.querySelector("[data-testid=task-board]")).not.toBeNull();
    // Left and Right move between the tabs.
    target
      .querySelector<HTMLElement>("[role=tablist]")
      ?.dispatchEvent(
        new KeyboardEvent("keydown", { code: "ArrowLeft", key: "ArrowLeft", bubbles: true, cancelable: true }),
      );
    await settle();
    expect(target.querySelector("[data-testid=task-board]")).toBeNull();
    expect(document.activeElement?.id).toBe("task-tab-talk");
    target.querySelector<HTMLButtonElement>("[data-testid=agent-board-tab]")?.click();
    await settle();
    expect(target.querySelector("[data-testid=board-summary]")?.textContent).toBe(
      "lint holds 1 path · 1 question for you",
    );
    expect(
      [...target.querySelectorAll("[data-testid=board-entry]")].map((e) => e.textContent?.replace(/\s+/g, " ").trim()),
    ).toEqual(["lint · Takes src/flag.ts", "lint · Asks you Keep the old flag?"]);
    type(target.querySelector<HTMLTextAreaElement>("[data-testid=board-question] textarea"), "No, drop it.");
    flushSync();
    target.querySelector<HTMLButtonElement>("[data-testid=board-question] button")?.click();
    await settle();
    expect(core.sent("board.answer").map((c) => c.params)).toEqual([
      { task: "t1", question: "b2", text: "No, drop it." },
    ]);
    expect(target.querySelector("[data-testid=board-question]")).toBeNull();
    expect(target.querySelector("[data-testid=agent-board-tab]")?.textContent).toBe("Board");
    // The person's own note goes on the board too.
    type(target.querySelector<HTMLTextAreaElement>(".board .note textarea"), "Keep the tests fast.");
    target
      .querySelector<HTMLTextAreaElement>(".board .note textarea")
      ?.dispatchEvent(new KeyboardEvent("keydown", { code: "Enter", key: "Enter", bubbles: true, cancelable: true }));
    await settle();
    expect(core.sent("board.post").map((c) => c.params)).toEqual([{ task: "t1", text: "Keep the tests fast." }]);
    expect(target.querySelectorAll("[data-testid=board-entry]")).toHaveLength(4);
  });

  it("marks files two workers claim or change on the Board tab and lists who changes what", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Rename the flag", { state: "idle" });
    for (const name of ["docs", "lint"]) core.addSession("p1", `${name} work`, { state: "idle" });
    core.tasks.push({
      id: "t1",
      project: "p1",
      coordinator: "s1",
      workers: ["s2", "s3"].map((session, n) => ({
        session,
        name: n === 0 ? "docs" : "lint",
        branch: `ovra/t1/${n}`,
        worktree: `/wt/${n}`,
        kept: null,
      })),
      limits: { max_workers: 3, budget_micro_usd: null },
      spent_micro_usd: 0,
    });
    core.boards.set("t1", [
      {
        id: "b1",
        author: { kind: "worker", name: "lint" },
        body: { kind: "claim", worker: "lint", paths: ["src/**"], note: "" },
      },
    ]);
    core.reviews = [{ ...review("s2", [file("src/flag.ts", "agent"), file("README.md", "agent")]), mode: "isolated" }];
    await setup(core);
    target.querySelector<HTMLButtonElement>(".sessions .row-button")?.click();
    await settle();
    expect(target.querySelector("[data-testid=agent-board-tab]")?.textContent).toBe("Board · 1 overlap");
    target.querySelector<HTMLButtonElement>("[data-testid=agent-board-tab]")?.click();
    await settle();
    expect(target.querySelector<HTMLDetailsElement>("[data-testid=board-who]")?.open).toBe(true);
    expect(
      [...target.querySelectorAll("[data-testid=board-who-row]")].map((e) =>
        e.textContent?.replace(/\s+/g, " ").trim(),
      ),
    ).toEqual(["src/flag.ts lint claims it · docs changed it · Overlap", "README.md docs changed it"]);
  });

  it("answers a question with options in one tap and marks the recommended one", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Rename the flag", { state: "idle" });
    core.tasks.push({
      id: "t1",
      project: "p1",
      coordinator: "s1",
      workers: [],
      limits: { max_workers: 3, budget_micro_usd: null },
      spent_micro_usd: 0,
    });
    core.boards.set("t1", [
      {
        id: "b1",
        author: { kind: "worker", name: "lint" },
        body: {
          kind: "question",
          to: { kind: "person" },
          text: "Keep the old flag?",
          options: ["Keep", "Drop"],
          recommended: 1,
          reason: "Nobody uses it",
        },
      },
    ]);
    await setup(core);
    target.querySelector<HTMLButtonElement>(".sessions .row-button")?.click();
    await settle();
    target.querySelector<HTMLButtonElement>("[data-testid=agent-board-tab]")?.click();
    await settle();
    const options = [...target.querySelectorAll<HTMLButtonElement>("[data-testid=board-option]")];
    expect(options.map((o) => o.textContent)).toEqual(["Keep", "Drop (recommended)"]);
    expect(target.querySelector(".board .why")?.textContent).toBe("Nobody uses it");
    options[1]?.click();
    await settle();
    expect(core.sent("board.answer").map((c) => c.params)).toEqual([{ task: "t1", question: "b1", text: "Drop" }]);
    expect(target.querySelector("[data-testid=board-question]")).toBeNull();
  });

  it("never calls a task done when its workers failed, and says why one did not start", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Write the notes", { state: "idle" });
    core.addSession("p1", "docs notes", { state: "failed", message: "claude was not found" });
    core.tasks.push({
      id: "t1",
      project: "p1",
      coordinator: "s1",
      workers: [{ session: "s2", name: "docs", branch: "ovra/t1/docs", worktree: "/wt/docs", kept: null }],
      failed: [{ name: "lint", reason: "the worktree could not be made" }],
      limits: { max_workers: 3, budget_micro_usd: null },
      spent_micro_usd: 0,
    });
    await setup(core);
    const rows = [...target.querySelectorAll(".sessions li")].map((li) => li.textContent?.replace(/\s+/g, " ").trim());
    expect(rows).toEqual([
      "Write the notes Failed: 2 of 2 workers 0 of 2 done",
      "lint Did not start: the worktree could not be made",
      "docs · docs notes Failed: claude was not found",
    ]);
  });

  it("names the provider from agents.providers where it is about one agent, and says Reply otherwise", async () => {
    const core = new FakeAgentCore();
    core.providers = core.providers.map((p) => ({ ...p, name: "Claude beta" }));
    core.addSession("p1", "Fix it", { state: "working" });
    core.transcripts.set("s1", [{ kind: "assistant_text", text: "Looking." }]);
    await setup(core);
    expect(target.querySelector("[data-testid=agents-footer]")?.textContent).toBe("Claude beta · Claude plan");
    selectRow("Fix it");
    await settle();
    expect(transcript()).toEqual(["Claude beta Looking."]);
    const reply = () => target.querySelector<HTMLTextAreaElement>("#agent-reply");
    expect(reply()?.placeholder).toBe("Claude beta is working… A reply waits for it");
    expect(target.querySelector("label[for=agent-reply]")?.textContent).toBe("Reply");
    core.status("s1", { state: "idle" });
    await settle();
    expect(reply()?.placeholder).toBe("Reply");
  });

  it("starts a session in the open project and streams its transcript", async () => {
    const core = await setup();
    type(target.querySelector("#agent-prompt"), "Fix the typo\nin README.md");
    modEnter();
    await settle();
    expect(core.sent("agents.start").map((c) => c.params)).toEqual([
      { project: "p1", agent: "claude", mode: "direct", account: null, prompt: "Fix the typo\nin README.md" },
    ]);
    // It opens, ready for a reply.
    expect(document.activeElement?.id).toBe("agent-reply");
    core.status("s1", { state: "working" });
    core.message("s1", { kind: "assistant_text", text: "Looking." });
    core.message("s1", { kind: "tool_use", id: "t1", name: "Bash", input: { command: "ls" } });
    core.message("s1", { kind: "tool_result", tool_use_id: "t1", output: "README.md", is_error: false, parent: "t0" });
    core.message("s1", {
      kind: "turn_end",
      usage: { input_tokens: 10, output_tokens: 5, cost_micro_usd: 1200 },
    });
    await settle();
    expect(transcript()).toEqual([
      "You Fix the typo in README.md",
      "Claude Looking.",
      // One line per step; what it ran and got opens on demand.
      "Subagent: Ran ls Command: ls Output, 1 line: README.md",
      "Turn ended · 15 tokens",
    ]);
    expect(panel()?.querySelector("[role=status]")?.textContent).toBe("Working");
    // Outputs are folded; screen readers hear what Claude says, not tool output.
    expect(panel()?.querySelector("[data-testid=transcript] details")?.hasAttribute("open")).toBe(false);
    expect(target.querySelector(".spoken")?.textContent).toBe("Claude: Looking.");
    expect(panel()?.querySelector("[data-testid=agent-usage]")?.textContent).toBe(
      "Total: 10 tokens in, 5 out, $0.0012",
    );
    expect(panel()?.querySelector("[data-testid=agent-billing]")).toBeNull();
    // A plan is no news: it waits under Details, and the name shares its line with the status.
    core.emit({ type: "agents.billing", data: { session: "s1", billing: { kind: "subscription" } } });
    await settle();
    expect(panel()?.querySelector("[data-testid=agent-billing]")).toBeNull();
    expect(panel()?.querySelector("details.more [data-testid=agent-billing-more]")).not.toBeNull();
    expect(panel()?.querySelector(".title-row [role=status]")).not.toBeNull();
    core.emit({ type: "agents.billing", data: { session: "s1", billing: { kind: "api_key" } } });
    await settle();
    const billed = panel()?.querySelector("[data-testid=agent-billing]");
    expect(billed?.textContent?.trim()).toBe("API key, at API prices");
    expect(billed?.classList.contains("warn")).toBe(true);
  });

  it("says whether a session runs in a sandbox, in one short line, once per screen", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Earlier task", { state: "stopped" });
    await setup(core);
    const line = () => panel()?.querySelector("[data-testid=agent-sandbox]");
    const more = () => panel()?.querySelector("[data-testid=agent-sandbox-more]")?.textContent;
    const gaps = () => panel()?.querySelector("[data-testid=agent-sandbox-gaps]")?.textContent;
    const once = () => target.querySelector("[data-testid=agents-no-sandbox]")?.textContent;
    const hint = () => target.querySelector("#agent-mode-hint")?.textContent;
    const said = () => (panel()?.textContent?.match(/No sandbox/g) ?? []).length;
    // Saved before Ovra recorded the report: nothing to say.
    selectRow("Earlier task");
    expect(line()).toBeNull();
    back();
    type(target.querySelector("#agent-prompt"), "Fix the typo");
    modEnter();
    await settle();
    expect(line()?.textContent).toBe("Sandbox: starting…");
    core.sandboxed("s2", null);
    await settle();
    expect(line()?.textContent?.trim()).toBe("No sandbox: the agent can change anything you can");
    // A security fact, set apart from "Needs approval" yellow.
    expect(line()?.classList.contains("security")).toBe(true);
    expect(line()?.classList.contains("warn")).toBe(false);
    expect(said()).toBe(1);
    back();
    // In the same words under the header, not on every row nor again in the mode hint.
    expect(once()?.trim()).toBe("No sandbox: the agent can change anything you can");
    expect(hint()).toBe("Works in this folder. Asks before risky steps.");
    expect(said()).toBe(1);
    selectRow("Fix the typo");
    core.sandboxed("s2", { name: "bubblewrap, Landlock, seccomp", network: "proxy", gaps: [] });
    await settle();
    expect(line()?.textContent?.trim()).toBe("Sandboxed");
    expect(line()?.classList.contains("security")).toBe(false);
    // The whole sentence and the gaps are a click away, never gone.
    expect(more()).toBe("Runs in a sandbox (bubblewrap, Landlock, seccomp), network only through Ovra's proxy.");
    expect(gaps()).toBeUndefined();
    core.sandboxed("s2", {
      name: "fake sandbox",
      network: "proxy",
      gaps: ["DNS lookups", "loopback services"],
    });
    await settle();
    expect(line()?.textContent?.trim()).toBe("Sandboxed · some gaps (see Details)");
    expect(gaps()).toBe("Not covered: DNS lookups, loopback services.");
    core.status("s2", { state: "stopped" });
    await settle();
    expect(gaps()).toBe("Did not cover: DNS lookups, loopback services.");
    expect(more()).toBe("Ran in a sandbox (fake sandbox), network only through Ovra's proxy.");
  });

  it("forgets the sandbox report when a session starts again", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Earlier task", { state: "idle" });
    await setup(core);
    selectRow("Earlier task");
    const line = () => panel()?.querySelector("[data-testid=agent-sandbox]")?.textContent?.trim();
    core.sandboxed("s1", null);
    await settle();
    expect(line()).toBe("No sandbox: the agent can change anything you can");
    // A restart clears the report in the core and sends only a status.
    core.status("s1", { state: "starting" });
    await settle();
    expect(line()).toBe("Sandbox: starting…");
  });

  it("leaves focus where the person moved it while the session was starting", async () => {
    const core = new FakeAgentCore();
    await setup(core);
    const elsewhere = document.body.appendChild(document.createElement("button"));
    const call = core.call.bind(core);
    core.call = (async (request: Parameters<typeof call>[0]) => {
      // The person moves on (to an approval, Reviews) before the start answers.
      if (request.method === "agents.start") elsewhere.focus();
      return call(request);
    }) as typeof core.call;
    type(target.querySelector("#agent-prompt"), "Fix the typo");
    modEnter();
    await settle();
    expect(core.sent("agents.start")).toHaveLength(1);
    expect(document.activeElement).toBe(elsewhere);
    elsewhere.remove();
  });

  it("keeps a sandbox report that came before the reply to agents.start", async () => {
    const core = new FakeAgentCore();
    await setup(core);
    const call = core.call.bind(core);
    core.call = (async (request: Parameters<typeof call>[0]) => {
      const reply = await call(request);
      if (request.method === "agents.start") core.sandboxed("s1", { name: "bwrap", network: "proxy", gaps: [] });
      return reply;
    }) as typeof core.call;
    type(target.querySelector("#agent-prompt"), "Fix the typo");
    modEnter();
    await settle();
    expect(panel()?.querySelector("[data-testid=agent-sandbox-more]")?.textContent).toBe(
      "Runs in a sandbox (bwrap), network only through Ovra's proxy.",
    );
  });

  it("keeps the session's title to one line, whole on hover", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the typo in README.md with the Edit tool and then run the tests", { state: "idle" });
    await setup(core);
    selectRow("Fix the typo in README.md with the Edit tool and then run the tests");
    const title = panel()?.querySelector("[data-testid=agent-session] h3");
    expect(title?.getAttribute("title")).toBe("Fix the typo in README.md with the Edit tool and then run the tests");
    expect(title?.classList.contains("clamp")).toBe(true);
  });

  it("marks the cost as an API price estimate on a Claude plan", async () => {
    const core = new FakeAgentCore();
    const info = core.addSession("p1", "Earlier task", { state: "idle" });
    info.billing = { kind: "subscription" };
    info.usage = { input_tokens: 64044, output_tokens: 372, cost_micro_usd: 2400 };
    // A turn without usage has nothing to say: no "0 tokens" line.
    const none = { input_tokens: 0, output_tokens: 0, cost_micro_usd: null };
    core.transcripts.set("s1", [
      { kind: "turn_end", usage: none },
      { kind: "turn_end", usage: info.usage },
    ]);
    await setup(core);
    selectRow("Earlier task");
    const estimate = "64,044 tokens in, 372 out, about $0.0024 at API prices (your plan's limits, not this amount)";
    expect(panel()?.querySelector("[data-testid=agent-usage]")?.textContent).toBe(`Total: ${estimate}`);
    // Tokens on a plan; the estimate in dollars only on hover.
    expect(transcript()).toEqual(["Turn ended · 64.4k tokens"]);
    expect(target.querySelector("[data-testid=transcript] .divider")?.getAttribute("title")).toBe(estimate);
  });

  it("names who wrote each message an agent was sent", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Write the notes", { state: "idle" });
    core.addSession("p1", "Write the docs notes.", { state: "idle" });
    core.tasks.push({
      id: "t1",
      project: "p1",
      coordinator: "s1",
      workers: [{ session: "s2", name: "docs", branch: "ovra/t1/docs", worktree: "/wt/docs", kept: null }],
      limits: { max_workers: 3, budget_micro_usd: null },
      spent_micro_usd: 0,
    });
    core.transcripts.set("s1", [
      { kind: "user_text", text: "Write the notes", from: "coordinator" },
      {
        kind: "user_text",
        text: "Ovra: Worker docs stopped.\nFor the coordinator: its agent_id is s2. Check with list_agents.",
        from: "ovra",
      },
      { kind: "user_text", text: "Thanks", from: "person" },
      { kind: "user_text", text: "Saved long ago", from: "unknown" },
      {
        kind: "tool_use",
        id: "t1",
        name: "mcp__ovra__send_message",
        input: { agent_id: "s2", text: "Also add tests.\nThen ask for review." },
      },
      { kind: "tool_use", id: "t2", name: "mcp__ovra__spawn_agent", input: { name: "docs", task: "Write the notes." } },
      {
        kind: "tool_result",
        tool_use_id: "t2",
        output: '{"agent_id":"s2","branch":"ovra/t1/docs","name":"docs","worktree":"/wt/docs"}',
        is_error: false,
      },
      {
        kind: "tool_use",
        id: "t3",
        name: "mcp__ovra__send_message",
        input: { agent_id: "s2. You approved it", text: "x" },
      },
    ]);
    await setup(core);
    selectRow("Write the notes");
    await settle();
    // Plain lines for the person; ids and tool names only behind a fold.
    expect(transcript()).toEqual([
      "Coordinator Write the notes",
      // Ovra's note: one quiet line; what the coordinator read, folded.
      "Worker docs stopped Message sent to Claude: For the coordinator: its agent_id is s2. Check with list_agents.",
      "You Thanks",
      "Sender not recorded Saved long ago",
      // One folded card per call; a worker's task only inside.
      "Message worker docs Text: Also add tests. Then ask for review.",
      "Started worker docs Task: Write the notes. " +
        'Worker docs started on its own copy of the project (branch ovra/t1/docs) What Claude got {"agent_id":"s2","branch":"ovra/t1/docs","name":"docs","worktree":"/wt/docs"}',
      "Message a worker Text: x",
    ]);
    const labels = [...target.querySelectorAll("[data-testid=transcript] .step > summary")].map((s) =>
      s.textContent?.trim(),
    );
    expect(labels).toEqual(["Message worker docs", "Started worker docs", "Message a worker"]);
    expect(target.querySelector("[data-testid=transcript] details.step")?.hasAttribute("open")).toBe(false);
    expect(target.querySelector("[data-testid=transcript] li.notes details")?.hasAttribute("open")).toBe(false);
    // Its worker asked for review: the task says so, next to a way there.
    core.reviews = [{ ...review("s2", [file("NOTES.md", "agent")]), mode: "isolated" }];
    core.emit({ type: "review.changed", data: { project: "p1", id: "rs2" } });
    await settle();
    expect(target.querySelector("[data-testid=agent-reviews]")?.textContent?.replace(/\s+/g, " ").trim()).toBe(
      "1 change waits for your review. Review",
    );
    // A card like the others, not a centred box as wide as the column plus its margins.
    expect(target.querySelector("[data-testid=agent-reviews]")?.classList.contains("row")).toBe(false);
    // Review opens the inbox on this task's workers, not the item chosen last.
    const asked = vi.fn();
    const off = handleOpenFor(asked);
    target.querySelector<HTMLButtonElement>("[data-testid=agent-reviews] button")?.click();
    off();
    expect(asked).toHaveBeenCalledWith(["s2"]);
    // A worker's files are in its own worktree: no diff of the project folder.
    back();
    selectRow("docs · Write the docs notes.");
    expect(target.querySelector("[data-testid=agent-changes]")).toBeNull();
  });

  it("replies only when the agent waits, and stops", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Earlier task", { state: "working" });
    core.transcripts.set("s1", [{ kind: "user_text", text: "Earlier task", from: "person" }]);
    await setup(core);
    selectRow("Earlier task");
    await settle();
    expect(transcript()).toEqual(["You Earlier task"]);
    const send = () => [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Send");
    const button = (name: string) =>
      [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === name);
    // Both say what they do before the press.
    // One plain Stop is the safe one; both say what they do before the press.
    expect([button("Stop")?.title, button("End task")?.title]).toEqual([
      "Stops this step; you can reply",
      "Ends the session",
    ]);
    button("Stop")?.click();
    await settle();
    expect(core.sent("agents.interrupt").map((c) => c.params)).toEqual([{ session: "s1" }]);
    type(target.querySelector("#agent-reply"), "and the docs");
    await settle();
    // While it works, a reply waits in its queue instead.
    expect(send()).toBeUndefined();
    expect(button("Queue")?.disabled).toBe(false);
    core.status("s1", { state: "idle" });
    await settle();
    expect(send()?.disabled).toBe(false);
    expect(button("Stop")).toBeUndefined();
    // Done: nothing to stop; Close task is there and names the task.
    expect(button("End task")).toBeUndefined();
    expect(button("Close task")?.getAttribute("aria-label")).toBe("Close task Earlier task");
    modEnter();
    await settle();
    expect(core.sent("agents.send").map((c) => c.params)).toEqual([{ session: "s1", text: "and the docs" }]);
    core.status("s1", { state: "working" });
    await settle();
    button("End task")?.click();
    await settle();
    expect(core.sent("agents.stop")).toHaveLength(1);
    core.status("s1", { state: "failed", message: "claude exited with code 1" });
    await settle();
    expect(panel()?.querySelector("[role=status]")?.textContent).toBe("Failed: claude exited with code 1");
    expect(target.querySelector("#agent-reply")).toBeNull();
    // Closed: off the list, in this browser for good; the core keeps it.
    button("Close task")?.click();
    await settle();
    expect(panel()?.querySelector("[data-testid=agents-empty]")).not.toBeNull();
    expect(localStorage.getItem("ovra.agents.closed.p1")).toBe(JSON.stringify(["s1"]));
  });

  it("shows the elements picked in the session's browser and attaches them to the reply", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the header", { state: "idle" });
    await setup(core);
    selectRow("Fix the header");
    const pick = (tag: string, locator: string): Pick => ({
      pick_id: `pk-${tag}`,
      url: "http://localhost:5173/",
      selector: `header > ${tag}`,
      locator,
      tag,
      text: "",
      html: "",
      rect: { x: 0, y: 0, width: 10, height: 10 },
    });
    const picks = browserPicks(core);
    picks.add("s1", "pg1", pick("button", 'button "Save"'));
    picks.add("s1", "pg1", pick("h1", 'heading "Welcome"'));
    picks.note("s1", "pk-button", "make it blue");
    await settle();
    const chips = () => [...target.querySelectorAll("[data-testid=pick-chip] span")].map((c) => c.textContent);
    expect(chips()).toEqual(['button "Save"', 'heading "Welcome"']);
    target.querySelector<HTMLButtonElement>("button[aria-label='Remove heading \"Welcome\"']")?.click();
    await settle();
    expect(chips()).toEqual(['button "Save"']);
    type(target.querySelector("#agent-reply"), "this one");
    modEnter();
    await settle();
    // The picks go first: the session's next message carries them.
    expect(core.calls.filter((c) => c.method === "browser.attach" || c.method === "agents.send")).toEqual([
      {
        method: "browser.attach",
        params: { page: "pg1", picks: [{ ...pick("button", 'button "Save"'), note: "make it blue" }] },
      },
      { method: "agents.send", params: { session: "s1", text: "this one" } },
    ]);
    expect(chips()).toEqual([]);
  });

  it("resumes an ended session that can be resumed", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Earlier task", { state: "working" });
    core.transcripts.set("s1", [{ kind: "user_text", text: "Earlier task", from: "person" }]);
    await setup(core);
    selectRow("Earlier task");
    const info = core.sessions[0];
    if (info) info.resumable = true;
    core.status("s1", { state: "stopped" });
    await settle();
    type(target.querySelector("#agent-resume"), "Carry on");
    modEnter();
    await settle();
    expect(core.sent("agents.resume").map((c) => c.params)).toEqual([{ session: "s1", prompt: "Carry on" }]);
    expect(panel()?.querySelector("[role=status]")?.textContent).toBe("Starting");
    expect(transcript()).toEqual(["You Earlier task", "You Carry on"]);
    expect(target.querySelector("#agent-resume")).toBeNull();
    expect(target.querySelector("#agent-reply")).not.toBeNull();
  });

  it("says plainly that a tool call was denied, and re-reads everything after a resync", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Earlier task", { state: "working" });
    core.transcripts.set("s1", [{ kind: "user_text", text: "Earlier task", from: "person" }]);
    await setup(core);
    selectRow("Earlier task");
    await settle();
    core.message("s1", {
      kind: "tool_result",
      tool_use_id: "t1",
      output: "The user doesn't want to proceed with this tool use. STOP what you are doing.",
      is_error: true,
    });
    await settle();
    expect(transcript()).toEqual(["You Earlier task", "Not run: you denied it or interrupted the turn"]);
    // Events were dropped: the core's transcript is the truth.
    core.transcripts.set("s1", [
      { kind: "user_text", text: "Earlier task", from: "person" },
      { kind: "assistant_text", text: "Missed" },
    ]);
    core.emit({ type: "info.resync", data: { skipped: 3 } });
    await settle();
    await settle();
    expect(transcript()).toEqual(["You Earlier task", "Claude Missed"]);
  });

  it("folds the start box to one line while requests wait, and opens it on focus", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the typo", { state: "waiting_for_approval" });
    await setup(core);
    const form = () => target.querySelector<HTMLFormElement>("form.new");
    const field = () => target.querySelector<HTMLTextAreaElement>("#agent-prompt");
    expect(form()?.classList.contains("folded")).toBe(false);
    core.ask({ id: "perm1", session: "s1", project: "p1", tool: "Bash", input: { command: "ls" }, title: null });
    await settle();
    // One line: the field alone; the list and the requests keep the room.
    expect(form()?.classList.contains("folded")).toBe(true);
    // The card explains itself: no extra Allow/Deny line takes the column's room.
    expect(target.querySelector("[data-testid=approval-hint]")).toBeNull();
    expect(field()?.rows).toBe(1);
    expect(startButton()).toBeUndefined();
    field()?.focus();
    field()?.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    flushSync();
    expect(form()?.classList.contains("folded")).toBe(false);
    expect(startButton()).toBeDefined();
    // Typed text keeps it open after focus leaves.
    type(field(), "Also the docs");
    field()?.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    flushSync();
    expect(form()?.classList.contains("folded")).toBe(false);
  });

  it("opens links the agent wrote: web pages in the browser, project files in the editor", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the typo", { state: "idle" });
    await setup(core);
    selectRow("Fix the typo");
    await settle();
    core.message("s1", {
      kind: "assistant_text",
      text: "[docs](https://example.com/d) [main](src/main.rs:12) [bad](javascript:alert(1)) [etc](/etc/passwd)",
    });
    await settle();
    const link = (name: string) =>
      [...target.querySelectorAll<HTMLAnchorElement>("[data-testid=agent-text] a")].find((a) => a.textContent === name);
    for (const name of ["bad", "etc"]) link(name)?.click();
    expect(core.sent("open.url")).toEqual([]);
    expect(workspace.openPath).toBeNull();
    link("docs")?.click();
    expect(core.sent("open.url").map((c) => c.params.url)).toEqual(["https://example.com/d"]);
    link("main")?.click();
    expect([workspace.openPath, workspace.reveal?.line]).toEqual(["src/main.rs", 12]);
  });

  it("says how many messages came while scrolled up, under the transcript rather than over it", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the typo", { state: "working" });
    core.transcripts.set("s1", [{ kind: "user_text", text: "Fix the typo", from: "person" }]);
    await setup(core);
    selectRow("Fix the typo");
    await settle();
    const log = target.querySelector<HTMLElement>("[data-testid=transcript]");
    if (!log) throw new Error("no log");
    Object.defineProperty(log, "scrollHeight", { value: 1000 });
    Object.defineProperty(log, "clientHeight", { value: 200 });
    log.scrollTop = 100;
    log.dispatchEvent(new Event("scroll"));
    flushSync();
    const jump = () => target.querySelector<HTMLButtonElement>("[data-testid=jump-to-latest]");
    expect(jump()?.getAttribute("aria-label")).toBe("Jump to latest");
    core.message("s1", { kind: "assistant_text", text: "Looking." });
    core.message("s1", { kind: "assistant_text", text: "Found it." });
    await settle();
    expect(jump()?.getAttribute("aria-label")).toBe("2 new · Jump to latest");
    // Its own row after the log, so it never covers a line.
    expect(jump()?.previousElementSibling).toBe(log);
    jump()?.click();
    flushSync();
    expect(jump()).toBeNull();
  });

  it("marks a step the person denied with a message as Denied, from the answer itself", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the typo", { state: "working" });
    core.transcripts.set("s1", []);
    await setup(core);
    selectRow("Fix the typo");
    await settle();
    const input = { file_path: "/demo/README.md", old_string: "teh", new_string: "the" };
    core.message("s1", { kind: "tool_use", id: "t1", name: "Edit", input });
    core.ask({ id: "perm1", session: "s1", project: "p1", tool: "Edit", input, title: null });
    core.emit({ type: "permissions.resolved", data: { id: "perm1", decision: "deny" } });
    // The agent got the person's message, not the CLI's words for a denial.
    core.message("s1", { kind: "tool_result", tool_use_id: "t1", output: "Only touch the docs", is_error: true });
    await settle();
    const step = target.querySelector("[data-testid=transcript] .step");
    expect(step?.querySelector("summary")?.textContent?.replace(/\s+/g, " ").trim()).toBe("Edit README.md Denied");
    expect(step?.classList.contains("failed")).toBe(false);
  });

  it("says next to the form when Claude fails right after starting, and points to the setup", async () => {
    const core = await setup();
    type(target.querySelector("#agent-prompt"), "Fix the typo");
    flushSync();
    startButton()?.click();
    await settle();
    const alert = () => panel()?.querySelector("[data-testid=agent-start-error]")?.textContent?.replace(/\s+/g, " ");
    core.status("s1", { state: "working" });
    await settle();
    core.status("s1", { state: "failed", message: "later" });
    await settle();
    // Failing after it got going is not a start problem.
    back();
    expect(alert()).toBeUndefined();
    type(target.querySelector("#agent-prompt"), "Again");
    modEnter();
    await settle();
    core.status("s2", { state: "failed", message: "claude was not found." });
    await settle();
    expect(alert()?.trim()).toBe("Claude could not start: claude was not found. Check Claude");
    // It opens Claude's setup, though the last check said Claude is ready.
    expect(target.querySelector("[data-testid=claude-setup]")).toBeNull();
    button("Check Claude")?.click();
    await settle();
    expect(target.querySelector("[data-testid=claude-setup] h3")?.textContent).toBe("Claude 2.1.331 is ready");
    // Back at the start box, with the prompt to try again.
    expect(target.querySelector<HTMLTextAreaElement>("#agent-prompt")?.value).toBe("Again");
  });

  it("shows why a session could not start", async () => {
    const core = new FakeAgentCore();
    const { CoreError } = await import("../lib/transport");
    core.failNext.set(
      "agents.start",
      new CoreError({ code: "conflict", message: "another agent already works in this project's main checkout" }),
    );
    await setup(core);
    type(target.querySelector("#agent-prompt"), "Second task");
    modEnter();
    await settle();
    expect(panel()?.querySelector("[role=alert]")?.textContent?.trim()).toBe(
      "Could not start: another agent already works in this project's main checkout",
    );
  });

  it("lists what a direct session changed once its turn is over, each file one press from its diff", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the typo", { state: "working" });
    core.reviews = [
      review("s1", [file("README.md", "agent"), file("notes.txt", "outside"), file("new.md", "agent", "added")]),
    ];
    const toggle = vi.fn();
    const off = registerCommand({ id: "inbox.toggle", title: "Review inbox", run: toggle });
    await setup(core);
    selectRow("Fix the typo");
    const changes = () => target.querySelector("[data-testid=agent-changes]");
    expect(changes()).toBeNull();
    core.status("s1", { state: "idle" });
    await settle();
    expect(changes()?.querySelector("h4")?.textContent).toBe("Changed files (2)");
    const files = [...(changes()?.querySelectorAll<HTMLButtonElement>("button.file") ?? [])];
    expect(files.map((b) => b.textContent)).toEqual(["README.md", "new.md"]);
    files[0]?.click();
    expect(workspace.gitDiff).toEqual({ project: "p1", path: "README.md", staged: false });
    // Git has no diff for a file it does not track yet: it opens instead.
    files[1]?.click();
    expect([workspace.gitDiff, workspace.openPath]).toEqual([null, "new.md"]);
    [...(changes()?.querySelectorAll("button") ?? [])].find((b) => b.textContent === "Review")?.click();
    expect(toggle).toHaveBeenCalledOnce();
    // Kept in Reviews: nothing waits any more.
    core.reviews = [{ ...review("s1", []), state: "approved" }];
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    expect(changes()).toBeNull();
    off();
  });

  it("says when every agent waits for the person, and that One agent, here waits in line at the cap", async () => {
    const core = new FakeAgentCore();
    // Each in its own copy: none has the project's folder, so One agent, here may queue.
    for (const n of [1, 2, 3]) core.addSession("p1", `Task ${n}`, { state: "waiting_for_approval" }).mode = "isolated";
    core.capacity = { working: 3, limit: 3, queued: 0, research_working: 0, research_limit: 2, waiting: [] };
    await setup(core);
    const count = () => target.querySelector("[data-testid=agents-capacity]");
    expect(count()?.textContent?.trim()).toBe("3 need you");
    expect(count()?.getAttribute("title")).toBe("All 3 agents wait for your answer");
    type(target.querySelector("#agent-prompt"), "Fix the typo");
    flushSync();
    // Said before the press: it waits for a place instead of being refused.
    expect(startButton()?.disabled).toBe(false);
    expect(startButton()?.title).toBe("Starts when an agent finishes");
    expect(target.querySelector("[data-testid=agent-at-cap]")?.textContent?.replace(/\s+/g, " ").trim()).toBe(
      "All 3 agents are busy. This starts when one finishes.",
    );
    core.status("s1", { state: "working" });
    await settle();
    expect(count()?.textContent?.trim()).toBe("3 of 3");
  });

  it("says before Start that another task has the project's folder, with the way on", async () => {
    const core = new FakeAgentCore();
    const first = core.addSession("p1", "Fix the typo", { state: "working" });
    await setup(core);
    const note = () =>
      target.querySelector("[data-testid=agent-folder-held]")?.textContent?.replace(/\s+/g, " ").trim();
    expect(note()).toBe('"Fix the typo" works in this folder. One task works here at a time. Use Plan and split');
    expect(startButton()?.title).toBe("Another task has this folder");
    core.status(first.id, { state: "idle" });
    await settle();
    expect(note()).toBe('"Fix the typo" still has this folder. Close it to start a new task here. Close it');
    button("Close it")?.click();
    await settle();
    expect(note()).toBeUndefined();
  });

  it("shows a waiting start's place in line, and Cancel takes it out", async () => {
    const core = new FakeAgentCore();
    const first = core.addSession("p1", "First", { state: "queued" });
    const second = core.addSession("p1", "Second", { state: "queued" });
    core.capacity = {
      working: 3,
      limit: 3,
      queued: 2,
      research_working: 0,
      research_limit: 2,
      waiting: [first.id, second.id],
    };
    await setup(core);
    const row = (title: string) =>
      [...target.querySelectorAll<HTMLElement>(".sessions [data-session]")].find((r) => r.textContent?.includes(title));
    expect(row("First")?.textContent).toContain("Waiting, 1st in line");
    expect(row("Second")?.textContent).toContain("Waiting, 2nd in line");
    row("Second")?.click();
    await settle();
    button("Cancel")?.click();
    await settle();
    expect(core.stopped).toContain(second.id);
  });

  it("unfolds a folded Agents column for New task, from its command and Mod+N", async () => {
    await setup();
    const column = document.createElement("div");
    column.dataset.testid = "agents";
    column.innerHTML = '<div class="strip"></div>';
    document.body.append(column);
    const unfold = vi.fn(() => column.querySelector(".strip")?.remove());
    const off = registerCommand({ id: "layout.toggle_agents", title: "t", run: unfold });
    const task = listCommands().find((c) => c.id === "agents.new_session");
    expect(task?.keys).toBe("Mod+N");
    await task?.run();
    await settle();
    expect(unfold).toHaveBeenCalledOnce();
    expect(document.activeElement?.id).toBe("agent-prompt");
    // Unfolded, it stays as it is.
    await task?.run();
    await settle();
    expect(unfold).toHaveBeenCalledOnce();
    off();
    column.remove();
  });

  it("puts New task in Focus's own task box while Focus is on", async () => {
    await setup();
    const box = document.createElement("textarea");
    box.id = "focus-task";
    document.body.append(box);
    focusMode.set(true);
    await listCommands()
      .find((c) => c.id === "agents.new_session")
      ?.run();
    expect(document.activeElement).toBe(box);
    focusMode.set(false);
    box.remove();
  });
});

describe("@ context", () => {
  // The file list is cached per project for the whole run, as in the palette.
  const FILES = [".env", "README.md", "src/lib.rs", "src/main.rs"];
  const field = () => target.querySelector<HTMLTextAreaElement>("#agent-prompt");
  const key = (code: string) =>
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { code, key: code, bubbles: true, cancelable: true }),
    );
  const options = () =>
    [...target.querySelectorAll("[data-testid=context-menu] [role=option] .name")].map((o) => o.textContent);
  /** Lets a chosen row's read, its check and the chip land. */
  const settleReads = async () => {
    for (let i = 0; i < 4; i++) await settle();
  };
  const chips = () =>
    [...target.querySelectorAll("[data-testid=context-chip]")].map((c) => c.textContent?.replace(/\s+/g, " ").trim());

  it("finds a file after @, adds it as a chip and starts the task with it", async () => {
    const core = new FakeAgentCore();
    core.listed = FILES;
    await setup(core);
    type(field(), "Fix @lib");
    await settle();
    expect(options()[0]).toBe("lib.rs");
    expect(field()?.getAttribute("aria-controls")).toMatch(/-menu$/);
    key("Enter");
    await settle();
    // The name stays in the sentence; the chip is said.
    expect(field()?.value).toBe("Fix lib.rs ");
    expect(chips()).toEqual(["lib.rs"]);
    expect(target.querySelector(".ctx-said")?.textContent).toBe("Added lib.rs");
    expect(core.sent("agents.start")).toHaveLength(0);
    // What the box holds is not offered again; nothing that fits says so.
    type(field(), "Fix @lib.rs");
    await settle();
    expect(options()).not.toContain("lib.rs");
    type(field(), "Fix @zzzq");
    await settle();
    expect(target.querySelector("[data-testid=context-menu] .none")?.textContent?.trim()).toBe("No matches");
    type(field(), "Fix ");
    await settle();
    // Escape keeps a typed @ as text; Backspace in an empty box removes the last chip.
    type(field(), "@src");
    await settle();
    key("Escape");
    await settle();
    expect(options()).toEqual([]);
    // Typing more of it opens the menu again, and Tab takes the match.
    type(field(), "@src/ma");
    await settle();
    expect(options()[0]).toBe("main.rs");
    key("Tab");
    await settle();
    expect(chips()).toEqual(["lib.rs", "main.rs"]);
    target.querySelector<HTMLButtonElement>("[aria-label='Remove main.rs']")?.click();
    await settle();
    type(field(), "Fix the parser");
    key("Enter");
    await settle();
    expect(core.sent("agents.start")[0]?.params.context).toEqual([
      { kind: "file", path: "src/lib.rs", confirmed: false },
    ]);
    expect(chips()).toEqual([]);
  });

  it("reads GitHub only after @#, and adds an issue or a failed check's log as a chip", async () => {
    const core = new FakeAgentCore();
    core.listed = FILES;
    core.issues = [
      { number: 12, title: "Login button does nothing", url: "https://github.com/o/r/issues/12", text: "Steps" },
      { number: 9, title: "Dark theme contrast", url: "https://github.com/o/r/issues/9", text: "Hard to read" },
    ];
    core.branchPr = {
      number: 7,
      url: "https://github.com/o/r/pull/7",
      branch: "fix",
      failed: [{ name: "test", state: "failed", url: null, run: 3 }],
    };
    core.logs.set(3, "error: 1 test failed");
    await setup(core);
    // The plain @ menu names the way in last; nothing is read from GitHub yet.
    type(field(), "Fix @");
    await settle();
    expect(options().at(-1)).toBe("GitHub issue or failed check");
    expect(core.sent("pr.issues")).toHaveLength(0);
    // Choosing it types @# and lists the branch's failed checks, then the issues.
    target
      .querySelectorAll<HTMLElement>("[data-testid=context-menu] [role=option]")
      [options().length - 1]?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, cancelable: true }));
    await settleReads();
    expect(options()).toEqual(["test", "#12", "#9"]);
    expect(field()?.value).toBe("Fix @#");
    // Typing filters them; Enter reads the issue and adds it.
    type(field(), "Fix @#login");
    await settle();
    expect(options()).toEqual(["#12"]);
    key("Enter");
    await settleReads();
    expect(chips()).toEqual(["#12"]);
    expect(field()?.value).toBe("Fix #12 ");
    // A failed check's log comes the same way.
    type(field(), "Fix #12 and @#test");
    await settleReads();
    expect(options()[0]).toBe("test");
    key("Enter");
    await settleReads();
    expect(chips()).toEqual(["#12", "test"]);
    // What the box holds is not offered again; gh's error says what to do.
    type(field(), "Fix @#");
    await settleReads();
    expect(options()).toEqual(["#9"]);
    type(field(), "Fix ");
    await settle();
    core.failNext.set("pr.issues", new CoreError({ code: "unavailable", message: "Install GitHub CLI (gh)" }));
    type(field(), "Fix @#");
    await settleReads();
    expect(target.querySelector("[data-testid=context-menu-empty]")?.textContent?.trim()).toBe(
      "Install GitHub CLI (gh)",
    );
    // An issue that cannot be read adds no chip and gives the typed @# back.
    type(field(), "Fix ");
    await settle();
    type(field(), "Fix @#9");
    await settleReads();
    core.failNext.set("pr.issue", new CoreError({ code: "unavailable", message: "gh issue view failed" }));
    key("Enter");
    await settleReads();
    expect(field()?.value).toBe("Fix @#9");
    expect(target.querySelector(".ctx-error")?.textContent).toBe("gh issue view failed");
    expect(chips()).toEqual(["#12", "test"]);
    type(field(), "Fix both");
    key("Enter");
    await settle();
    expect(core.sent("agents.start")[0]?.params.context).toEqual([
      {
        kind: "issue",
        number: 12,
        title: "Login button does nothing",
        url: "https://github.com/o/r/issues/12",
        text: "Steps",
        confirmed: false,
      },
      { kind: "check", pr: 7, name: "test", url: null, text: "error: 1 test failed", confirmed: false },
    ]);
  });

  it("adds files dropped on the box or pasted into it as chips", async () => {
    const core = new FakeAgentCore();
    const drops = new Set<(drop: WindowDrop) => void>();
    Object.assign(core, {
      onDrop: (listener: (drop: WindowDrop) => void) => {
        drops.add(listener);
        return () => drops.delete(listener);
      },
    });
    await setup(core);
    // A file of the project, dropped from the system's file manager onto the task box.
    document.elementFromPoint = () => field();
    for (const listener of drops) listener({ id: "d1", x: 5, y: 5, paths: ["/demo/src/lib.rs"] });
    await settle();
    Reflect.deleteProperty(document, "elementFromPoint");
    expect(chips()).toEqual(["lib.rs"]);
    // A screenshot pasted into the field is saved in the attachments folder.
    const paste = new Event("paste", { bubbles: true, cancelable: true });
    const shot = new File(["png"], "shot.png", { type: "image/png" });
    Object.defineProperty(paste, "clipboardData", { value: { files: [shot], getData: () => "" } });
    field()?.dispatchEvent(paste);
    expect(paste.defaultPrevented).toBe(true);
    await vi.waitFor(async () => {
      await settle();
      expect(chips()).toEqual(["lib.rs", "shot.png"]);
    });
    // It is an image: the chip shows it.
    expect(target.querySelector<HTMLImageElement>("[data-testid=context-thumb]")?.src).toBe(
      "data:image/png;base64,iVBO",
    );
    // The first copy says why the folder is in the project.
    expect(target.querySelector("[data-testid=context-copied-note]")?.textContent).toContain("Git ignores the folder");
    // A key pasted from elsewhere is copied into the project only after Copy
    // anyway; a log pasted with it goes in at once.
    const key2 = new File(["k"], "id_ed25519");
    const log = new File(["l"], "crash.log");
    const paste2 = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(paste2, "clipboardData", { value: { files: [key2, log], getData: () => "" } });
    core.contextNotes.set(".ovra-attachments/id_ed25519", "secret_file");
    field()?.dispatchEvent(paste2);
    await vi.waitFor(async () => {
      await settle();
      expect(target.querySelector("[data-testid=context-copy-ask]")?.textContent).toContain(
        "id_ed25519 may hold secrets",
      );
    });
    await vi.waitFor(() => expect(chips()).toEqual(["lib.rs", "shot.png", "crash.log"]));
    expect(core.calls.filter((c) => c.method === "context.attach")).toHaveLength(2);
    [...target.querySelectorAll<HTMLButtonElement>("[data-testid=context-copy-ask] button")]
      .find((b) => b.textContent === "Skip id_ed25519")
      ?.click();
    await settle();
    expect(target.querySelector("[data-testid=context-copy-ask]")).toBeNull();
    expect(core.calls.filter((c) => c.method === "context.attach")).toHaveLength(2);
    type(field(), "What is wrong here?");
    key("Enter");
    await settle();
    expect(core.sent("agents.start")[0]?.params.context).toEqual([
      { kind: "file", path: "src/lib.rs", confirmed: false },
      { kind: "image", path: ".ovra-attachments/shot.png", confirmed: false },
      { kind: "file", path: ".ovra-attachments/crash.log", confirmed: false },
    ]);
  });

  it("holds a chip that may hold a secret until the person chooses Send anyway", async () => {
    const core = new FakeAgentCore();
    core.listed = FILES;
    core.contextNotes.set(".env", "secret_file");
    await setup(core);
    type(field(), "@.env");
    await settle();
    key("Enter");
    await settle();
    expect(chips()).toEqual([".env May hold secrets Send anyway"]);
    const start = [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Start");
    type(field(), "Use the keys");
    key("Enter");
    await settle();
    expect(core.sent("agents.start")).toHaveLength(0);
    expect(target.textContent).toContain("Choose Send anyway or remove the marked item first.");
    expect(start?.disabled).toBe(true);
    target.querySelector<HTMLButtonElement>("[aria-label='Send .env anyway']")?.click();
    await settle();
    // Focus goes back to the field, so Enter starts the task.
    expect(document.activeElement).toBe(field());
    expect(start?.disabled).toBe(false);
    key("Enter");
    await settle();
    expect(core.sent("agents.start")[0]?.params.context).toEqual([{ kind: "file", path: ".env", confirmed: true }]);
  });

  it("takes Ask agent from another area into the box and focuses it", async () => {
    const core = await setup();
    askAgent({ kind: "terminal", title: "zsh", text: "test failed" });
    await settle();
    expect(chips()).toEqual(["zsh"]);
    expect(document.activeElement).toBe(field());
    // Chips belong to their project's box.
    workspace.project = { id: "p2", name: "other", root: "/other", last_opened_ms: null, trusted: false };
    await settle();
    expect(chips()).toEqual([]);
    workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
    await settle();
    expect(chips()).toEqual(["zsh"]);
    field()?.focus();
    // A held Backspace does not take chips out; one press takes one and says so.
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { code: "Backspace", repeat: true, bubbles: true, cancelable: true }),
    );
    await settle();
    expect(chips()).toEqual(["zsh"]);
    key("Backspace");
    await settle();
    expect(chips()).toEqual([]);
    expect(target.querySelector(".ctx-said")?.textContent).toBe("Removed zsh");
    expect(core.sent("context.check")).toHaveLength(1);
  });
});

function file(path: string, by: FileChange["by"], kind: FileChange["kind"] = "modified"): FileChange {
  return { path, kind, from: null, by, additions: 1, deletions: 1, blocked: false, protected: false };
}

function review(session: string, files: FileChange[]): ReviewItem {
  return {
    id: `r${session}`,
    project: "p1",
    session,
    mode: "direct",
    state: "open",
    summary: null,
    files,
    can_undo: false,
    not_copied: [],
    can_undo_take: false,
    can_undo_discard: false,
    can_undo_keep: false,
    fingerprint: null,
    git_changes: [],
    git_unchecked: false,
  };
}
