import type { ISearchOptions, ISearchResultChangeEvent } from "@xterm/addon-search";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it } from "vitest";
import FindBar, { type TerminalSearch } from "./FindBar.svelte";

/** Records the searches; `results` tells the bar a new count, as the addon does after a search. */
class FakeSearch implements TerminalSearch {
  calls: [string, string, ISearchOptions | undefined][] = [];
  cleared = 0;
  private listener: ((event: ISearchResultChangeEvent) => void) | null = null;
  findNext(term: string, options?: ISearchOptions) {
    this.calls.push(["next", term, options]);
    return true;
  }
  findPrevious(term: string, options?: ISearchOptions) {
    this.calls.push(["previous", term, options]);
    return true;
  }
  clearDecorations() {
    this.cleared++;
  }
  onDidChangeResults(listener: (event: ISearchResultChangeEvent) => void) {
    this.listener = listener;
    return { dispose: () => (this.listener = null) };
  }
  results(resultIndex: number, resultCount: number) {
    this.listener?.({ resultIndex, resultCount });
    flushSync();
  }
}

const decorations = { matchOverviewRuler: "#000000", activeMatchColorOverviewRuler: "#ffffff" };
let app: Record<string, unknown> | null = null;
const target = document.body.appendChild(document.createElement("div"));
afterEach(() => {
  if (app) unmount(app);
  app = null;
});

function setup() {
  const search = new FakeSearch();
  let closed = 0;
  const bar = mount(FindBar, { target, props: { search, decorations, onclose: () => closed++ } });
  app = bar;
  flushSync();
  const input = target.querySelector<HTMLInputElement>("input");
  if (!input) throw new Error("no input");
  const type = (text: string) => {
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
  };
  const press = (key: string, shiftKey = false) =>
    input.dispatchEvent(new KeyboardEvent("keydown", { key, shiftKey, bubbles: true, cancelable: true }));
  const button = (label: string) => target.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
  const count = () => target.querySelector(".count")?.textContent;
  return { search, bar, input, type, press, button, count, closed: () => closed };
}

it("finds as the text is typed, steps with Enter and Shift+Enter, and shows the count", () => {
  const { search, input, type, press, button, count } = setup();
  expect(input.getAttribute("aria-label")).toBe("Find in terminal");
  type("err");
  search.results(0, 3);
  expect(count()).toBe("1 of 3");
  press("Enter");
  press("Enter", true);
  button("Next match")?.click();
  button("Previous match")?.click();
  expect(search.calls.map(([dir, term, options]) => [dir, term, options?.incremental])).toEqual([
    ["next", "err", true],
    ["next", "err", false],
    ["previous", "err", false],
    ["next", "err", false],
    ["previous", "err", false],
  ]);
  expect(search.calls[0]?.[2]?.decorations).toBe(decorations);
  search.results(-1, 1000);
  expect(count()).toBe("1000+ results");
  search.results(-1, 0);
  expect(count()).toBe("No results");
});

it("clears the highlights when the text is emptied and when the bar closes", () => {
  const { search, type, press, bar } = setup();
  type("x");
  type("");
  expect(search.cleared).toBe(1);
  press("Enter");
  expect(search.calls).toHaveLength(1);
  unmount(bar);
  app = null;
  expect(search.cleared).toBe(2);
});

it("closes on Escape and on the close button, and focuses its text on request", () => {
  const { input, press, button, closed, type, bar } = setup();
  type("abc");
  (bar as unknown as { focus: () => void }).focus();
  expect(document.activeElement).toBe(input);
  expect([input.selectionStart, input.selectionEnd]).toEqual([0, 3]);
  press("Escape");
  button("Close find")?.click();
  expect(closed()).toBe(2);
});
