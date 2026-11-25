import { describe, expect, it } from "vitest";
import type { SessionInfo } from "../lib/protocol";
import { listWarning, NO_SANDBOX, sandboxGaps, sandboxLine, sandboxState } from "./sandbox";

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
  account: null,
  ...extra,
});

describe("sandbox words", () => {
  it("tells starting, none, sandboxed and unknown apart", () => {
    expect(sandboxState(info({}))).toEqual({ kind: "pending" });
    expect(sandboxState(info({ sandbox_reported: true }))).toEqual({ kind: "none" });
    expect(sandboxState(info({ status: { state: "stopped" } }))).toEqual({ kind: "unknown" });
    // Saved before the flag existed, with a report: still sandboxed, in the past.
    const old = info({ status: { state: "stopped" }, sandbox: { name: "bwrap", network: "blocked", gaps: [] } });
    // One short line; the whole sentence goes under Details.
    expect(sandboxLine(sandboxState(old), true)).toEqual({
      text: "Sandboxed · no network",
      warning: false,
      more: "Ran in a sandbox (bwrap), no network.",
    });
    // One wording for no sandbox, wherever it is said.
    expect(sandboxLine({ kind: "none" }, true)).toEqual({ text: NO_SANDBOX, warning: true, more: null });
    expect(NO_SANDBOX).toBe("No sandbox: Claude can change anything you can");
    expect(sandboxLine({ kind: "unknown" }, true)).toBeNull();
    // Said once for the list; ended sessions and pending ones say nothing.
    expect(listWarning([old, info({})])).toBeNull();
    expect(listWarning([old, info({ sandbox_reported: true })])).toBe(NO_SANDBOX);
  });

  it("warns about an open network even inside a sandbox", () => {
    const open = sandboxState(info({ sandbox: { name: "x", network: "open", gaps: [] }, sandbox_reported: true }));
    expect(sandboxLine(open, false)).toEqual({
      text: "Sandboxed · network not limited",
      warning: true,
      more: "Runs in a sandbox (x), network not limited.",
    });
    expect(listWarning([info({ sandbox: { name: "x", network: "open", gaps: [] }, sandbox_reported: true })])).toBe(
      "Sandboxed, network not limited",
    );
    expect(open.kind).toBe("sandboxed");
  });

  it("lists what the sandbox does not cover, and nothing without gaps or a sandbox", () => {
    const gaps = ["DNS lookups", "loopback services", "shared folders"];
    const boxed = sandboxState(info({ sandbox: { name: "x", network: "proxy", gaps }, sandbox_reported: true }));
    expect(sandboxGaps(boxed, false)).toBe("Not covered: DNS lookups, loopback services, shared folders.");
    expect(sandboxGaps(boxed, true)).toBe("Did not cover: DNS lookups, loopback services, shared folders.");
    // The line only says there are gaps; the list is under Details.
    expect(sandboxLine(boxed, false)?.text).toBe("Sandboxed · gaps");
    const tight = sandboxState(info({ sandbox: { name: "x", network: "proxy", gaps: [] }, sandbox_reported: true }));
    expect(sandboxGaps(tight, false)).toBeNull();
    expect(sandboxGaps({ kind: "none" }, false)).toBeNull();
  });
});
