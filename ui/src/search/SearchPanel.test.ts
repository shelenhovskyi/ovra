import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
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
  // Before a search, the panel says what it searches.
  expect(target.querySelector("section h2")?.textContent).toBe("Search in files");
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
  expect(target.querySelector("section h2")).toBeNull();
  const names = [...target.querySelectorAll(".file-name .name")].map((n) => n.textContent);
  expect(names).toEqual(["b.md", "a.rs"]);
  expect([...target.querySelectorAll("mark")].map((m) => m.textContent)).toEqual(["needle", "needle"]);
  // A cut line starts with "…"; indentation is dropped.
  expect([...target.querySelectorAll(".line .text")].map((t) => t.textContent)).toEqual([
    "…a long needle",
    "let needle = 1;",
  ]);

  // One Tab stop for the results; ArrowDown from the box goes there, Escape comes back.
  const buttons = [...target.querySelectorAll<HTMLButtonElement>("button[data-result]")];
  expect(buttons.map((b) => b.tabIndex)).toEqual([0, -1]);
  input.focus();
  input.dispatchEvent(new KeyboardEvent("keydown", { code: "ArrowDown", key: "ArrowDown", bubbles: true }));
  expect(document.activeElement).toBe(buttons[0]);
  buttons[0]?.dispatchEvent(new KeyboardEvent("keydown", { code: "ArrowDown", key: "ArrowDown", bubbles: true }));
  expect(document.activeElement).toBe(buttons[1]);
  flushSync();
  expect(buttons.map((b) => b.tabIndex)).toEqual([-1, 0]);
  buttons[1]?.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true }));
  expect(document.activeElement).toBe(input);
  buttons[1]?.click();
  buttons[0]?.click();
  expect(opened).toEqual([
    ["src/a.rs", { line: 2, from: 4, to: 10 }],
    ["b.md", { line: 7, from: 127, to: 133 }],
  ]);
  // The opened result stays selected.
  flushSync();
  expect(buttons.map((b) => b.getAttribute("aria-current"))).toEqual(["true", null]);

  const matchCase = target.querySelector<HTMLButtonElement>("button[aria-label='Match case']");
  matchCase?.click();
  flushSync();
  expect(matchCase?.getAttribute("aria-pressed")).toBe("true");
  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  const last = core.calls.at(-1);
  expect(last?.method === "search.start" && last.params.query.case_sensitive).toBe(true);
});

it("replaces all matches after asking, then searches again", async () => {
  const core = new FakeSearchCore();
  core.disk.set("a.txt", "needle needle\n");
  app = mount(SearchPanel, { target, props: { transport: core, project: "p1", onopen: () => {} } });
  const input = target.querySelector<HTMLInputElement>("input[type=search]");
  if (!input) throw new Error("no search input");
  input.value = "needle";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  for (let i = 0; i < 4; i++) await Promise.resolve();
  core.results(
    1,
    file("a.txt", [
      1,
      "needle needle",
      [
        [0, 6],
        [7, 13],
      ],
    ]),
  );
  core.done(1);
  target.querySelector<HTMLButtonElement>("button[aria-label='Replace']")?.click();
  await tick();
  const replace = target.querySelector<HTMLInputElement>("input[aria-label='Replace with']");
  if (!replace) throw new Error("no replace input");
  expect(document.activeElement).toBe(replace);
  replace.value = "pin";
  replace.dispatchEvent(new Event("input", { bubbles: true }));
  replace.dispatchEvent(new KeyboardEvent("keydown", { code: "Enter", key: "Enter", bubbles: true }));
  flushSync();
  const dialog = target.querySelector("[role=alertdialog]");
  expect(dialog?.querySelector("p")?.textContent).toBe("Replace 2 matches in 1 file?");
  expect(document.activeElement?.textContent).toBe("Replace");
  (document.activeElement as HTMLButtonElement).click();
  await vi.waitFor(() =>
    expect(target.querySelector("[data-testid=replace-note]")?.textContent).toBe("Replaced 2 matches in 1 file."),
  );
  expect(core.disk.get("a.txt")).toBe("pin pin\n");
  expect(core.methods().at(-1)).toBe("search.start");
});
