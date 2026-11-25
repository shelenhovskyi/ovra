import { afterEach, describe, expect, it, vi } from "vitest";
import { CoreError } from "../lib/transport";
import { FakeSearchCore, file } from "./fake-search";
import { mergeFiles, SearchModel, searchError, splitGlobs, TYPING_DELAY_MS } from "./model.svelte";

const models: SearchModel[] = [];

function setup() {
  const core = new FakeSearchCore();
  const model = new SearchModel(core, "p1");
  models.push(model);
  return { core, model };
}

afterEach(() => {
  for (const model of models.splice(0)) model.dispose();
  vi.useRealTimers();
});

describe("SearchModel", () => {
  it("sends the query and keeps streamed files sorted by path", async () => {
    const { core, model } = setup();
    model.pattern = "needle";
    model.caseSensitive = true;
    model.include = "*.rs, src ,";
    await model.run();
    expect(core.calls[0]).toEqual({
      method: "search.start",
      params: {
        project: "p1",
        query: {
          pattern: "needle",
          regex: false,
          case_sensitive: true,
          whole_word: false,
          include: ["*.rs", "src"],
          exclude: [],
        },
      },
    });
    core.results(1, file("src/b.rs", [3, "needle", [[0, 6]]]));
    core.results(1, file("a.rs", [1, "a needle", [[2, 8]]], [9, "needle", [[0, 6]]]));
    core.results(99, file("other.rs", [1, "needle", [[0, 6]]]));
    expect(model.files.map((f) => f.path)).toEqual(["a.rs", "src/b.rs"]);
    expect(model.lineCount).toBe(3);
    expect(model.status).toEqual({ kind: "searching" });
    core.done(1, { truncated: true });
    expect(model.status).toEqual({ kind: "done", truncated: true, stopped: false });
  });

  it("keeps results that arrive before the core answers with the id", async () => {
    const { core, model } = setup();
    core.beforeReply = (id) => [
      { type: "search.results", data: { id, files: [file("a.txt", [1, "x", [[0, 1]]])] } },
      { type: "search.done", data: { id, files: 1, lines: 1, truncated: false, cancelled: false } },
    ];
    model.pattern = "x";
    await model.run();
    expect(model.files.map((f) => f.path)).toEqual(["a.txt"]);
    expect(model.status.kind).toBe("done");
  });

  it("waits for typing to pause, and a new search cancels the running one", async () => {
    vi.useFakeTimers();
    const { core, model } = setup();
    model.pattern = "a";
    model.schedule();
    model.pattern = "ab";
    model.schedule();
    await vi.advanceTimersByTimeAsync(TYPING_DELAY_MS - 1);
    expect(core.calls).toEqual([]);
    await vi.advanceTimersByTimeAsync(1);
    expect(core.methods()).toEqual(["search.start"]);
    core.results(1, file("a.txt", [1, "ab", [[0, 2]]]));
    model.pattern = "abc";
    await model.run();
    expect(core.calls[1]).toEqual({ method: "search.cancel", params: { id: 1 } });
    expect(model.files).toEqual([]);
    core.results(1, file("late.txt", [1, "ab", [[0, 2]]]));
    expect(model.files).toEqual([]);
  });

  it("an empty pattern clears the results without asking the core", async () => {
    const { core, model } = setup();
    await model.run();
    expect(core.calls).toEqual([]);
    expect(model.status).toEqual({ kind: "idle" });
  });

  it("says in plain words why the core rejects a pattern, and how to search as typed", async () => {
    const { core, model } = setup();
    core.failNext = new CoreError({
      code: "bad_request",
      message: "invalid search pattern: regex parse error:\n    (?:fn ()\n       ^\nerror: unclosed group",
    });
    model.pattern = "fn (";
    model.regex = true;
    await model.run();
    expect(model.status).toEqual({
      kind: "error",
      message: "Not a valid regular expression: a ( is not closed. Turn off .* to search for the text as typed.",
    });
    expect(searchError("invalid search pattern: something new")).toBe(
      "Not a valid regular expression. Turn off .* to search for the text as typed.",
    );
    expect(searchError("invalid glob: [")).toBe("invalid glob: [");
  });

  it("stop cancels the running search and keeps what it found", async () => {
    const { core, model } = setup();
    model.pattern = "x";
    await model.run();
    core.results(1, file("a.txt", [1, "x", [[0, 1]]]));
    model.stop();
    expect(core.methods()).toEqual(["search.start", "search.cancel"]);
    expect(model.status).toEqual({ kind: "done", truncated: false, stopped: true });
    expect(model.files).toHaveLength(1);
  });

  it("stopping before the core answers cancels the search once its id arrives", async () => {
    const { core, model } = setup();
    model.pattern = "x";
    const running = model.run();
    model.stop();
    await running;
    expect(core.methods()).toEqual(["search.start", "search.cancel"]);
    core.results(1, file("a.txt", [1, "x", [[0, 1]]]));
    expect(model.files).toEqual([]);
    expect(model.status).toEqual({ kind: "done", truncated: false, stopped: true });
  });

  it("searches again after missing events, which may include the end", async () => {
    const { core, model } = setup();
    model.pattern = "x";
    await model.run();
    core.emit({ type: "info.resync", data: { skipped: 3 } });
    for (let i = 0; i < 4; i++) await Promise.resolve();
    expect(core.methods()).toEqual(["search.start", "search.cancel", "search.start"]);
    core.done(2);
    core.emit({ type: "info.resync", data: { skipped: 3 } });
    expect(core.methods()).toHaveLength(3);
  });
});

describe("helpers", () => {
  it("splits globs on commas", () => {
    expect(splitGlobs(" *.ts,, docs/ ")).toEqual(["*.ts", "docs/"]);
  });

  it("merges files in byte order of their paths", () => {
    const merged = mergeFiles([file("b", [1, "", []])], [file("B", [1, "", []]), file("a/z", [1, "", []])]);
    expect(merged.map((f) => f.path)).toEqual(["B", "a/z", "b"]);
  });
});
