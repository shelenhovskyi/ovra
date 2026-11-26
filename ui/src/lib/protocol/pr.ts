// Mirrors crates/core/src/protocol/pr.rs: pull requests of reviewed workers
// through the person's own `gh` (docs/specs/pr-flow.md, ADR 0010).

import type { ProjectId } from "./projects";
import type { FileChange, ReviewId } from "./review";

/** The person's GitHub CLI, as `gh --version` and `gh auth status` say. */
export type GhStatus = {
  installed: boolean;
  version: string | null;
  /** The remote's host, such as `github.com`. */
  host: string | null;
  /** The account `gh` is signed in with on that host. */
  login: string | null;
  signed_in: boolean;
};

/** A pull request to open. */
export type PrDraft = {
  /** A remote's name, such as `origin`; never a URL. */
  remote: string;
  base: string;
  branch: string;
  title: string;
  body: string;
  draft: boolean;
};

export type PrState = "open" | "closed" | "merged";

/** `failed` includes cancelled, timed out and waiting for an action; `skipped` includes neutral. */
export type CheckState = "pending" | "passed" | "failed" | "skipped";

export type PrCheck = {
  name: string;
  state: CheckState;
  /** Its page, opened through `open.url`. */
  url: string | null;
  /** Its workflow run, for `pr.send_log`; null for a check that is not a GitHub Actions run. */
  run: number | null;
};

/** What `gh pr view` says. */
export type PrStatus = {
  state: PrState;
  draft: boolean;
  url: string;
  /** The commit the PR's branch is at. */
  head: string;
  checks: PrCheck[];
  /** GitHub's `reviewDecision`, such as `APPROVED`. */
  review_decision: string | null;
  /** GitHub's `mergeStateStatus`, such as `CLEAN`. */
  merge_state: string | null;
};

/** The pull request a review item was published as. */
export type PullRequest = {
  number: number;
  url: string;
  remote: string;
  /** The repository it was opened in, `HOST/OWNER/REPO`; absent or null in older state. */
  repo?: string | null;
  base: string;
  branch: string;
  /** The last commit Ovra pushed: an update is refused when the remote branch moved from it. */
  pushed: string;
  /** Null until asked. */
  status: PrStatus | null;
};

/** What `pr.prepare` drafted for the dialog. */
export type PrPrepared = {
  remote: string;
  host: string | null;
  base: string;
  branch: string;
  /** Drafted by the worker; the person edits them. */
  title: string;
  body: string;
  files: FileChange[];
  /** Files that conflict with the fetched base: the worker must merge it first. */
  conflicts: string[];
  /** Sent back with `pr.create`. */
  fingerprint: string | null;
  gh: GhStatus;
};

/** An open issue as `gh issue list` names it, for the @ menu. */
export type IssueSummary = { number: number; title: string; url: string };

/** An issue with its body and latest comments, cut to fit one context item. Written by anyone who can open issues. */
export type Issue = IssueSummary & { text: string };

/** The open pull request from the main checkout's branch and its failed GitHub Actions checks (each has a `run`). */
export type BranchPr = { number: number; url: string; branch: string; failed: PrCheck[] };

/** The end of a failed check's log, cut to fit one context item. `pr` is the pull request's number. */
export type CheckLog = { pr: number; name: string; url: string | null; text: string };

export type PrRequest =
  /** `remote` null: `origin`. */
  | { method: "pr.status"; params: { project: ProjectId; remote?: string | null } }
  /** Null picks `origin` and its default branch. */
  | { method: "pr.prepare"; params: { id: ReviewId; remote?: string | null; base?: string | null } }
  /**
   * `include_protected`: the person chose to publish the protected files
   * listed too (`.github/workflows` among them: pushing runs them on
   * GitHub); without it a change with protected files is refused.
   */
  | {
      method: "pr.create";
      params: { id: ReviewId; draft: PrDraft; fingerprint: string | null; include_protected?: boolean };
    }
  /** Refused (`conflict`) when the remote branch moved; `include_protected` as for `pr.create`. */
  | { method: "pr.update"; params: { id: ReviewId; fingerprint: string | null; include_protected?: boolean } }
  | { method: "pr.refresh"; params: { id: ReviewId } }
  /** Sends a failed check's log to the worker as a comment. Answers `ok`. */
  | { method: "pr.send_log"; params: { id: ReviewId; run: number } }
  /** Asks the worker to merge the base branch its changes conflict with. Answers `ok`. */
  | { method: "pr.ask_update"; params: { id: ReviewId; remote?: string | null; base?: string | null } }
  /** Open issues of `origin`'s repository, newest first, 30 at most. */
  | { method: "pr.issues"; params: { project: ProjectId } }
  | { method: "pr.issue"; params: { project: ProjectId; number: number } }
  /** The open pull request from the main checkout's branch (by its upstream); null without one. */
  | { method: "pr.branch_checks"; params: { project: ProjectId } }
  /** The log of a failed check run of that pull request. */
  | { method: "pr.check_log"; params: { project: ProjectId; run: number } };

export type PrResponse =
  | { type: "pr.status"; data: GhStatus }
  | { type: "pr.prepared"; data: PrPrepared }
  | { type: "pr.pull_request"; data: PullRequest }
  | { type: "pr.issues"; data: IssueSummary[] }
  | { type: "pr.issue"; data: Issue }
  | { type: "pr.branch_checks"; data: BranchPr | null }
  | { type: "pr.check_log"; data: CheckLog };

export type PrEvent =
  /** An item's pull request or its checks changed; re-fetch `review.list`. */
  { type: "pr.changed"; data: { project: ProjectId; id: ReviewId } };

/** The response a `pr.*` method resolves to. */
export type PrResponseFor<M extends PrRequest["method"]> = M extends "pr.status"
  ? Extract<PrResponse, { type: "pr.status" }>
  : M extends "pr.prepare"
    ? Extract<PrResponse, { type: "pr.prepared" }>
    : M extends "pr.send_log" | "pr.ask_update"
      ? { type: "ok" }
      : M extends "pr.issues" | "pr.issue" | "pr.branch_checks" | "pr.check_log"
        ? Extract<PrResponse, { type: M }>
        : Extract<PrResponse, { type: "pr.pull_request" }>;
