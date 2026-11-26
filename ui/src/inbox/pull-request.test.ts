import { describe, expect, it } from "vitest";
import type { PrCheck, PullRequest } from "../lib/protocol";
import { review } from "./fake-review";
import { canOpenPr, canUpdatePr, checksLine, ghAccount, ghProblem, isWebUrl, NO_GH, prChip } from "./pull-request";

const check = (state: PrCheck["state"]): PrCheck => ({ name: state, state, url: null, run: null });
const pr = (extra: Partial<PullRequest> = {}): PullRequest => ({
  number: 123,
  url: "https://github.com/o/r/pull/123",
  remote: "origin",
  base: "main",
  branch: "ovra/t1/docs",
  pushed: "c1",
  status: null,
  ...extra,
});
const status = (extra: Partial<NonNullable<PullRequest["status"]>> = {}) => ({
  state: "open" as const,
  draft: true,
  url: "https://github.com/o/r/pull/123",
  head: "c1",
  checks: [],
  review_decision: null,
  merge_state: null,
  ...extra,
});

describe("pull request words", () => {
  it("says the number, draft and how many checks passed, as far as known", () => {
    expect(prChip(pr())).toBe("PR #123");
    expect(prChip(pr({ status: status() }))).toBe("PR #123 · draft · no checks yet");
    const checks = [check("passed"), check("passed"), check("passed"), check("failed"), check("pending")];
    expect(prChip(pr({ status: status({ checks }) }))).toBe("PR #123 · draft · 3 of 5 checks passed");
    expect(prChip(pr({ status: status({ draft: false, state: "merged", checks }) }))).toBe(
      "PR #123 · merged · 3 of 5 checks passed",
    );
    expect(prChip(pr({ status: status({ draft: false, state: "closed" }) }))).toBe("PR #123 · closed");
    expect(checksLine([check("passed")])).toBe("1 of 1 check passed");
  });

  it("offers a pull request only for a worker's open item with changes, and an update once it reopens", () => {
    const worker = review("r1", { mode: "isolated" });
    expect(canOpenPr(worker)).toBe(true);
    expect(canOpenPr(review("r1"))).toBe(false);
    expect(canOpenPr({ ...worker, files: [] })).toBe(false);
    expect(canOpenPr({ ...worker, state: "approved" })).toBe(false);
    const published = { ...worker, state: "published" as const, pull_request: pr() };
    expect(canOpenPr(published)).toBe(false);
    expect(canUpdatePr(published)).toBe(false);
    expect(canUpdatePr({ ...published, state: "open" })).toBe(true);
  });

  it("says how to get gh when it is missing or signed out, and whose account pushes otherwise", () => {
    const gh = { installed: true, version: "2.60.0", host: "github.com", login: "octo", signed_in: true };
    expect(ghProblem(gh)).toBeNull();
    expect(ghAccount(gh)).toBe("Signed in as octo on github.com");
    expect(ghProblem({ ...gh, signed_in: false, login: null })).toBe(NO_GH);
    expect(ghProblem({ ...gh, installed: false, signed_in: false })).toBe(NO_GH);
    expect(ghAccount({ ...gh, signed_in: false })).toBeNull();
  });

  it("hands only web pages to the browser", () => {
    expect(isWebUrl("https://github.com/o/r/pull/1")).toBe(true);
    expect(isWebUrl("javascript:alert(1)")).toBe(false);
    expect(isWebUrl(null)).toBe(false);
  });
});
