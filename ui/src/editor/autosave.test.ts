import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { AUTO_SAVE_DELAY_MS, AutoSaver, type Saveable } from "./autosave";
import type { AutoSave } from "./prefs.svelte";

let mode: AutoSave = "after_delay";
const saver = new AutoSaver(() => mode);

function buffer(over: Partial<Saveable> = {}): Saveable & { saves: number } {
  const b = {
    status: "ready",
    dirty: true,
    conflict: false,
    deleted: false,
    error: null as string | null,
    saves: 0,
    save: async () => {
      b.saves++;
      b.dirty = false;
      return true;
    },
    ...over,
  };
  return b;
}

beforeEach(() => {
  vi.useFakeTimers();
  mode = "after_delay";
});
afterEach(() => {
  vi.useRealTimers();
});

test("after a delay: saves once, a second after the last edit", () => {
  const b = buffer();
  saver.edited(b);
  vi.advanceTimersByTime(AUTO_SAVE_DELAY_MS - 100);
  saver.edited(b);
  vi.advanceTimersByTime(AUTO_SAVE_DELAY_MS - 1);
  expect(b.saves).toBe(0);
  vi.advanceTimersByTime(1);
  expect(b.saves).toBe(1);
  // Leaving the file does nothing in this mode.
  b.dirty = true;
  saver.left([b]);
  expect(b.saves).toBe(1);
});

test("never saves over a conflict, a deleted file, a failed save or a discarded buffer", () => {
  const blocked = [buffer({ conflict: true }), buffer({ deleted: true }), buffer({ error: "disk full" })];
  for (const b of blocked) saver.edited(b);
  const discarded = buffer();
  saver.edited(discarded);
  saver.cancel(discarded);
  vi.advanceTimersByTime(AUTO_SAVE_DELAY_MS);
  expect([...blocked, discarded].map((b) => b.saves)).toEqual([0, 0, 0, 0]);
});

test("turned off or switched mode while waiting: no save", () => {
  const b = buffer();
  saver.edited(b);
  mode = "off";
  vi.advanceTimersByTime(AUTO_SAVE_DELAY_MS);
  saver.left([b]);
  expect(b.saves).toBe(0);
});

test("on focus change: saves the dirty buffers it is given, not after edits", () => {
  mode = "on_focus_change";
  const a = buffer();
  const clean = buffer({ dirty: false });
  saver.edited(a);
  vi.advanceTimersByTime(AUTO_SAVE_DELAY_MS);
  expect(a.saves).toBe(0);
  saver.left([a, clean]);
  saver.left([a]);
  expect([a.saves, clean.saves]).toEqual([1, 0]);
});
