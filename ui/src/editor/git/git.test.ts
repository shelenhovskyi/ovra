import { describe, expect, test } from "vitest";
import type { Blame, CoreEvent, Hunk, Request } from "../../lib/protocol";
import { CoreError, type Transport } from "../../lib/transport";
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
