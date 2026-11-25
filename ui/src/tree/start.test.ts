import { expect, it } from "vitest";
import { openedAgo, shortPath } from "./start";

it("shortens the home folder to ~ and leaves other paths alone", () => {
  expect(
    ["/home/ann/code/app", "/Users/ann", "C:\\Users\\ann\\app", "/root/x", "/homework/a", "/srv/app"].map(shortPath),
  ).toEqual(["~/code/app", "~", "~\\app", "~/x", "/homework/a", "/srv/app"]);
});

it("says when a project was opened in a few words", () => {
  const now = Date.UTC(2026, 10, 25, 12);
  expect(
    [null, now - 10_000, now - 5 * 60_000, now - 2 * 3_600_000, now - 30 * 3_600_000, now - 3 * 86_400_000].map((ms) =>
      openedAgo(ms, now),
    ),
  ).toEqual(["", "Just now", "5 min ago", "2h ago", "Yesterday", "3 days ago"]);
  expect(openedAgo(Date.UTC(2026, 10, 1, 12), now)).toBe("Nov 1");
});
