import { describe, expect, it } from "vitest";
import { byDay, dayLabel, timeLabel } from "./days";
import { entry } from "./fake-history";

const at = (y: number, m: number, d: number, h = 12, min = 0) => new Date(y, m - 1, d, h, min).getTime();
const now = at(2026, 11, 26, 9);

describe("history days", () => {
  it("names days as people say them", () => {
    expect(dayLabel(at(2026, 11, 26, 0, 1), now)).toBe("Today");
    expect(dayLabel(at(2026, 11, 25, 23, 59), now)).toBe("Yesterday");
    expect(dayLabel(at(2026, 11, 22), now)).toBe("Sunday");
    expect(dayLabel(at(2026, 11, 2), now)).toBe("November 2");
    expect(dayLabel(at(2025, 12, 31), now)).toBe("December 31, 2025");
    expect(dayLabel(null, now)).toBe("Earlier");
    expect(timeLabel(at(2026, 11, 26, 7, 5))).toBe("07:05");
    expect(timeLabel(null)).toBe("");
  });

  it("groups runs of the same day in the order given", () => {
    const groups = byDay(
      [
        entry("s3", at(2026, 11, 26, 8)),
        entry("s2", at(2026, 11, 26, 1)),
        entry("s1", at(2026, 11, 25)),
        entry("s0", null),
      ],
      now,
    );
    expect(groups.map((g) => [g.label, g.entries.map((e) => e.session.id)])).toEqual([
      ["Today", ["s3", "s2"]],
      ["Yesterday", ["s1"]],
      ["Earlier", ["s0"]],
    ]);
  });
});
