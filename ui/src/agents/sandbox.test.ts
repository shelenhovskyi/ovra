import { describe, expect, it } from "vitest";
import type { SessionInfo } from "../lib/protocol";
import { sandboxBadge, sandboxLine, sandboxState } from "./sandbox";

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
    // Saved before the flag existed, with a report: still sandboxed.
    const old = info({ status: { state: "stopped" }, sandbox: { name: "bwrap", network: "blocked" } });
    expect(sandboxLine(sandboxState(old))?.text).toBe("Runs in a sandbox (bwrap), no network.");
    expect(sandboxBadge(sandboxState(old))).toBe("Sandboxed");
    const open = info({ sandbox: { name: "x", network: "open" }, sandbox_reported: true });
    expect(sandboxLine(sandboxState(open))).toEqual({
      text: "Runs in a sandbox (x), network not limited.",
      warning: false,
    });
    expect(sandboxLine({ kind: "unknown" })).toBeNull();
    expect(sandboxBadge({ kind: "pending" })).toBeNull();
  });
});
