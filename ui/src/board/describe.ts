// Board entries in words for people: who wrote one, what it says, and the
// task's board in one line.

import type { Author, BoardEntry, Recipient } from "../lib/protocol";

export function authorName(author: Author): string {
  switch (author.kind) {
    case "person":
      return "You";
    case "coordinator":
      return "Coordinator";
    case "worker":
      return author.name;
    case "ovra":
      return "Ovra";
  }
}

function recipientName(to: Recipient): string {
  if (to.kind === "person") return "you";
  return to.kind === "coordinator" ? "the coordinator" : to.name;
}

/** The entry's first words, before its text: what kind of entry it is. */
export function entryHead(entry: BoardEntry, entries: BoardEntry[]): string {
  const body = entry.body;
  switch (body.kind) {
    case "claim":
      return entry.author.kind === "worker" && entry.author.name === body.worker ? "Takes" : `Gives ${body.worker}`;
    case "release":
      return "Gives back";
    case "question":
      return `Asks ${recipientName(body.to)}`;
    case "answer": {
      const asked = entries.find((e) => e.id === body.question);
      return asked ? `Answers ${authorName(asked.author)}` : "Answers";
    }
    case "note":
      return "Note";
    case "ovra":
      return "Noticed";
  }
}

/** The entry's own words: its text, or the paths it names. */
export function entryText(entry: BoardEntry): string {
  const body = entry.body;
  if (body.kind === "claim") return body.note ? `${body.paths.join(", ")} · ${body.note}` : body.paths.join(", ");
  if (body.kind === "release") return body.paths.join(", ");
  return body.text;
}

/** Which paths each worker holds now: its claims less what it gave back, in claim order. */
export function held(entries: BoardEntry[]): Map<string, string[]> {
  const byWorker = new Map<string, string[]>();
  for (const { body } of entries) {
    if (body.kind === "claim") {
      const paths = byWorker.get(body.worker) ?? [];
      byWorker.set(body.worker, [...paths, ...body.paths.filter((p) => !paths.includes(p))]);
    } else if (body.kind === "release") {
      const left = (byWorker.get(body.worker) ?? []).filter((p) => !body.paths.includes(p));
      if (left.length > 0) byWorker.set(body.worker, left);
      else byWorker.delete(body.worker);
    }
  }
  return byWorker;
}

/** The board in one line: who holds what, and what waits on the person. */
export function summary(entries: BoardEntry[], openForPerson: number): string {
  if (entries.length === 0) return "Nothing on the board yet.";
  const parts = [...held(entries)].map(([worker, paths]) => `${worker} holds ${count(paths.length, "path")}`);
  if (openForPerson > 0) parts.push(`${count(openForPerson, "question")} for you`);
  return parts.length > 0 ? parts.join(" · ") : count(entries.length, "entry", "entries");
}

function count(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/**
 * When an entry was written, in local time: `14:05` today, `Nov 25, 14:05`
 * on another day; empty for an entry saved before entries had a time.
 */
export function entryTime(entry: BoardEntry, now: Date): string {
  if (entry.at_ms === undefined) return "";
  const at = new Date(entry.at_ms);
  const clock = `${String(at.getHours()).padStart(2, "0")}:${String(at.getMinutes()).padStart(2, "0")}`;
  const today =
    at.getFullYear() === now.getFullYear() && at.getMonth() === now.getMonth() && at.getDate() === now.getDate();
  return today ? clock : `${MONTHS[at.getMonth()]} ${at.getDate()}, ${clock}`;
}
