import { describe, expect, it } from "vitest";
import { FakeServersCore } from "./fake-servers";
import { MAX_LINES, ServerLog } from "./log.svelte";

const lines = (from: number, count: number) =>
  Array.from({ length: count }, (_, i) => ({ n: from + i, text: `l${from + i}` }));

describe("ServerLog", () => {
  it("reads every kept page and merges lines that arrived meanwhile", async () => {
    const core = new FakeServersCore();
    core.logs.set("srv1", lines(1, 1500));
    const log = new ServerLog(core, "srv1");
    const loading = log.load();
    core.emit({ type: "servers.output", data: { id: "srv1", lines: lines(1500, 2) } });
    await loading;
    expect(log.lines.map((l) => l.n)).toEqual(lines(1, 1501).map((l) => l.n));
    expect(core.calls.filter((c) => c.method === "servers.logs")).toHaveLength(2);
    log.dispose();
  });

  it("keeps only the newest lines and ignores other servers", async () => {
    const core = new FakeServersCore();
    const log = new ServerLog(core, "srv1");
    await log.load();
    core.emit({ type: "servers.output", data: { id: "srv2", lines: lines(1, 1) } });
    expect(log.lines).toEqual([]);
    core.emit({ type: "servers.output", data: { id: "srv1", lines: lines(1, MAX_LINES + 10) } });
    expect(log.lines).toHaveLength(MAX_LINES);
    expect(log.lines[0]?.n).toBe(11);
    expect(log.dropped).toBe(true);
    log.dispose();
  });
});
