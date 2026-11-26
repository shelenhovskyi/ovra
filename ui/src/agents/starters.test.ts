import { describe, expect, it } from "vitest";
import { STARTERS, starterMode } from "./starters";

describe("starters", () => {
  it("explains through research while research has a place, else as one agent here", () => {
    const explain = STARTERS.find((s) => s.id === "explain");
    const tests = STARTERS.find((s) => s.id === "tests");
    if (!explain || !tests) throw new Error("missing starter");
    expect(starterMode(explain, true)).toBe("research");
    expect(starterMode(explain, false)).toBe("here");
    expect(starterMode(tests, false)).toBe("here");
    expect(STARTERS.map((s) => s.label)).toEqual(["Explain this project", "Fix failing tests", "Write a README"]);
  });
});
