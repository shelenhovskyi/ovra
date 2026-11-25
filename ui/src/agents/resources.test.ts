import { describe, expect, it } from "vitest";
import type { SessionResources } from "../lib/protocol";
import { bytes, resourcesLine, resourcesTip, waitingLabel } from "./resources";

const MB = 2 ** 20;
const GB = 2 ** 30;

const r = (over: Partial<SessionResources> = {}): SessionResources => ({
  session: "s1",
  agent: { memory_bytes: 800 * MB, cpu_percent: 30, processes: 6 },
  servers: null,
  browser: null,
  ...over,
});

describe("resources", () => {
  it("says sizes in MB and GB", () => {
    expect([bytes(0), bytes(420 * MB), bytes(1.24 * GB), bytes(24.4 * GB)]).toEqual([
      "1 MB",
      "420 MB",
      "1.2 GB",
      "24 GB",
    ]);
  });

  it("adds the parts up in one quiet line, with a worker's worktree", () => {
    const all = r({ servers: { memory_bytes: 400 * MB, cpu_percent: 5, processes: 2 } });
    expect(resourcesLine(all)).toBe("1.2 GB · 35% CPU");
    expect(resourcesLine(all, { bytes: 2.4 * GB, complete: false })).toBe("1.2 GB · 35% CPU · over 2.4 GB on disk");
    expect(resourcesLine(r({ agent: null }))).toBe("");
    expect(resourcesLine(undefined)).toBe("");
  });

  it("splits the parts in the tooltip", () => {
    const tip = resourcesTip(r({ browser: { memory_bytes: 100 * MB, cpu_percent: 0, processes: 1 } }), {
      bytes: 2 * GB,
      complete: true,
    });
    expect(tip.split("\n")).toEqual([
      "About 900 MB of memory and 30% CPU (100% is one core)",
      "Agent and its commands: 800 MB, 30% CPU, 6 processes",
      "Browser: 100 MB, 0% CPU, 1 process",
      "Worktree: 2.0 GB on disk",
    ]);
  });

  it("says why queued workers wait", () => {
    expect([waitingLabel("low_memory"), waitingLabel("busy_cpu")]).toEqual([
      "Waiting: low memory",
      "Waiting: CPU busy",
    ]);
  });
});
