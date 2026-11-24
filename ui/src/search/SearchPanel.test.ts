import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it } from "vitest";
import type { TextSpot } from "../shell/workspace.svelte";
import { FakeSearchCore, file } from "./fake-search";
import SearchPanel from "./SearchPanel.svelte";

let app: Record<string, unknown> | null = null;
const target = document.body.appendChild(document.createElement("div"));

afterEach(() => {
  if (app) unmount(app);
  app = null;
});

it("shows results grouped by file, highlights matches and opens the chosen one", async () => {
  const core = new FakeSearchCore();
  const opened: [string, TextSpot][] = [];
  app = mount(SearchPanel, {
    target,
    props: { transport: core, project: "p1", onopen: (path: string, at: TextSpot) => opened.push([path, at]) },
  });
  const input = target.querySelector<HTMLInputElement>("input[type=search]");
  if (!input) throw new Error("no search input");
  input.value = "needle";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  // The start's reply passes through a few promises.
  for (let i = 0; i < 4; i++) await Promise.resolve();
  core.results(1, file("src/a.rs", [2, "let needle = 1;", [[4, 10]]]));
  core.results(1, {
    path: "b.md",
    lines: [{ line: 7, text: "a long needle", column: 120, ranges: [[7, 13]] }],
  });
  core.done(1);
  flushSync();

  const summary = target.querySelector("[data-testid=search-summary]")?.textContent;
  expect(summary).toBe("2 matching lines in 2 files.");
  const names = [...target.querySelectorAll(".file-name .name")].map((n) => n.textContent);
  expect(names).toEqual(["b.md", "a.rs"]);
  expect([...target.querySelectorAll("mark")].map((m) => m.textContent)).toEqual(["needle", "needle"]);
  // A cut line starts with "…"; indentation is dropped.
  expect([...target.querySelectorAll(".line .text")].map((t) => t.textContent)).toEqual([
    "…a long needle",
    "let needle = 1;",
  ]);

  const buttons = [...target.querySelectorAll<HTMLButtonElement>("button[data-result]")];
  buttons[0]?.focus();
  buttons[0]?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  expect(document.activeElement).toBe(buttons[1]);
  buttons[1]?.click();
  buttons[0]?.click();
  expect(opened).toEqual([
    ["src/a.rs", { line: 2, from: 4, to: 10 }],
    ["b.md", { line: 7, from: 127, to: 133 }],
  ]);

  const matchCase = target.querySelector<HTMLButtonElement>("button[aria-label='Match case']");
  matchCase?.click();
  flushSync();
  expect(matchCase?.getAttribute("aria-pressed")).toBe("true");
  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  const last = core.calls.at(-1);
  expect(last?.method === "search.start" && last.params.query.case_sensitive).toBe(true);
});
