import { describe, expect, it } from "vitest";
import type { Request } from "../lib/protocol";
import { bash, entry, FakeAuditCore } from "./fake-audit";
import { AuditTrail, MAX_SHOWN, PAGE } from "./trail.svelte";

const seqs = (trail: AuditTrail) => trail.entries.map((e) => e.seq);
const params = (calls: Request[]) => calls.map((c) => (c.method === "audit.list" ? c.params : null));
const log = (length: number) => Array.from({ length }, (_, i) => entry(i + 1, bash(true)));

type ListParams = Extract<Request, { method: "audit.list" }>["params"];

/** Holds the `audit.list` calls `which` picks until the returned function is called. */
function hold(core: FakeAuditCore, which: (params: ListParams) => boolean): () => void {
  let release = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const call = core.call.bind(core);
  core.call = (async (request: Request) => {
    if (request.method === "audit.list" && which(request.params)) await held;
    return call(request);
  }) as typeof core.call;
  return release;
}

describe("AuditTrail", () => {
  it("opens on the newest page and follows new entries", async () => {
    const core = new FakeAuditCore();
    core.entries = log(PAGE + 1);
    core.entries.push(entry(600, bash(true), { project: "p2" }));
    const trail = new AuditTrail(core);
    await trail.show("p1");
    expect(trail.entries).toHaveLength(PAGE);
    expect(trail.entries[0]?.seq).toBe(2);
    expect(trail.older).toBe(true);
    expect(params(core.calls)).toEqual([{ project: "p1", order: "newest_first", limit: PAGE }]);

    core.append(entry(700, bash(false)));
    core.append(entry(701, bash(false), { project: "p2" }));
    // The same entry twice (a page and the live event) shows once.
    core.append(entry(700, bash(false)));
    expect(seqs(trail).slice(-2)).toEqual([501, 700]);
    expect(trail.entries).toHaveLength(PAGE + 1);
    trail.dispose();
  });

  it("a short log needs one page and has nothing older", async () => {
    const core = new FakeAuditCore();
    core.entries = log(3);
    const trail = new AuditTrail(core);
    await trail.show("p1");
    expect(seqs(trail)).toEqual([1, 2, 3]);
    expect(trail.older).toBe(false);
    expect(await trail.loadOlder()).toBe(0);
    expect(core.calls).toHaveLength(1);
  });

  it("loads older pages back to the start of the log", async () => {
    const core = new FakeAuditCore();
    core.entries = log(2 * PAGE + 1);
    const trail = new AuditTrail(core);
    await trail.show("p1");
    expect(await trail.loadOlder()).toBe(PAGE);
    expect(trail.entries).toHaveLength(2 * PAGE);
    expect(trail.older).toBe(true);
    expect(await trail.loadOlder()).toBe(1);
    expect(trail.entries).toHaveLength(2 * PAGE + 1);
    expect(trail.entries[0]?.seq).toBe(1);
    expect(trail.older).toBe(false);
    expect(params(core.calls).slice(1)).toEqual([
      { project: "p1", before: PAGE + 2, order: "newest_first", limit: PAGE },
      { project: "p1", before: 2, order: "newest_first", limit: PAGE },
    ]);
  });

  it("catches up after the newest shown entry when it reloads", async () => {
    const core = new FakeAuditCore();
    core.entries = log(3);
    const trail = new AuditTrail(core);
    await trail.show("p1");
    // Appended while disconnected: no live events.
    core.entries.push(...Array.from({ length: PAGE + 2 }, (_, i) => entry(i + 4, bash(true))));
    await trail.reload();
    expect(trail.entries).toHaveLength(PAGE + 5);
    expect(params(core.calls).slice(1)).toEqual([
      { project: "p1", after: 3, limit: PAGE },
      { project: "p1", after: PAGE + 3, limit: PAGE },
    ]);
  });

  it("drops the oldest from view past the cap, and can load them again", async () => {
    const core = new FakeAuditCore();
    core.entries = log(3);
    const trail = new AuditTrail(core);
    await trail.show("p1");
    core.entries.push(...Array.from({ length: MAX_SHOWN - 2 }, (_, i) => entry(i + 4, bash(true))));
    await trail.reload();
    expect(trail.entries).toHaveLength(MAX_SHOWN);
    expect(trail.entries[0]?.seq).toBe(2);
    expect(trail.older).toBe(true);
    await trail.loadOlder();
    expect(trail.entries[0]?.seq).toBe(1);
    expect(trail.entries).toHaveLength(MAX_SHOWN + 1);
    // Another project starts over.
    await trail.show("p2");
    expect(trail.entries).toEqual([]);
    expect(trail.older).toBe(false);
  });

  it("an entry appended while an older page loads leaves no gap", async () => {
    const core = new FakeAuditCore();
    core.entries = log(3);
    const trail = new AuditTrail(core);
    await trail.show("p1");
    core.entries.push(...Array.from({ length: MAX_SHOWN - 2 }, (_, i) => entry(i + 4, bash(true))));
    await trail.reload();
    expect(trail.entries[0]?.seq).toBe(2);
    const release = hold(core, (params) => params.before !== undefined);
    const loading = trail.loadOlder();
    core.append(entry(MAX_SHOWN + 2, bash(true)));
    release();
    await loading;
    expect(seqs(trail)).toEqual(Array.from({ length: MAX_SHOWN + 2 }, (_, i) => i + 1));
  });

  it("a reload does not undo what an older page found out", async () => {
    const core = new FakeAuditCore();
    core.entries = log(PAGE + 1);
    const trail = new AuditTrail(core);
    await trail.show("p1");
    expect(trail.older).toBe(true);
    const release = hold(core, (params) => params.after !== undefined);
    const reloading = trail.reload();
    await trail.loadOlder();
    expect(trail.older).toBe(false);
    release();
    await reloading;
    expect(trail.older).toBe(false);
    expect(trail.entries).toHaveLength(PAGE + 1);
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
