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

/** The files one worker changed, from its review item. */
export type WorkerChanges = { worker: string; paths: string[] };

/** One line of "Who changes what": a file, or a claimed path nobody changed yet. */
export type WhoRow = {
  path: string;
  /** Workers whose claims cover it, the one that claimed first first. */
  claims: string[];
  /** Workers that changed it. */
  changes: string[];
  /** Two or more workers claim or change it. */
  overlap: boolean;
};

/**
 * File to workers, from the claims held now and the files workers changed:
 * the rows where two workers meet first, then by path.
 */
export function whoChanges(entries: BoardEntry[], changes: WorkerChanges[]): WhoRow[] {
  const holders = [...held(entries)].flatMap(([worker, patterns]) => patterns.map((pattern) => ({ worker, pattern })));
  const rows = new Map<string, WhoRow>();
  const row = (path: string): WhoRow => {
    let found = rows.get(path);
    if (!found) {
      const claims = [...new Set(holders.filter((h) => covers(h.pattern, path)).map((h) => h.worker))];
      found = { path, claims, changes: [], overlap: false };
      rows.set(path, found);
    }
    return found;
  };
  for (const { worker, paths } of changes)
    for (const path of paths) {
      const r = row(path);
      if (!r.changes.includes(worker)) r.changes.push(worker);
    }
  // A claim that covers no changed file shows as itself.
  for (const { pattern } of holders) if (![...rows.keys()].some((path) => covers(pattern, path))) row(pattern);
  for (const r of rows.values()) r.overlap = new Set([...r.claims, ...r.changes]).size > 1;
  return [...rows.values()].sort((a, b) => Number(b.overlap) - Number(a.overlap) || a.path.localeCompare(b.path));
}

/** A row's workers in words: "lint claims it · docs changed it". */
export function whoText(row: WhoRow): string {
  const parts: string[] = [];
  if (row.claims.length > 0) parts.push(`${names(row.claims)} ${row.claims.length === 1 ? "claims" : "claim"} it`);
  if (row.changes.length > 0) parts.push(`${names(row.changes)} changed it`);
  return parts.join(" · ");
}

function names(list: string[]): string {
  return list.length > 1 ? `${list.slice(0, -1).join(", ")} and ${list.at(-1)}` : (list[0] ?? "");
}

/**
 * Whether a claimed path covers `path`, as the core matches claims: `*`
 * within a folder, `**` across folders, a trailing `/` for a whole folder.
 */
export function covers(pattern: string, path: string): boolean {
  if (pattern.endsWith("/")) return path === pattern.slice(0, -1) || path.startsWith(pattern);
  let source = "";
  for (const part of pattern.split(/(\*+)/))
    if (part.startsWith("**")) source += ".*";
    else if (part === "*") source += "[^/]*";
    else source += part.replace(/[.+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`^${source}$`, "s").test(path);
}

/** The board in one line: who holds what, what overlaps, and what waits on the person. */
export function summary(entries: BoardEntry[], openForPerson: number, overlaps = 0): string {
  if (entries.length === 0 && overlaps === 0) return "Nothing on the board yet.";
  const parts = [...held(entries)].map(([worker, paths]) => `${worker} holds ${count(paths.length, "path")}`);
  if (overlaps > 0) parts.push(`${count(overlaps, "file")} shared between workers`);
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
