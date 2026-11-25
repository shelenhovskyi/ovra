import { describe, expect, it } from "vitest";
import type { SessionInfo } from "../lib/protocol";
import { sandboxBadge, sandboxGaps, sandboxLine, sandboxState } from "./sandbox";

const info = (extra: Partial<SessionInfo>): SessionInfo => ({
  id: "s1",
  project: "p1",
  agent: "claude",
  mode: "direct",
  status: { state: "working" },
  title: "t",
  usage: { input_tokens: 0, output_tokens: 0, cost_micro_usd: null },
  resumable: false,
  billing: null,
  sandbox: null,
  sandbox_reported: false,
  ...extra,
});

describe("sandbox words", () => {
  it("tells starting, none, sandboxed and unknown apart", () => {
    expect(sandboxState(info({}))).toEqual({ kind: "pending" });
    expect(sandboxState(info({ sandbox_reported: true }))).toEqual({ kind: "none" });
    expect(sandboxState(info({ status: { state: "stopped" } }))).toEqual({ kind: "unknown" });
    // Saved before the flag existed, with a report: still sandboxed, in the past.
    const old = info({ status: { state: "stopped" }, sandbox: { name: "bwrap", network: "blocked", gaps: [] } });
    expect(sandboxLine(sandboxState(old), true)?.text).toBe("Ran in a sandbox (bwrap), no network.");
    expect(sandboxBadge(sandboxState(old))).toEqual({ text: "Sandboxed", warning: false });
    expect(sandboxLine({ kind: "none" }, true)).toEqual({
      text: "Ran without a sandbox: the agent could read and change anything your user can.",
      warning: true,
    });
    expect(sandboxLine({ kind: "unknown" }, true)).toBeNull();
    expect(sandboxBadge({ kind: "pending" })).toBeNull();
  });

  it("warns about an open network even inside a sandbox", () => {
    const open = sandboxState(info({ sandbox: { name: "x", network: "open", gaps: [] }, sandbox_reported: true }));
    expect(sandboxLine(open, false)).toEqual({ text: "Runs in a sandbox (x), network not limited.", warning: true });
    expect(sandboxBadge(open)).toEqual({ text: "Sandboxed, open network", warning: true });
  });

  it("lists what the sandbox does not cover, and nothing without gaps or a sandbox", () => {
    const gaps = ["DNS lookups", "loopback services", "shared folders"];
    const boxed = sandboxState(info({ sandbox: { name: "x", network: "proxy", gaps }, sandbox_reported: true }));
    expect(sandboxGaps(boxed, false)).toBe("Not covered: DNS lookups, loopback services, shared folders.");
    expect(sandboxGaps(boxed, true)).toBe("Did not cover: DNS lookups, loopback services, shared folders.");
    const tight = sandboxState(info({ sandbox: { name: "x", network: "proxy", gaps: [] }, sandbox_reported: true }));
    expect(sandboxGaps(tight, false)).toBeNull();
    expect(sandboxGaps({ kind: "none" }, false)).toBeNull();
  });
});
