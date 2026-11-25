import { expect, test } from "vitest";
import { MemoryStorage } from "./fake-core";
import { EditorPrefs } from "./prefs.svelte";

test("preferences start at the defaults and survive a reload", () => {
  const storage = new MemoryStorage();
  const prefs = new EditorPrefs(storage);
  expect([prefs.fontSize, prefs.wordWrap, prefs.tabSize, prefs.autoSave]).toEqual([13.5, false, 4, "off"]);
  prefs.zoom(1);
  prefs.wordWrap = true;
  prefs.tabSize = 2;
  prefs.autoSave = "after_delay";
  const again = new EditorPrefs(storage);
  expect([again.fontSize, again.wordWrap, again.tabSize, again.autoSave]).toEqual([14.5, true, 2, "after_delay"]);
  again.zoom(0);
  expect(new EditorPrefs(storage).fontSize).toBe(13.5);
});

test("font size and tab size stay within their limits; bad values are ignored", () => {
  const prefs = new EditorPrefs(null);
  for (let i = 0; i < 20; i++) prefs.zoom(1);
  expect(prefs.fontSize).toBe(24);
  for (let i = 0; i < 20; i++) prefs.zoom(-1);
  expect(prefs.fontSize).toBe(10);
  prefs.fontSize = Number.NaN;
  prefs.tabSize = 40.4;
  prefs.autoSave = "sometimes" as never;
  expect([prefs.fontSize, prefs.tabSize, prefs.autoSave]).toEqual([10, 16, "off"]);
});

test("unreadable or failing storage gives the defaults and keeps changes for the page", () => {
  const storage = new MemoryStorage();
  storage.setItem("ovra.editor.prefs", '{"fontSize": 99, "wordWrap": "yes", "autoSave": 3');
  expect(new EditorPrefs(storage).fontSize).toBe(13.5);
  storage.setItem("ovra.editor.prefs", '{"fontSize": 99, "wordWrap": "yes"}');
  expect([new EditorPrefs(storage).fontSize, new EditorPrefs(storage).wordWrap]).toEqual([24, false]);
  const full = {
    getItem: () => {
      throw new Error("blocked");
    },
    setItem: () => {
      throw new Error("full");
    },
  };
  const prefs = new EditorPrefs(full);
  prefs.wordWrap = true;
  expect(prefs.wordWrap).toBe(true);
});
