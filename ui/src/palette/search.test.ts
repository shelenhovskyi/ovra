import { expect, it } from "vitest";
import { idleRows, keywordMatches, noteRecent, recentIds } from "./search";
import type { Entry } from "./sources";

const entry = (id: string, group: string, keywords?: string[]): Entry => ({
  id,
  label: id,
  group,
  keywords,
  run: () => {},
});
const ids = (entries: Entry[]) => entries.map((e) => `${e.group}:${e.id}`);

it("an empty query shows recent entries that still exist, then suggestions or projects", () => {
  const all = [entry("file:a", "Files"), entry("command:x", "Commands"), entry("command:y", "Commands")];
  expect(ids(idleRows(all, ["file:gone", "command:x", "file:a"], ["command:x", "command:y", "command:z"]))).toEqual([
    "Recent:command:x",
    "Recent:file:a",
    "Suggested:command:y",
  ]);
  const start = [entry("project:p", "Projects"), entry("command:x", "Commands")];
  expect(ids(idleRows(start, [], null))).toEqual(["Projects:project:p"]);
});

it("keywords match at word starts, leaving out rows already ranked", () => {
  const all = [
    entry("net", "Commands", ["proxy", "Settings"]),
    entry("rules", "Commands", ["permissions", "Settings"]),
  ];
  expect(keywordMatches(all, "prox", new Set()).map((e) => e.id)).toEqual(["net"]);
  expect(keywordMatches(all, "settings", new Set(["rules"])).map((e) => e.id)).toEqual(["net"]);
  expect(keywordMatches(all, "ettings", new Set())).toEqual([]);
  expect(keywordMatches(all, " ", new Set())).toEqual([]);
});

it("remembers what was chosen, newest first, without repeats", () => {
  const data = new Map<string, string>();
  const store = { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => data.set(k, v) };
  noteRecent("a", store as unknown as Storage);
  noteRecent("b", store as unknown as Storage);
  noteRecent("a", store as unknown as Storage);
  expect(recentIds(store as unknown as Storage)).toEqual(["a", "b"]);
});
