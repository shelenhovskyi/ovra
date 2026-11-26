import { describe, expect, it } from "vitest";
import { Away, awayLine, BREAK_MS, type Moment } from "./away.svelte";

const done = (...rows: [string, string, string][]): Moment["done"] =>
  rows.map(([id, name, status]) => ({ id, name, status }));

describe("awayLine", () => {
  it("names what ended meanwhile and counts what waits now", () => {
    const before = { done: done(["s1", "Old try", "Done"]), waiting: 0 };
    const now = { done: done(["s1", "Old try", "Done"], ["s2", "Fix sync", "Done"]), waiting: 2 };
    expect(awayLine(before, now)).toBe("While you were away, Fix sync finished and 2 things wait for you.");
  });

  it("says failed and stopped, and counts what it does not name", () => {
    const now = {
      done: done(
        ["s1", "Fix sync", "Failed: usage limit reached"],
        ["s2", "Add a test", "Stopped"],
        ["s3", "Docs", "Done"],
        ["s4", "Lint", "Done"],
      ),
      waiting: 1,
    };
    expect(awayLine({ done: [], waiting: 0 }, now)).toBe(
      "While you were away, Fix sync failed, Add a test stopped, 2 more ended and 1 thing waits for you.",
    );
  });

  it("speaks of a session that ended again with other words", () => {
    const before = { done: done(["s1", "Fix sync", "Done"]), waiting: 0 };
    const now = { done: done(["s1", "Fix sync", "Failed: crashed"]), waiting: 0 };
    expect(awayLine(before, now)).toBe("While you were away, Fix sync failed.");
  });

  it("says nothing when nothing ended and nothing new waits", () => {
    const moment = { done: done(["s1", "Fix sync", "Done"]), waiting: 1 };
    expect(awayLine(moment, moment)).toBeNull();
    expect(awayLine({ done: [], waiting: 2 }, { done: [], waiting: 1 })).toBeNull();
  });

  it("speaks of new things that wait when nothing ended", () => {
    expect(awayLine({ done: [], waiting: 1 }, { done: [], waiting: 3 })).toBe(
      "While you were away, 3 things wait for you.",
    );
  });

  it("cuts a long name", () => {
    const now = { done: done(["s1", "x".repeat(80), "Done"]), waiting: 0 };
    expect(awayLine({ done: [], waiting: 0 }, now)).toBe(`While you were away, ${"x".repeat(59)}… finished.`);
  });
});

describe("Away", () => {
  const before = { done: [], waiting: 0 };
  const after = { done: done(["s1", "Fix sync", "Done"]), waiting: 0 };

  it("shows the line when the person comes back after a break, until dismissed", () => {
    const away = new Away();
    away.left(0, before);
    away.here(BREAK_MS, after);
    expect(away.line).toBe("While you were away, Fix sync finished.");
    away.dismiss();
    expect(away.line).toBeNull();
  });

  it("stays quiet after a short absence, and counts a break from what the person last saw", () => {
    const away = new Away();
    away.here(0, before);
    away.here(BREAK_MS - 1, before);
    away.here(BREAK_MS + 1, after);
    expect(away.line).toBeNull();
  });

  it("needs a first moment before it can speak", () => {
    const away = new Away();
    away.here(BREAK_MS * 3, after);
    expect(away.line).toBeNull();
  });
});
