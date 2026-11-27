// Words and rules for a worker's pull request (docs/specs/pr-flow.md): when
// the item offers one, what its chip says, and what stops the dialog.

import type { CheckState, GhStatus, PrCheck, PullRequest, ReviewItem } from "../lib/protocol";

/** What the dialog says when the person's `gh` cannot be used. */
export const NO_GH = "Install GitHub CLI (gh) and run gh auth login in a terminal.";

export const CHECK_LABEL: Record<CheckState, string> = {
  pending: "Running",
  passed: "Passed",
  failed: "Failed",
  skipped: "Skipped",
};

/** A worker's open item with changes and no pull request yet can become one. */
export function canOpenPr(item: ReviewItem): boolean {
  return item.mode === "isolated" && item.state === "open" && !item.pull_request && item.files.length > 0;
}

/** A worker's kept changes that can still go out as a pull request. */
export function canOpenKeptPr(item: ReviewItem): boolean {
  return item.can_publish_kept && !item.pull_request && item.files.length > 0;
}

/** A comment reopened a published item and the worker's new work waits: it can be pushed to the same pull request. */
/** GitHub runs these on a push, with the repository's secrets. */
export function isWorkflow(path: string): boolean {
  return /^\.github\/(workflows|actions)\//i.test(path);
}

/** What publishing the protected files `paths` means, in plain words. */
export function protectedWarning(paths: string[]): string {
  const files = `These files are protected: ${paths.join(", ")}.`;
  if (!paths.some(isWorkflow)) return `${files} Publish them only if you checked their diffs.`;
  return `${files} Workflow changes run on GitHub, with the repository's secrets, as soon as the branch is pushed, before anyone merges.`;
}

export function canUpdatePr(item: ReviewItem): boolean {
  return item.state === "open" && item.pull_request !== undefined && item.files.length > 0;
}

/** Null when `gh` is installed and signed in, else the one line on how to fix it. */
export function ghProblem(gh: GhStatus): string | null {
  return gh.installed && gh.signed_in ? null : NO_GH;
}

/** Who pushes and where, such as `Signed in as octo on github.com`. */
export function ghAccount(gh: GhStatus): string | null {
  if (!gh.installed || !gh.signed_in) return null;
  const who = gh.login ? `Signed in as ${gh.login}` : "Signed in";
  return gh.host ? `${who} on ${gh.host}` : who;
}

/** `3 of 5 checks passed`; null before the first look or without checks. */
export function checksLine(checks: readonly PrCheck[] | undefined): string | null {
  if (!checks || checks.length === 0) return null;
  const passed = checks.filter((c) => c.state === "passed").length;
  return `${passed} of ${checks.length} ${checks.length === 1 ? "check" : "checks"} passed`;
}

/** `PR #123 · draft · 3 of 5 checks passed`: only what is known so far. */
export function prChip(pr: PullRequest): string {
  const parts = [`PR #${pr.number}`];
  const status = pr.status;
  if (status?.state === "merged") parts.push("merged");
  else if (status?.state === "closed") parts.push("closed");
  else if (status?.draft) parts.push("draft");
  const checks = checksLine(status?.checks);
  if (checks) parts.push(checks);
  else if (status && status.state === "open") parts.push("no checks yet");
  return parts.join(" · ");
}

/** Only http and https pages are handed to the system browser. */
export function isWebUrl(url: string | null | undefined): url is string {
  return typeof url === "string" && /^https?:\/\//i.test(url);
}
