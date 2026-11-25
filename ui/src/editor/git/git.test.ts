import { history, undo } from "@codemirror/commands";
import { EditorState, Text } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, beforeAll, beforeEach, describe, expect, test, vi } from "vitest";
import type { Blame, CoreEvent, Hunk, Request } from "../../lib/protocol";
import { CoreError, type Transport } from "../../lib/transport";
import { describeChange, gitExtension, goToChange, readBlame, showChangeAtCursor, toggleBlame } from "./extension";
import { GitFile, pickHunk } from "./file";

class FakeGit implements Transport {
  index: { text: string | null; exists: boolean } = { text: "a\nb\nc\n", exists: true };
  hunks: Hunk[] = [];
  blame: Blame = { ranges: [], commits: {} };
  failures = new Map<string, CoreError>();
  calls: Request[] = [];

  // biome-ignore lint/suspicious/noExplicitAny: test double for a generic method
  async call(request: Request): Promise<any> {
    this.calls.push(request);
    const failure = this.failures.get(request.method);
    if (failure) throw failure;
    switch (request.method) {
      case "git.show":
        return { type: "git.file", data: this.index };
      case "git.diff":
        return {
          type: "git.diff",
          data: { path: "f", from: null, binary: false, hunks: this.hunks, truncated: false },
        };
      case "git.blame":
        return { type: "git.blame", data: this.blame };
      default:
        return { type: "ok" };
    }
  }

  onEvent(_listener: (event: CoreEvent) => void): () => void {
    return () => {};
  }

  count(method: string): number {
    return this.calls.filter((c) => c.method === method).length;
  }
}

const hunk = (new_start: number, new_lines: number): Hunk => ({
  old_start: new_start,
  old_lines: new_lines,
  new_start,
  new_lines,
  header: "",
  lines: [],
});

describe("GitFile", () => {
  test("caches index text and blame until changed; a core without git is not asked again", async () => {
    const core = new FakeGit();
    const file = new GitFile({ transport: core, project: "p1", path: "f" });
    expect(await file.indexLines()).toEqual(["a", "b", "c", ""]);
    await file.indexLines();
    await file.blame();
    await file.blame();
    expect([core.count("git.show"), core.count("git.blame")]).toEqual([1, 1]);
    core.index = { text: null, exists: false };
    file.changed();
    expect(await file.indexLines()).toBeNull();
    core.failures.set("git.show", new CoreError({ code: "unavailable", message: "no git" }));
    file.changed();
    expect(await file.indexLines()).toBeNull();
    file.changed();
    expect(await file.indexLines()).toBeNull();
    expect(await file.blame()).toBeNull();
    expect([core.count("git.show"), core.count("git.blame")]).toEqual([3, 1]);
  });

  test("stages the unstaged-diff hunk that overlaps the change, and explains conflicts", async () => {
    const core = new FakeGit();
    const file = new GitFile({ transport: core, project: "p1", path: "f" });
    core.hunks = [hunk(1, 5), hunk(20, 7)];
    // Buffer lines 23-24 (0-based 22-23).
    expect(await file.stage({ oldStart: 22, oldCount: 1, newStart: 22, newCount: 2 })).toBeNull();
    expect(core.calls.at(-1)).toEqual({
      method: "git.stage_hunk",
      params: { project: "p1", path: "f", hunk: hunk(20, 7) },
    });
    // A deletion at the end of a hunk's context still overlaps it.
    expect(pickHunk(core.hunks, { oldStart: 5, oldCount: 2, newStart: 5, newCount: 0 })).toEqual(hunk(1, 5));
    expect(pickHunk(core.hunks, { oldStart: 10, oldCount: 1, newStart: 10, newCount: 1 })).toBeUndefined();
    core.failures.set("git.stage_hunk", new CoreError({ code: "conflict", message: "patch does not apply" }));
    expect(await file.stage({ oldStart: 2, oldCount: 1, newStart: 2, newCount: 1 })).toBe(
      "The file changed since its diff was read. Try again.",
    );
  });
});

describe("git extension", () => {
  beforeAll(() => {
    Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
    Range.prototype.getBoundingClientRect = () => new DOMRect();
  });
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(Date.UTC(2026, 10, 25));
  });
  let view: EditorView | null = null;
  afterEach(() => {
    view?.destroy();
    vi.useRealTimers();
  });

  /** The text of the editor's live region. */
  const announced = (target: EditorView) => target.dom.querySelector(".cm-announced")?.textContent;

  function setup(doc: string, dirty = false) {
    const core = new FakeGit();
    const file = new GitFile({ transport: core, project: "p1", path: "f" });
    const saved = Text.of(core.index.text?.split("\n") ?? [""]);
    const extensions = [history(), gitExtension({ file, dirty: () => dirty, saved: () => saved })];
    view = new EditorView({ state: EditorState.create({ doc, extensions }), parent: document.body });
    const created = view;
    const markers = () =>
      [...created.dom.querySelectorAll<HTMLElement>("[data-testid=git-marker]")].map((m) => m.dataset.change);
    return { core, file, view: created, markers };
  }

  test("marks changes against the index after a pause; untracked files get none", async () => {
    const { file, core, view, markers } = setup("a\nB\nc\nd\n");
    await vi.advanceTimersByTimeAsync(0);
    expect(markers()).toEqual(["modified", "added"]);
    expect(view.dom.querySelector<HTMLElement>("[data-change=added]")?.title).toBe("Added lines, click for actions");
    view.dispatch({ changes: { from: 2, to: 3, insert: "b" } });
    await vi.advanceTimersByTimeAsync(150);
    expect(markers()).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(50);
    expect(markers()).toEqual(["added"]);
    core.index = { text: null, exists: false };
    file.changed();
    await vi.advanceTimersByTimeAsync(0);
    expect(markers()).toEqual([]);
  });

  test("Revert hunk puts the index lines back as an undoable edit; Escape closes the popup", async () => {
    const { view, markers } = setup("a\nB\nX\nc\n");
    await vi.advanceTimersByTimeAsync(0);
    expect(markers()).toEqual(["modified", "modified"]);
    view.dispatch({ selection: { anchor: 3 } });
    showChangeAtCursor();
    const popup = view.dom.querySelector<HTMLElement>("[data-testid=git-change-popup]");
    expect(popup?.querySelector("pre")?.textContent).toBe("b");
    expect(document.activeElement?.textContent).toBe("Stage hunk");
    [...(popup?.querySelectorAll("button") ?? [])].find((b) => b.textContent === "Revert hunk")?.click();
    expect(view.state.doc.toString()).toBe("a\nb\nc\n");
    expect(view.dom.querySelector("[data-testid=git-change-popup]")).toBeNull();
    undo(view);
    expect(view.state.doc.toString()).toBe("a\nB\nX\nc\n");
    await vi.advanceTimersByTimeAsync(200);
    showChangeAtCursor();
    const again = view.dom.querySelector<HTMLElement>("[data-testid=git-change-popup]");
    expect(again?.getAttribute("aria-label")).toBe("Changed lines 2 to 3");
    // Tab cycles inside the popup; Escape from any of its buttons closes it.
    const tab = (shiftKey = false) =>
      document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { code: "Tab", shiftKey, bubbles: true }));
    tab();
    tab();
    expect(document.activeElement?.textContent).toBe("Close");
    tab();
    expect(document.activeElement?.textContent).toBe("Stage hunk");
    tab(true);
    expect(document.activeElement?.textContent).toBe("Close");
    again?.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true }));
    expect(view.dom.querySelector("[data-testid=git-change-popup]")).toBeNull();
    expect(view.hasFocus).toBe(true);
    // On a line without a change the command closes an open popup and says why.
    showChangeAtCursor();
    view.dispatch({ selection: { anchor: 0 } });
    showChangeAtCursor();
    expect(view.dom.querySelector("[data-testid=git-change-popup]")).toBeNull();
    expect(announced(view)).toBe("No git change on this line.");
  });

  test("an unsaved file's popup focuses Close, never Revert", async () => {
    const { view } = setup("a\nB\nc\n", true);
    await vi.advanceTimersByTimeAsync(0);
    view.dispatch({ selection: { anchor: 2 } });
    showChangeAtCursor();
    expect(document.activeElement?.textContent).toBe("Close");
  });

  test("next and previous change move the cursor, wrap around and announce the change", async () => {
    // Index a b c: line 1 added, line 3 changed, line 5 added at the end.
    const { view } = setup("new\na\nB\nc\nend\n");
    await vi.advanceTimersByTimeAsync(0);
    const line = () => view.state.doc.lineAt(view.state.selection.main.head).number;
    goToChange(1);
    expect([line(), announced(view)]).toEqual([3, "Changed line 3, 2 of 3"]);
    goToChange(1);
    expect([line(), announced(view)]).toEqual([5, "Added line 5, 3 of 3"]);
    goToChange(1);
    expect([line(), announced(view)]).toEqual([1, "Added line 1, 1 of 3"]);
    goToChange(-1);
    expect(line()).toBe(5);
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: "a\nb\nc\n" } });
    goToChange(-1);
    expect(announced(view)).toBe("No git changes in this file.");
    expect(
      [
        { oldStart: 2, oldCount: 3, newStart: 2, newCount: 0 },
        { oldStart: 0, oldCount: 1, newStart: 0, newCount: 0 },
        { oldStart: 1, oldCount: 0, newStart: 1, newCount: 2 },
      ].map(describeChange),
    ).toEqual(["Deleted 3 lines after line 2", "Deleted line at the start", "Added lines 2 to 3"]);
  });

  test("blames the cursor line after a pause and shows the blame gutter on toggle", async () => {
    const { core, view } = setup("a\nb\nc\n");
    const time = Date.UTC(2026, 10, 23) / 1000;
    core.blame = {
      ranges: [
        { start: 1, lines: 2, commit: "c1" },
        { start: 3, lines: 1, commit: null },
      ],
      commits: { c1: { id: "c1", author: "Ann", time, summary: "Start" } },
    };
    await vi.advanceTimersByTimeAsync(500);
    const inline = () => view.dom.querySelector("[data-testid=git-inline-blame]")?.textContent;
    expect(inline()).toBe("Ann, 2 days ago · Start");
    view.dispatch({ selection: { anchor: 4 } });
    expect(inline()).toBeUndefined();
    await vi.advanceTimersByTimeAsync(500);
    expect(inline()).toBe("Not committed yet");
    // Not read as part of the line; said on demand instead.
    expect(view.dom.querySelector("[data-testid=git-inline-blame]")?.getAttribute("aria-hidden")).toBe("true");
    view.dispatch({ selection: { anchor: 0 } });
    await readBlame();
    expect(announced(view)).toBe("Ann, 2 days ago · Start");
    toggleBlame();
    expect(announced(view)).toBe("Blame shown.");
    await vi.advanceTimersByTimeAsync(0);
    const labels = [...view.dom.querySelectorAll("[data-testid=git-blame]")].map((m) => m.textContent);
    expect(labels).toEqual(["Ann, 2 days ago", "Not committed yet"]);
    toggleBlame();
    expect(announced(view)).toBe("Blame hidden.");
    await vi.advanceTimersByTimeAsync(0);
    expect(view.dom.querySelectorAll("[data-testid=git-blame]")).toHaveLength(0);
    expect(core.count("git.blame")).toBe(1);
  });
});
