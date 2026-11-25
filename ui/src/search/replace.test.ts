import { expect, describe as group, it } from "vitest";
import type { FileContent, FileMatches, Request, ResponseFor } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { file } from "./fake-search";
import {
  applyEdits,
  describe,
  expand,
  matcher,
  type OpenBuffer,
  planFile,
  type ReplaceQuery,
  replaceInFiles,
} from "./replace";

const query = (pattern: string, replacement: string, extra: Partial<ReplaceQuery> = {}): ReplaceQuery => ({
  pattern,
  replacement,
  regex: false,
  caseSensitive: false,
  wholeWord: false,
  ...extra,
});

const replaced = (text: string, found: FileMatches, q: ReplaceQuery, only?: number) =>
  applyEdits(text, planFile(text, found, q, only).edits);

function must<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) throw new Error("missing");
  return value;
}

group("matcher", () => {
  it("escapes literal text and follows the case and word options", () => {
    expect("a.b axb A.B".match(must(matcher(query("a.b", ""))))).toEqual(["a.b", "A.B"]);
    expect("a.b A.B".match(must(matcher(query("a.b", "", { caseSensitive: true }))))).toEqual(["a.b"]);
    expect("cat cats écat cat_".match(must(matcher(query("cat", "", { wholeWord: true }))))).toEqual(["cat"]);
    expect(matcher(query("(?i)x", "", { regex: true }))).toBeNull();
  });
});

group("expand", () => {
  it("works like String.replace", () => {
    const re = /(?<k>\w+)=(\d)/u;
    const line = "a k=1 b";
    const match = must(re.exec(line));
    const template = "[$2|$1|$<k>|$&|$$|$`|$'|$3|$0|$10]";
    expect(expand(template, match, line)).toBe(line.replace(re, template).slice(2, -2));
  });
});

group("planFile", () => {
  it("replaces the reported matches at their UTF-16 ranges, keeping line breaks", () => {
    const text = "x\r\nlet 🦀 = needle + Needle;\r\nneedle\n";
    const found = file(
      "a.rs",
      [
        2,
        "let 🦀 = needle + Needle;",
        [
          [9, 15],
          [18, 24],
        ],
      ],
      [3, "needle", [[0, 6]]],
    );
    expect(replaced(text, found, query("needle", "pin"))).toBe("x\r\nlet 🦀 = pin + pin;\r\npin\n");
    expect(replaced(text, found, query("needle", "pin"), 3)).toBe("x\r\nlet 🦀 = needle + Needle;\r\npin\n");
  });

  it("expands capture groups in regex mode only", () => {
    const text = "f(a, b)\n";
    const found = file("a", [1, "f(a, b)", [[2, 6]]]);
    expect(replaced(text, found, query("(\\w), (\\w)", "$2, $1", { regex: true }))).toBe("f(b, a)\n");
    expect(replaced("$1 x\n", file("a", [1, "$1 x", [[3, 4]]]), query("x", "$1"))).toBe("$1 $1\n");
  });

  it("reports a file whose lines are not what the search found", () => {
    const found = file("a", [2, "needle", [[0, 6]]]);
    expect(planFile("x\nneedles\n", found, query("needle", "")).changed).toBe(true);
    expect(planFile("x\n", found, query("needle", "")).changed).toBe(true);
    expect(planFile("x\nneedle", found, query("needle", "")).changed).toBe(false);
  });

  it("skips a line where JavaScript finds other matches than the core", () => {
    const plan = planFile("aaa\n", file("a", [1, "aaa", [[0, 1]]]), query("a", "b"));
    expect(plan).toMatchObject({ edits: [], skipped: 1, changed: false });
  });

  it("replaces every match of a long line the core cut to a window", () => {
    const line = `${"x".repeat(500)} needle ${"y".repeat(500)} needle`;
    const found = {
      path: "a",
      lines: [{ line: 1, text: line.slice(421, 821), column: 421, ranges: [[80, 86]] as [number, number][] }],
    };
    expect(replaced(line, found, query("needle", "n"))).toBe(`${"x".repeat(500)} n ${"y".repeat(500)} n`);
  });
});

class FakeFs implements Transport {
  files = new Map<string, { text: string; mtime: number }>();
  /** Bumps a file's mtime between its read and its write. */
  raceOn: string | null = null;

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    if (request.method === "fs.read_file") {
      const f = must(this.files.get(request.params.path));
      const data: FileContent = {
        path: request.params.path,
        size: 0,
        mtime_ms: f.mtime,
        body: { kind: "text", text: f.text },
      };
      if (this.raceOn === request.params.path) f.mtime++;
      return { type: "fs.file", data } as ResponseFor<R>;
    }
    if (request.method === "fs.write_file") {
      const f = must(this.files.get(request.params.path));
      if (request.params.expected_mtime_ms !== f.mtime) throw new CoreError({ code: "conflict", message: "changed" });
      this.files.set(request.params.path, { text: request.params.text, mtime: f.mtime + 1 });
      return { type: "fs.written", data: { mtime_ms: f.mtime + 1 } } as ResponseFor<R>;
    }
    throw new Error(`unexpected ${request.method}`);
  }

  onEvent(): () => void {
    return () => {};
  }
}

it("replaceInFiles edits open buffers, writes other files and never overwrites a change", async () => {
  const fs = new FakeFs();
  fs.files.set("disk", { text: "needle\n", mtime: 5 });
  fs.files.set("raced", { text: "needle\n", mtime: 5 });
  fs.files.set("stale", { text: "moved\n", mtime: 5 });
  const edits: unknown[] = [];
  const buffer: OpenBuffer = {
    loaded: Promise.resolve(),
    status: "ready",
    text: "a needle\n",
    edit: (spec) => edits.push(spec) > 0,
  };
  fs.raceOn = "raced";
  const hit = (path: string, text = "needle", at = 0) => file(path, [1, text, [[at, at + 6]]]);
  const out = await replaceInFiles(
    { transport: fs, project: "p", buffer: (path) => (path === "open" ? buffer : undefined) },
    [hit("disk"), hit("open", "a needle", 2), hit("raced"), hit("stale")],
    query("needle", "pin"),
  );
  expect(fs.files.get("disk")?.text).toBe("pin\n");
  expect(fs.files.get("raced")?.text).toBe("needle\n");
  expect(edits).toEqual([{ changes: [{ from: 2, to: 8, insert: "pin" }] }]);
  expect(out).toMatchObject({ replaced: 2, files: ["disk", "open"], changed: ["stale"], conflicts: ["raced"] });
  expect(describe(out)).toBe(
    "Replaced 2 matches in 2 files. 1 file changed since the search. 1 file changed while replacing.",
  );
});
