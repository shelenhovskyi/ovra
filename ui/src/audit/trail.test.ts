import { describe, expect, it } from "vitest";
import { bash, entry, FakeAuditCore } from "./fake-audit";
import { AuditTrail, MAX_SHOWN } from "./trail.svelte";

const seqs = (trail: AuditTrail) => trail.entries.map((e) => e.seq);

describe("AuditTrail", () => {
  it("reads every page of the project's log and follows new entries", async () => {
    const core = new FakeAuditCore();
    core.entries = Array.from({ length: 501 }, (_, i) => entry(i + 1, bash(true)));
    core.entries.push(entry(600, bash(true), { project: "p2" }));
    const trail = new AuditTrail(core);
    await trail.show("p1");
    expect(trail.entries).toHaveLength(501);
    expect(core.calls.map((c) => (c.method === "audit.list" ? c.params.after : null))).toEqual([null, 500]);

    core.append(entry(700, bash(false)));
    core.append(entry(701, bash(false), { project: "p2" }));
    // The same entry twice (a page and the live event) shows once.
    core.append(entry(700, bash(false)));
    expect(seqs(trail).slice(-2)).toEqual([501, 700]);
    expect(trail.entries).toHaveLength(502);
    expect(trail.cut).toBe(false);
    trail.dispose();
  });

  it("keeps the newest entries of a long log and says so", async () => {
    const core = new FakeAuditCore();
    core.entries = Array.from({ length: MAX_SHOWN + 10 }, (_, i) => entry(i + 1, bash(true)));
    const trail = new AuditTrail(core);
    await trail.show("p1");
    expect(trail.entries).toHaveLength(MAX_SHOWN);
    expect(trail.entries[0]?.seq).toBe(11);
    expect(trail.cut).toBe(true);
    // Another project starts over.
    await trail.show("p2");
    expect(trail.entries).toEqual([]);
    expect(trail.cut).toBe(false);
  });

  it("says when the log cannot be read", async () => {
    const core = new FakeAuditCore();
    core.call = () => Promise.reject(new Error("unavailable: the audit log is not available in this build"));
    const trail = new AuditTrail(core);
    await trail.show("p1");
    expect(trail.error).toContain("not available");
    expect(trail.loading).toBe(false);
  });
});
