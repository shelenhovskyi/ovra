import { expect, it } from "vitest";
import { change, review } from "./fake-review";
import { ProtectedLooks } from "./protected.svelte";

const names = (files: { path: string }[]) => files.map((f) => f.path);

it("keeps a direct item's protected files only once each one's diff was seen", () => {
  const looks = new ProtectedLooks();
  const settings = change(".claude/settings.json", { protected: true });
  const envrc = change(".envrc", { protected: true, by: "outside" });
  const item = review("r1", { files: [change("README.md"), settings, envrc] });
  expect(looks.directWaits(item)).toBe(true);
  expect(looks.included(item)).toBe(false);
  // Any file's diff but a protected one's counts for nothing.
  looks.saw(item, item.files[0] ?? settings, null);
  looks.saw(item, settings, null);
  expect(names(looks.unseen(item))).toEqual([".envrc"]);
  expect(looks.directWaits(item)).toBe(true);
  looks.saw(item, envrc, null);
  expect(looks.directWaits(item)).toBe(false);
  expect(looks.included(item)).toBe(true);
  // A direct item without protected files applies none.
  expect(looks.included(review("r2"))).toBe(false);
});

it("counts a diff as seen only in the version of the files the list shows, and asks again after a change", () => {
  const looks = new ProtectedLooks();
  const claude = change("CLAUDE.md", { protected: true });
  const item = review("r1", { fingerprint: "d2", files: [claude] });
  // The diff on screen was read before the file changed again.
  looks.saw(item, claude, "d1");
  expect(looks.directWaits(item)).toBe(true);
  looks.saw(item, claude, "d2");
  expect(looks.directWaits(item)).toBe(false);
  // The file changed once more: a new look.
  const changed = { ...claude, additions: claude.additions + 1 };
  expect(looks.directWaits(review("r1", { fingerprint: "d2", files: [changed] }))).toBe(true);
  expect(looks.directWaits(review("r1", { fingerprint: "d3", files: [claude] }))).toBe(true);
  // Seen means seen in this sitting.
  looks.forget();
  expect(looks.directWaits(item)).toBe(true);
});

it("ticks a worker's protected files in only after each one's diff was seen, and copies them only when ticked", () => {
  const looks = new ProtectedLooks();
  const envrc = change(".envrc", { protected: true });
  const mcp = change(".mcp.json", { protected: true });
  const waits = change("CLAUDE.md", { protected: true, blocked: true });
  const item = review("r1", {
    session: "s2",
    mode: "isolated",
    fingerprint: "f1",
    files: [change("NOTES.md"), envrc, mcp, waits],
  });
  // One that waits for the direct agent is not the worker's to copy.
  expect(names(looks.includable(item))).toEqual([".envrc", ".mcp.json"]);
  expect(looks.directWaits(item)).toBe(false);
  looks.saw(item, envrc, "f1");
  expect(names(looks.includableUnseen(item))).toEqual([".mcp.json"]);
  expect(looks.tick(item, true)).toBe(false);
  expect(looks.ticked(item)).toBe(false);
  expect(looks.included(item)).toBe(false);
  looks.saw(item, mcp, "f1");
  expect(looks.included(item)).toBe(false);
  expect(looks.tick(item, true)).toBe(true);
  expect(looks.included(item)).toBe(true);
  looks.untick();
  expect(looks.included(item)).toBe(false);
});

it("drops the tick once the worker's files change, for good", () => {
  const looks = new ProtectedLooks();
  const files = [change(".envrc", { protected: true }), change(".mcp.json", { protected: true })];
  const before = review("r2", { session: "s3", mode: "isolated", fingerprint: "f2", files });
  for (const f of files) looks.saw(before, f, "f2");
  looks.tick(before, true);
  expect(looks.ticked(before)).toBe(true);
  const after = { ...before, fingerprint: "f3" };
  expect(looks.ticked(after)).toBe(false);
  looks.dropStaleTick(after);
  // Looking at each again does not bring it back, nor does the old version.
  for (const f of files) looks.saw(after, f, "f3");
  expect(looks.includableUnseen(after)).toEqual([]);
  expect(looks.ticked(after)).toBe(false);
  expect(looks.ticked(before)).toBe(false);
  expect(looks.included(after)).toBe(false);
  // A new sitting drops it too.
  looks.tick(after, true);
  looks.forget();
  expect(looks.ticked(after)).toBe(false);
});

it("replaces a protected file left over, including one that waited at Keep or was renamed, only once seen", () => {
  const looks = new ProtectedLooks();
  const mcp = change(".mcp.json", { protected: true });
  const envrc = change(".envrc", { protected: true, blocked: true });
  const renamed = change("notes.md", { kind: "renamed", from: "CLAUDE.md", protected: true });
  const item = review("r3", {
    session: "s3",
    mode: "isolated",
    state: "partly_approved",
    files: [mcp, envrc, renamed, change("other.md", { protected: true })],
    not_copied: [".envrc", ".mcp.json", "CLAUDE.md"],
  });
  expect(names(looks.takeGuarded(item))).toEqual([".mcp.json", ".envrc", "notes.md"]);
  looks.saw(item, mcp, null);
  looks.saw(item, renamed, null);
  expect(names(looks.takeUnseen(item))).toEqual([".envrc"]);
  looks.saw(item, envrc, null);
  expect(looks.takeUnseen(item)).toEqual([]);
  // Nothing is left over before a partial Keep.
  expect(looks.takeGuarded({ ...item, state: "open" })).toEqual([]);
});
