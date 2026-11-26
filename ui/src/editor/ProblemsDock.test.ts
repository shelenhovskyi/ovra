import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, expect, test } from "vitest";
import { agentAsks } from "../agents/context.svelte";
import { listCommands } from "../keys/commands";
import { Workspace } from "../shell/workspace.svelte";
import { FakeCore } from "./fake-core";
import { problems, problemsDock } from "./lsp/problems.svelte";
import ProblemsHost from "./ProblemsHost.svelte";

let app: Record<string, unknown> | null = null;
const target = document.body.appendChild(document.createElement("div"));

afterEach(() => {
  problems.forget("p1", "s1");
  problemsDock.shown = false;
  if (app) unmount(app);
  app = null;
});

function mountDock(): Workspace {
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: true };
  app = mount(ProblemsHost, { target, props: { transport: new FakeCore(), workspace } });
  flushSync();
  return workspace;
}

const at = (line: number, character: number) => ({
  start: { line, character },
  end: { line, character: character + 3 },
});
const $ = (selector: string) => target.querySelector<HTMLElement>(selector);
const rows = () => [...target.querySelectorAll<HTMLElement>("[data-testid=problem]")];
const press = (code: string) =>
  document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { code, bubbles: true, cancelable: true }));
const showProblems = () =>
  listCommands()
    .find((c) => c.id === "problems.toggle")
    ?.run();

test("an empty list says so, and the status bar shows no counts", async () => {
  mountDock();
  await showProblems();
  await tick();
  expect($("[data-testid=problems]")?.hidden).toBe(false);
  expect($("[data-testid=problems] .empty")?.textContent).toBe("No problems.");
  expect(document.activeElement?.textContent).toBe("Problems");
  expect($("[data-testid=problem-counts]")).toBeNull();
  press("Escape");
  flushSync();
  expect($("[data-testid=problems]")?.hidden).toBe(true);
});

test("the counts open the list; arrows move through it, Enter or a click opens the spot", async () => {
  const workspace = mountDock();
  problems.publish("p1", "s1", "b.ts", [{ range: at(0, 0), severity: 2, message: "unused" }]);
  problems.publish("p1", "s1", "a.ts", [
    { range: at(3, 4), severity: 1, message: "type\nmismatch" },
    { range: at(1, 0), severity: 3, message: "note" },
  ]);
  flushSync();
  const counts = $("[data-testid=problem-counts]");
  expect(counts?.textContent).toBe("1 error, 1 warning");
  counts?.click();
  await tick();
  await tick();
  expect(rows().map((r) => r.textContent?.trim())).toEqual(["type mismatch 4:5", "note 2:1", "unused 1:1"]);
  expect(rows().map((r) => r.tabIndex)).toEqual([0, -1, -1]);
  expect(document.activeElement).toBe(rows()[0]);
  press("ArrowDown");
  press("ArrowDown");
  await tick();
  expect(document.activeElement).toBe(rows()[2]);
  press("Enter");
  expect([workspace.openPath, workspace.mode, workspace.reveal]).toEqual([
    "b.ts",
    "edit",
    { path: "b.ts", line: 1, from: 0, to: 3 },
  ]);
  rows()[1]?.click();
  expect(workspace.reveal).toEqual({ path: "a.ts", line: 2, from: 0, to: 3 });
  // Ask agent hands the problem to a task, without opening it.
  rows()[0]?.querySelector<HTMLElement>("button[aria-label='Ask agent']")?.click();
  expect(agentAsks.waiting.at(-1)).toEqual({
    kind: "problem",
    path: "a.ts",
    line: 4,
    severity: "error",
    message: "type mismatch",
    source: null,
  });
  expect(workspace.reveal?.line).toBe(2);
});
