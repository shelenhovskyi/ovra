import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HistoryEntry } from "../lib/protocol";
import { mainView } from "../shell/main-view.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { entry, FakeHistoryCore } from "./fake-history";
import TestHost from "./TestHost.svelte";

let app: Record<string, unknown> | null = null;
let target: HTMLElement;

beforeEach(() => {
  vi.useFakeTimers({ now: new Date(2026, 10, 26, 18, 0) });
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  mainView.close(false);
  if (app) unmount(app);
  app = null;
  target.remove();
  vi.useRealTimers();
});

async function settle(ms = 0) {
  flushSync();
  await vi.advanceTimersByTimeAsync(ms);
  flushSync();
}

const today = new Date(2026, 10, 26, 10, 30).getTime();
const yesterday = new Date(2026, 10, 25, 16, 5).getTime();

async function setup(entries: HistoryEntry[]) {
  const core = new FakeHistoryCore();
  core.entries = entries;
  const opened: HistoryEntry[] = [];
  app = mount(TestHost, {
    target,
    props: { transport: core, workspace: new Workspace(), onopen: (e) => opened.push(e) },
  });
  mainView.show("history");
  await settle();
  return { core, opened };
}

const rows = () =>
  [...target.querySelectorAll("[data-testid=history-row]")].map((r) =>
    [...r.children].map((c) => c.textContent?.replace(/\s+/g, " ").trim()).join(" | "),
  );
const headings = () => [...target.querySelectorAll("h3")].map((h) => h.textContent);
const search = () => target.querySelector<HTMLInputElement>("[data-testid=history-search]");

describe("History", () => {
  it("lists every project's tasks by day, searches as the person types and opens a row", async () => {
    const { core, opened } = await setup([
      entry("s2", today, { title: "Fix the login bug", project: "p2" }),
      entry("s1", yesterday, { title: "Write the README", status: { state: "idle" } }),
    ]);
    expect(headings()).toEqual(["Today", "Yesterday"]);
    expect(rows()).toEqual(["Fix the login bug 10:30 | site · Stopped", "Write the README 16:05 | demo · Done"]);

    core.entries[1] = entry("s1", yesterday, { title: "Write the README" }, "Wrote the [install] section");
    const field = search();
    if (!field) throw new Error("no search field");
    field.value = "install";
    field.dispatchEvent(new Event("input"));
    // Typing waits for a pause: one search, not one per key.
    await settle(100);
    expect(core.calls.filter((c) => c.method === "agents.history")).toHaveLength(1);
    await settle(100);
    expect(rows()).toEqual(["Write the README 16:05 | demo · Stopped | Wrote the install section"]);
    expect(target.querySelector("[data-testid=history-snippet] mark")?.textContent).toBe("install");

    target.querySelector<HTMLButtonElement>("[data-testid=history-row]")?.click();
    expect(opened.map((e) => e.session.id)).toEqual(["s1"]);
  });

  it("narrows to one project and says when nothing matches", async () => {
    const { core } = await setup([entry("s1", today, { title: "Fix it", project: "p2" })]);
    const select = target.querySelector<HTMLSelectElement>("[data-testid=history-project]");
    if (!select) throw new Error("no project menu");
    select.value = "p1";
    select.dispatchEvent(new Event("change"));
    await settle(200);
    expect(core.calls.at(-1)).toEqual({ method: "agents.history", params: { query: "", project: "p1" } });
    expect(target.querySelector("[data-testid=history-empty]")?.textContent).toBe("No tasks match.");
  });

  it("says what History is for before any task ran", async () => {
    await setup([]);
    expect(target.querySelector("[data-testid=history-empty] h2")?.textContent).toBe("No past tasks yet");
  });
});
