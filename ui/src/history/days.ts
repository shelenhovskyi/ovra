// How History groups and dates past sessions: by the day of their latest
// activity, in the computer's time zone, newest first as the core sends them.

import type { HistoryEntry } from "../lib/protocol";

export type DayGroup = { label: string; entries: HistoryEntry[] };

const DAY_MS = 24 * 60 * 60 * 1000;

function startOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** "Today", "Yesterday", a weekday within the week, else the date; "Earlier" for sessions without a time. */
export function dayLabel(atMs: number | null | undefined, nowMs: number): string {
  if (atMs == null) return "Earlier";
  const days = Math.round((startOfDay(nowMs) - startOfDay(atMs)) / DAY_MS);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  const date = new Date(atMs);
  if (days < 7) return date.toLocaleDateString("en-US", { weekday: "long" });
  const sameYear = date.getFullYear() === new Date(nowMs).getFullYear();
  return date.toLocaleDateString("en-US", {
    month: "long",
    day: "numeric",
    ...(sameYear ? {} : { year: "numeric" }),
  });
}

/** The time of day, as 14:05. */
export function timeLabel(atMs: number | null | undefined): string {
  if (atMs == null) return "";
  const d = new Date(atMs);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** `entries` in runs of the same day, keeping their order. */
export function byDay(entries: HistoryEntry[], nowMs: number): DayGroup[] {
  const groups: DayGroup[] = [];
  for (const entry of entries) {
    const label = dayLabel(entry.session.active_ms ?? entry.session.started_ms, nowMs);
    const last = groups.at(-1);
    if (last?.label === label) last.entries.push(entry);
    else groups.push({ label, entries: [entry] });
  }
  return groups;
}
