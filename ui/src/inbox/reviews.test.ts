import { describe, expect, it, vi } from "vitest";
import { CoreError } from "../lib/transport";
import { change, FakeReviewCore, review } from "./fake-review";
import { ReviewInbox } from "./reviews.svelte";

async function setup() {
  const core = new FakeReviewCore();
  core.items = [review("r1"), review("r2", { state: "approved" }), review("r3", { project: "p2" })];
  const inbox = new ReviewInbox(core);
  await inbox.show("p1");
  return { core, inbox };
}

describe("ReviewInbox", () => {
  it("lists the open project's items and counts the open ones", async () => {
    const { inbox } = await setup();
    expect(inbox.items.map((i) => i.id)).toEqual(["r1", "r2"]);
    expect(inbox.open.map((i) => i.id)).toEqual(["r1"]);
  });

  it("counts a partly approved item as waiting until it is resolved", async () => {
    const { core, inbox } = await setup();
    core.items.push(review("r4", { state: "partly_approved", not_copied: ["README.md"] }));
    await inbox.reload();
    expect(inbox.waiting.map((i) => i.id)).toEqual(["r1", "r4"]);
    expect(await inbox.resolve("r4")).toBe(true);
    expect(inbox.waiting.map((i) => i.id)).toEqual(["r1"]);
    expect(inbox.outcome?.action).toBe("resolved");
  });

  it("re-fetches when an item of its project changes, and bumps `changes`", async () => {
    const { core, inbox } = await setup();
    core.emit({ type: "review.changed", data: { project: "p2", id: "r3" } });
    expect(core.sent("review.list")).toHaveLength(1);
    core.items.push(review("r4"));
    core.emit({ type: "review.changed", data: { project: "p1", id: "r4" } });
    await inbox.reload();
    expect(inbox.items.map((i) => i.id)).toEqual(["r1", "r2", "r4"]);
    expect(inbox.changes).toBe(1);
  });

  it("keeps the item the core answers and what it left for the person", async () => {
    const { core, inbox } = await setup();
    core.conflicts = ["notes.md"];
    expect(await inbox.reject("r1")).toBe(true);
    expect(inbox.items[0]?.state).toBe("rejected");
    expect(inbox.outcome).toEqual({
      id: "r1",
      action: "rejected",
      commit: null,
      conflicts: ["notes.md"],
      restored: [],
    });
    expect(await inbox.approve("r2", "Fix the typo")).toBe(true);
    expect(core.sent("review.approve")[0]?.params).toEqual({ id: "r2", commit: "Fix the typo" });
  });

  it("approves a worker's item with the fingerprint of the files it shows", async () => {
    const { core, inbox } = await setup();
    core.items.push(review("r4", { mode: "isolated", fingerprint: "f-seen" }));
    await inbox.reload();
    core.items[core.items.length - 1] = review("r4", { mode: "isolated", fingerprint: "f-newer" });
    expect(await inbox.approve("r4", null)).toBe(true);
    expect(core.sent("review.approve")[0]?.params).toEqual({ id: "r4", commit: null, fingerprint: "f-seen" });

    // Refused because the files moved on: the list shows them as they are now.
    core.items.push(review("r5", { mode: "isolated", fingerprint: "f-old" }));
    await inbox.reload();
    core.items[core.items.length - 1] = review("r5", { mode: "isolated", fingerprint: "f-now" });
    core.failNext.set("review.approve", new CoreError({ code: "conflict", message: "review them again" }));
    expect(await inbox.approve("r5", null)).toBe(false);
    await vi.waitFor(() => expect(inbox.items.find((i) => i.id === "r5")?.fingerprint).toBe("f-now"));
  });

  it("sends one action at a time per item and reports a failure", async () => {
    const { core, inbox } = await setup();
    const first = inbox.comment("r1", "Use a test");
    expect(await inbox.comment("r1", "again")).toBe(false);
    expect(await first).toBe(true);
    core.failNext.set("review.approve", new CoreError({ code: "conflict", message: "the diff changed" }));
    expect(await inbox.approve("r1", null)).toBe(false);
    expect(inbox.error).toBe("the diff changed");
    expect(core.sent("review.comment").map((c) => c.params.text)).toEqual(["Use a test"]);
  });

  it("says why the list is missing", async () => {
    const core = new FakeReviewCore();
    core.failNext.set("review.list", new CoreError({ code: "unavailable", message: "not ready" }));
    const inbox = new ReviewInbox(core);
    await inbox.show("p1");
    expect(inbox.error).toBe("Could not list reviews: not ready");
  });

  it("re-fetches once for a burst of file changes or a resync", async () => {
    const { core, inbox } = await setup();
    const before = core.sent("review.list").length;
    core.emit({ type: "fs.changed", data: { project: "p1", paths: ["a"] } });
    core.emit({ type: "fs.changed", data: { project: "p1", paths: ["b"] } });
    core.emit({ type: "fs.files_changed", data: { project: "p1", added: ["c"], removed: [], version: 2 } });
    core.emit({ type: "fs.changed", data: { project: "p2", paths: ["a"] } });
    await vi.waitFor(() => expect(core.sent("review.list").length).toBe(before + 2));
    core.emit({ type: "info.resync", data: { skipped: 3 } });
    await vi.waitFor(() => expect(core.sent("review.list").length).toBe(before + 3));
    expect(inbox.changes).toBe(3);
  });

  it("counts only items with the agent's files as waiting", async () => {
    const core = new FakeReviewCore();
    core.items = [review("r1"), review("r2", { files: [change("a", { by: "outside" })] })];
    const inbox = new ReviewInbox(core);
    await inbox.show("p1");
    expect(inbox.waiting.map((i) => i.id)).toEqual(["r1"]);
  });
});
