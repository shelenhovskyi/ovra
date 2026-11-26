// Words for what a session takes of the computer (`resources.sample`,
// docs/specs/agent-resources.md): one quiet line for the list, the split
// for its tooltip.

import type { DiskUsage, Pressure, SessionResources, Usage } from "../lib/protocol";

/** Bytes for people: `420 MB`, `1.2 GB`. */
export function bytes(n: number): string {
  const mb = n / 2 ** 20;
  if (mb < 1000) return `${Math.max(1, Math.round(mb))} MB`;
  const gb = n / 2 ** 30;
  return `${gb < 10 ? gb.toFixed(1) : Math.round(gb)} GB`;
}

function sum(r: SessionResources): Usage | null {
  const parts = [r.agent, r.servers, r.browser].filter((u): u is Usage => u !== null);
  if (parts.length === 0) return null;
  return parts.reduce((a, b) => ({
    memory_bytes: a.memory_bytes + b.memory_bytes,
    cpu_percent: a.cpu_percent + b.cpu_percent,
    processes: a.processes + b.processes,
  }));
}

/** `up to 1.2 GB on disk`: cloned files count in full, so a worktree's size is an upper bound. */
export function diskLabel(d: DiskUsage): string {
  return `${d.complete ? "up to" : "more than"} ${bytes(d.bytes)} on disk`;
}

/** `1.2 GB · 35% CPU`; empty while nothing runs. A worker's disk use has its own line. */
export function resourcesLine(r: SessionResources | undefined): string {
  const total = r && sum(r);
  if (!total) return "";
  return `${bytes(total.memory_bytes)} · ${total.cpu_percent}% CPU`;
}

/** The split behind `resourcesLine`, one part per line. */
export function resourcesTip(r: SessionResources, worktree?: DiskUsage | null): string {
  const total = sum(r);
  if (!total) return "";
  const part = (name: string, u: Usage | null) =>
    u
      ? [
          `${name}: ${bytes(u.memory_bytes)}, ${u.cpu_percent}% CPU, ${u.processes} ${u.processes === 1 ? "process" : "processes"}`,
        ]
      : [];
  return [
    `About ${bytes(total.memory_bytes)} of memory and ${total.cpu_percent}% CPU (100% is one core)`,
    ...part("Agent and its commands", r.agent),
    ...part("Dev servers", r.servers),
    ...part("Browser", r.browser),
    ...(worktree ? [`Worktree: ${diskLabel(worktree)}`] : []),
  ].join("\n");
}

/** Why queued workers wait, for their row. */
export function waitingLabel(pressure: Pressure): string {
  return pressure === "low_memory" ? "Waiting: low memory" : "Waiting: CPU busy";
}

/** The same, short, for the capacity line. */
export function pressureNote(pressure: Pressure): string {
  return pressure === "low_memory" ? "memory low" : "CPU busy";
}
