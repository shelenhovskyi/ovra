import { describe, expect, it } from "vitest";
import { commentLine, hunkPlace, parseHunks } from "./hunks";

const PATCH = [
  "diff --git a/src/a.ts b/src/a.ts",
  "index 1..2 100644",
  "--- a/src/a.ts",
  "+++ b/src/a.ts",
  "@@ -1,3 +1,3 @@ function top()",
  "-one\r",
  "+uno\r",
  " two\r",
  " three\r",
  "@@ -10,2 +10,0 @@",
  "-gone",
  "-also gone",
  "\\ No newline at end of file",
  "",
].join("\n");

describe("parseHunks", () => {
  it("gives each hunk back as the core needs it, with numbered lines", () => {
    const [first, second] = parseHunks(PATCH);
    expect(first?.hunk).toEqual({
      old_start: 1,
      old_lines: 3,
      new_start: 1,
      new_lines: 3,
      header: "function top()",
      // A CRLF file's lines keep their `\r`, or the hunk would not match it.
      lines: ["-one\r", "+uno\r", " two\r", " three\r"],
    });
    expect(first?.lines.slice(0, 3)).toEqual([
      { kind: "removed", text: "one", old: 1 },
      { kind: "added", text: "uno", new: 1 },
      { kind: "context", text: "two", old: 2, new: 2 },
    ]);
    expect(second?.hunk.lines).toEqual(["-gone", "-also gone", "\\ No newline at end of file"]);
    expect(first && hunkPlace(first)).toBe("lines 1–3");
    expect(second && hunkPlace(second)).toBe("lines 10–11 of the old version");
    const [removed, added] = first?.lines ?? [];
    expect(removed && commentLine(removed)).toEqual({ line: 1, old: true });
    expect(added && commentLine(added)).toEqual({ line: 1, old: false });
    expect(parseHunks("Binary files differ\n")).toEqual([]);
  });
});
