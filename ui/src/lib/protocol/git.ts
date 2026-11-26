// Mirrors crates/core/src/protocol/git.rs. Paths are relative to the project
// root. The panel refreshes on `fs.git_changed`; there are no git events.

import type { ProjectId } from "./projects";

export type GitRev = "head" | "index";

/** One hunk of a unified diff with three lines of context. */
export type Hunk = {
  /** From 1 (0 for an empty old side). */
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  /** Git's text after the second `@@`, such as a function name. */
  header: string;
  /** Lines without breaks (CRLF files keep the `\r`), each starting with ` `, `+`, `-`, or `\` for "No newline at end of file". */
  lines: string[];
};

export type GitRequest =
  /** Answers `git.no_repo` when the project is not in a git repository. */
  | { method: "git.status"; params: { project: ProjectId } }
  /**
   * `git init` in the project folder: no commit, no remote, nothing fetched.
   * A person's action, not an agent tool. `conflict` when it is already in a
   * git repository or has a `.git` of its own. Emits `fs.git_changed`.
   */
  | { method: "git.init"; params: { project: ProjectId } }
  /** Adds the working-tree version of `paths` (new, changed or deleted) to the index. */
  | { method: "git.stage"; params: { project: ProjectId; paths: string[] } }
  /** Puts `paths` in the index back as in HEAD; the working tree is not touched. */
  | { method: "git.unstage"; params: { project: ProjectId; paths: string[] } }
  /**
   * Stages one hunk of `git.diff` (`staged: false`) of a tracked file, or with
   * `unstage` takes a hunk of the staged diff out. `conflict` when the file
   * changed since that diff.
   */
  | { method: "git.stage_hunk"; params: { project: ProjectId; path: string; hunk: Hunk; unstage?: boolean } }
  /**
   * Throws away unstaged changes: tracked files go back to the index version,
   * untracked files are deleted. Answers a snapshot for `git.undo_discard`.
   */
  | { method: "git.discard"; params: { project: ProjectId; paths: string[] } }
  | { method: "git.undo_discard"; params: { project: ProjectId; undo: string; paths: string[] } }
  /**
   * `git commit` of what is staged, with the person's config and hooks.
   * With `amend`, an empty message keeps the last commit's message.
   * `bad_request` for an empty message or nothing staged (unless `amend`);
   * a failing hook answers `conflict` with its output.
   */
  | { method: "git.commit"; params: { project: ProjectId; message: string; amend?: boolean } }
  /** Local branches, most recently committed first. */
  | { method: "git.branches"; params: { project: ProjectId } }
  /**
   * `git switch`, never forced: `conflict` when changes would be overwritten
   * or while an agent session writes the main checkout.
   */
  | { method: "git.switch"; params: { project: ProjectId; branch: string } }
  /**
   * Stops the `git.commit`, `git.switch` or switching `git.create_branch` running in the
   * project, with its hooks and gpg;
   * that request then answers `conflict`. `ok` also when nothing runs.
   */
  | { method: "git.cancel"; params: { project: ProjectId } }
  /** A branch at HEAD; `bad_request` for an invalid name, `conflict` if it exists. */
  | { method: "git.create_branch"; params: { project: ProjectId; name: string; switch?: boolean } }
  /** Index to working tree, or with `staged` HEAD to index. Untracked files diff as all added. */
  | { method: "git.diff"; params: { project: ProjectId; path: string; staged?: boolean } }
  /** A file's text in HEAD or the index. */
  | { method: "git.show"; params: { project: ProjectId; path: string; rev: GitRev } }
  /** Who last changed each line of the file in the working tree. */
  | { method: "git.blame"; params: { project: ProjectId; path: string } }
  /** Latest commits of HEAD, newest first; `limit` defaults to 50, at most 200 (MAX_LOG in Rust). */
  | { method: "git.log"; params: { project: ProjectId; limit?: number } }
  /**
   * Commits HEAD gained since the last pull, merge, rebase or reset (`ORIG_HEAD..HEAD`),
   * newest first, at most 100 (MAX_PULLED in Rust), with messages and changed files.
   */
  | { method: "git.pulled"; params: { project: ProjectId } }
  /**
   * Fetches the current branch's upstream and fast-forwards to it ("Get commits").
   * `conflict` when both have new commits, changes would be overwritten, or an agent writes the checkout.
   */
  | { method: "git.pull"; params: { project: ProjectId } }
  /** Sends the current branch's commits to its upstream, never forced ("Send commits"). */
  | { method: "git.push"; params: { project: ProjectId } }
  /**
   * Sends a branch without upstream to a new branch of its name on the remote, never forced, and
   * makes that its upstream ("Publish branch"). `conflict` when the remote has that branch already.
   */
  | { method: "git.publish"; params: { project: ProjectId } };

export type GitOperation = "merge" | "rebase" | "cherry_pick" | "revert" | "bisect";

export type GitChange =
  | "added"
  | "modified"
  | "deleted"
  | "renamed"
  | "copied"
  | "type_changed"
  | "untracked"
  | "conflicted";

export type StatusFile = {
  path: string;
  /** The old path of a staged rename or copy. */
  from: string | null;
  /** HEAD to index. */
  staged: GitChange | null;
  /** Index to working tree; `conflicted` (nothing staged) or `untracked` included. */
  unstaged: GitChange | null;
};

export type RepoStatus = {
  /** Null when HEAD is detached. */
  branch: string | null;
  /** Null before the first commit. */
  head: string | null;
  upstream: string | null;
  /** For a branch without upstream, the remote "Publish branch" sends it to; null with no remote. */
  publish_to?: string | null;
  ahead: number;
  behind: number;
  operation: GitOperation | null;
  files: StatusFile[];
  /** Files left out after 5,000 (MAX_STATUS_FILES in Rust). */
  truncated: number;
};

export type CommitInfo = {
  id: string;
  author: string;
  /** Author time, seconds since the Unix epoch. */
  time: number;
  /** First line of the message. */
  summary: string;
};

export type BranchInfo = {
  name: string;
  current: boolean;
  upstream: string | null;
  /** Null for a branch without commits. */
  commit: CommitInfo | null;
};

export type FileDiff = {
  path: string;
  from: string | null;
  /** Binary files have no hunks. */
  binary: boolean;
  hunks: Hunk[];
  /** Lines left out after 20,000; such a diff cannot be staged by hunk. */
  truncated: boolean;
};

export type GitFileText = {
  /** Null when not in that revision, binary, or over 4 MiB. */
  text: string | null;
  exists: boolean;
};

export type BlameRange = {
  /** From 1. */
  start: number;
  lines: number;
  /** Null for lines not committed yet. */
  commit: string | null;
};

export type Blame = { ranges: BlameRange[]; commits: Record<string, CommitInfo> };

export type CommitFile = {
  path: string;
  /** Null for a binary file. */
  added: number | null;
  removed: number | null;
};

export type CommitDetail = {
  commit: CommitInfo;
  /** The message after its first line, cut to 2 KiB. */
  body: string;
  /** At most 50 (MAX_PULLED_FILES in Rust). */
  files: CommitFile[];
  more_files: number;
};

export type Pulled = {
  /** `ORIG_HEAD`; null when git has none. */
  from: string | null;
  commits: CommitDetail[];
  truncated: number;
};

export type GitResponse =
  | { type: "git.status"; data: RepoStatus }
  | { type: "git.no_repo" }
  | { type: "git.discarded"; data: { undo: string } }
  | { type: "git.committed"; data: { commit: CommitInfo; output: string } }
  | { type: "git.branches"; data: { branches: BranchInfo[] } }
  | { type: "git.diff"; data: FileDiff }
  | { type: "git.file"; data: GitFileText }
  | { type: "git.blame"; data: Blame }
  | { type: "git.log"; data: { commits: CommitInfo[] } }
  | { type: "git.pulled"; data: Pulled };

/** `ResponseFor` for the git domain. */
export type GitResponseFor<M extends GitRequest["method"]> = M extends "git.status"
  ? Extract<GitResponse, { type: "git.status" | "git.no_repo" }>
  : M extends "git.discard"
    ? Extract<GitResponse, { type: "git.discarded" }>
    : M extends "git.commit"
      ? Extract<GitResponse, { type: "git.committed" }>
      : M extends "git.branches"
        ? Extract<GitResponse, { type: "git.branches" }>
        : M extends "git.diff"
          ? Extract<GitResponse, { type: "git.diff" }>
          : M extends "git.show"
            ? Extract<GitResponse, { type: "git.file" }>
            : M extends "git.blame"
              ? Extract<GitResponse, { type: "git.blame" }>
              : M extends "git.log"
                ? Extract<GitResponse, { type: "git.log" }>
                : M extends "git.pulled"
                  ? Extract<GitResponse, { type: "git.pulled" }>
                  : { type: "ok" };
