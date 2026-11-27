// What a schedule's row says: when it runs, when next, and how the last run went.

import type { LastRun, ScheduleTime } from "../lib/protocol";

export const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] as const;

/** "Every day at 03:00", "Weekdays at 09:00" or "Mon, Wed at 09:30". */
export function whenText(when: ScheduleTime): string {
  const days = [...new Set(when.days)].sort((a, b) => a - b);
  const key = days.join(",");
  const which =
    key === "1,2,3,4,5,6,7"
      ? "Every day"
      : key === "1,2,3,4,5"
        ? "Weekdays"
        : key === "6,7"
          ? "Weekends"
          : days.map((d) => DAYS[d - 1] ?? "?").join(", ");
  return `${which} at ${when.time}`;
}

/** A moment as "today 03:00", "tomorrow 03:00" or "Mon 30 Nov, 03:00", in this computer's time. */
export function momentText(ms: number, now: number): string {
  const at = new Date(ms);
  const time = at.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
  const day = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((day(at) - day(new Date(now))) / 86_400_000);
  if (days === 0) return `today ${time}`;
  if (days === 1) return `tomorrow ${time}`;
  if (days === -1) return `yesterday ${time}`;
  const date = at.toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
  return `${date}, ${time}`;
}

/** How the last run went, in one line. */
export function lastText(last: LastRun, now: number): string {
  const when = momentText(last.at_ms, now);
  switch (last.outcome) {
    case "started":
      return `Last run started ${when}`;
    case "skipped":
      return `Skipped ${when}: ${last.reason}`;
    case "missed":
      return `Missed ${when}: Ovra was not running or the computer slept`;
    case "failed":
      return `Could not start ${when}: ${last.message}`;
  }
}

/** This computer's time zone, which a saved schedule's time is in. */
export function localZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
}
