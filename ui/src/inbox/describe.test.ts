import { describe, expect, it } from "vitest";
import { approveHint, defaultCommitMessage, fileMarkers, fileSummary, itemSummary, rejectHint } from "./describe";
import { change, review } from "./fake-review";

describe("inbox wording", () => {
  it("marks files changed outside the agent and files waiting for the direct agent", () => {
    expect(fileMarkers(change("a"))).toEqual([]);
    expect(fileMarkers(change("a", { by: "outside" }))).toEqual(["Changed outside the agent"]);
    expect(fileMarkers(change("a", { by: "both", blocked: true }))).toEqual([
      "Also changed outside the agent",
      "Waits for the direct agent",
    ]);
    expect(fileSummary(change("b", { kind: "renamed", from: "a", additions: 0, deletions: 0 }))).toBe(
      "renamed from a, +0 −0",
    );
  });

  it("sums up an item and says what approve and reject do", () => {
    const worker = review("r1", {
      mode: "isolated",
      files: [change("a"), change("b", { blocked: true }), change("c", { by: "outside" })],
      summary: "Fix the typo\n\nDetails",
    });
    expect(itemSummary(worker)).toBe("3 files, 1 changed outside the agent, 1 waiting for the direct agent");
    expect(approveHint(worker)).toContain("except files that wait for the direct agent");
    expect(approveHint(worker)).toContain("Merging its branch stays with you.");
    expect(rejectHint(worker)).toBe("Reject discards the worker's changes. Your checkout is not touched.");
    expect(rejectHint(review("r2", { files: [change("a", { by: "both" })] }))).toContain(
      "Changes made outside the agent stay as they are",
    );
    expect(defaultCommitMessage(worker)).toBe("Fix the typo");
    expect(defaultCommitMessage(review("r3"))).toBe("");
  });
});
