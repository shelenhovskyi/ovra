import { describe, expect, it } from "vitest";
import { CoreError } from "../lib/transport";
import { FakeReviewCore, review } from "./fake-review";
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
    expect(inbox.outcome).toEqual({ id: "r1", state: "rejected", conflicts: ["notes.md"] });
    expect(await inbox.approve("r2", "Fix the typo")).toBe(true);
    expect(core.sent("review.approve")[0]?.params).toEqual({ id: "r2", commit: "Fix the typo" });
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
});
