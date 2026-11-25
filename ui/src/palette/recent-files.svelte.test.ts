import { flushSync } from "svelte";
import { expect, it } from "vitest";
import type { Project } from "../lib/protocol";
import { noteRecentFile, recentFileEntries, recentFiles, recentFirst, trackRecentFiles } from "./recent-files.svelte";
import type { Entry } from "./sources";

/** A Storage over a Map, so tests never share the page's localStorage. */
function memory(): Storage {
  const items = new Map<string, string>();
  return {
    get length() {
      return items.size;
    },
    clear: () => items.clear(),
    getItem: (key) => items.get(key) ?? null,
    key: (index) => [...items.keys()][index] ?? null,
    removeItem: (key) => void items.delete(key),
    setItem: (key, value) => void items.set(key, value),
  };
}

const file = (path: string): Entry => ({ id: `file:${path}`, label: path, run: () => {} });
const rows = (entries: Entry[] | null) => entries?.map((e) => `${e.group}:${e.label}`) ?? null;

it("keeps the files opened per project, newest first, without repeats, at most 20", () => {
  const store = memory();
  for (const path of ["a.md", "b.md", "a.md"]) noteRecentFile("p1", path, store);
  noteRecentFile("p2", "c.md", store);
  expect(recentFiles("p1", store)).toEqual(["a.md", "b.md"]);
  expect(recentFiles("p2", store)).toEqual(["c.md"]);
  for (let i = 0; i < 25; i++) noteRecentFile("p1", `f${i}`, store);
  expect(recentFiles("p1", store)).toHaveLength(20);
  expect(recentFiles("p1", store)[0]).toBe("f24");
  // A broken or missing storage only costs the list.
  store.setItem("ovra.palette.recent_files:p3", "{not json");
  expect(recentFiles("p3", store)).toEqual([]);
  expect(recentFiles("p1", null)).toEqual([]);
});

it("drops files that no longer exist and leaves out the file shown now", () => {
  const store = memory();
  for (const path of ["gone.md", "b.md", "a.md"]) noteRecentFile("p1", path, store);
  const entries = [file("a.md"), file("b.md"), file("c.md")];
  expect(recentFileEntries("p1", entries, "a.md", store).map((e) => e.label)).toEqual(["b.md"]);
  expect(recentFiles("p1", store)).toEqual(["a.md", "b.md"]);
});

it("an empty Go to file lists the recent files first, else ranks as usual", () => {
  const entries = [file("a.md"), file("b.md"), file("c.md")];
  expect(rows(recentFirst([file("c.md")], entries, 50))).toEqual(["Recent:c.md", "Files:a.md", "Files:b.md"]);
  expect(rows(recentFirst([file("c.md")], entries, 1))).toEqual(["Recent:c.md", "Files:a.md"]);
  expect(recentFirst([], entries, 50)).toBeNull();
});

it("notes each file the window shows in its project's list", () => {
  const store = memory();
  const workspace = $state({ project: null as Project | null, openPath: null as string | null });
  const stop = trackRecentFiles(workspace, store);
  workspace.openPath = "a.md";
  flushSync();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  workspace.openPath = "b.md";
  flushSync();
  workspace.openPath = "c.md";
  flushSync();
  stop();
  workspace.openPath = "d.md";
  flushSync();
  expect(recentFiles("p1", store)).toEqual(["c.md", "b.md"]);
});
