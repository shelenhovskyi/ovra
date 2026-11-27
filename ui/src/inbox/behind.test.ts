import { describe, expect, it } from "vitest";
import { behindText, named, updatedText } from "./behind";

describe("behind", () => {
  it("names a few files by name and counts the rest", () => {
    expect(named(["src/a.ts"])).toBe("a.ts");
    expect(named(["a", "b"])).toBe("a and b");
    expect(named(["a", "b", "c", "d", "e"])).toBe("a, b, c and 2 more");
  });

  it("warns only when Keep would leave files out", () => {
    expect(behindText({ files: 1, clashes: [] })).toMatch(/^Your project changed in 1 file since .* Keep copies all/);
    expect(behindText({ files: 4, clashes: ["a", "b"] })).toContain("Keep would leave them out");
  });

  it("says what an update did", () => {
    const id = "r1";
    expect(updatedText({ id, files: 0, conflicts: [], written: false })).toMatch(/^Nothing to bring in/);
    expect(updatedText({ id, files: 2, conflicts: [], written: true })).toBe(
      "Brought in 2 files from your project. The diff now shows only the agent's changes.",
    );
    expect(updatedText({ id, files: 1, conflicts: ["a", "b"], written: true })).toContain(
      "a and b now have conflict markers",
    );
  });
});
