// State of the git panel for one project: the status, branches, recent
// commits and the outcome of the person's last action. It refreshes on
// `fs.git_changed` and after each of its own calls; events that arrive while
// a status call runs cost one more call, not one each.

import { SvelteMap } from "svelte/reactivity";
import type { BranchInfo, CommitInfo, GitChange, Hunk, ProjectId, RepoStatus, StatusFile } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export type GitState =
  | { kind: "loading" }
  | { kind: "ready"; status: RepoStatus }
  | { kind: "no_repo" }
  | { kind: "error"; message: string };

/** Where a failure is shown: next to what the person pressed. */
export type GitErrorScope = "files" | "commit" | "branch" | "diff";

/** A failed action; `output` is a hook's or git's text, shown as is. */
export type GitError = { scope: GitErrorScope; message: string; output?: string };

export type Discarded = { undo: string; paths: string[] };

/** Commits shown under "Recent commits". */
const LOG_LIMIT = 20;

export const CHANGE_LETTER: Record<GitChange, string> = {
  added: "A",
  modified: "M",
  deleted: "D",
  renamed: "R",
  copied: "C",
  type_changed: "T",
  untracked: "U",
  conflicted: "!",
};

export const CHANGE_LABEL: Record<GitChange, string> = {
  added: "added",
  modified: "modified",
  deleted: "deleted",
  renamed: "renamed",
  copied: "copied",
  type_changed: "type changed",
  untracked: "untracked",
  conflicted: "conflicted",
};

export function message(err: unknown): string {
  return err instanceof CoreError ? err.error.message : err instanceof Error ? err.message : String(err);
}

/** `text` as a sentence of its own: capital first letter, full stop at the end. */
function sentence(text: string): string {
  const trimmed = text.trim();
  return `${trimmed.charAt(0).toUpperCase()}${trimmed.slice(1)}${/[.!?]$/.test(trimmed) ? "" : "."}`;
}

function statusError(err: unknown): string {
  if (err instanceof CoreError && err.code === "unavailable") return `Git is not available here: ${err.error.message}`;
  return `Could not read the git status: ${message(err)}`;
}

/** "src/a.ts", or "3 files". */
export function describePaths(paths: readonly string[]): string {
  return paths.length === 1 ? (paths[0] ?? "") : `${paths.length} files`;
}

/**
 * Why git would refuse `name` as a branch name (its `check-ref-format`
 * rules), with a fix when there is an obvious one; null when it looks fine.
 */
export function branchNameProblem(name: string): string | null {
  const parts = name.split("/");
  if (/\s/.test(name)) return `Branch names cannot contain spaces. Try ${name.trim().replace(/\s+/g, "-")}.`;
  if (name.startsWith("-")) return "Branch names cannot start with -.";
  if (name.includes("..")) return "Branch names cannot contain two dots in a row (..).";
  if (/[~^:?*[\\]/.test(name) || [...name].some((c) => c < " " || c === "\u007f"))
    return "Branch names cannot contain ~ ^ : ? * [ or \\.";
  if (name === "@" || name.includes("@{")) return "Branch names cannot be @ or contain @{.";
  if (parts.some((p) => p === "")) return "Branch names cannot start or end with / or contain //.";
  if (parts.some((p) => p.startsWith("."))) return "No part of a branch name can start with a dot.";
  if (parts.some((p) => p.endsWith(".lock"))) return "No part of a branch name can end with .lock.";
  if (name.endsWith(".")) return "Branch names cannot end with a dot.";
  return null;
}

/** The status's files in the panel's three lists; a partly staged file is in two. */
export function fileLists(files: readonly StatusFile[]): {
  conflicts: StatusFile[];
  staged: StatusFile[];
  changes: StatusFile[];
} {
  const conflicts = files.filter((f) => f.unstaged === "conflicted");
  const staged = files.filter((f) => f.staged !== null && f.unstaged !== "conflicted");
  const changes = files.filter((f) => f.unstaged !== null && f.unstaged !== "conflicted");
  return { conflicts, staged, changes };
}

const live = new SvelteMap<ProjectId, GitModel>();

/** The panel's model of `project`, while the panel is mounted. */
export function gitModel(project: ProjectId): GitModel | undefined {
  return live.get(project);
}

export class GitModel {
  state = $state<GitState>({ kind: "loading" });
  /** Null until the branch menu first opens. */
  branches = $state.raw<BranchInfo[] | null>(null);
  /** Null until "Recent commits" first opens. */
  log = $state.raw<CommitInfo[] | null>(null);
  logOpen = $state(false);
  /** An action is running; others wait. */
  busy = $state(false);
  /** The last discard, until the next one or its undo. */
  discarded = $state<Discarded | null>(null);
  /** The last commit made here, with its hooks' output. */
  committed = $state<{ commit: CommitInfo; output: string } | null>(null);
  error = $state<GitError | null>(null);
  /** For the panel's live region. */
  notice = $state("");
  /** Counts changes seen (events and own calls), so a diff view knows to reload. */
  version = $state(0);
  /** A palette command asks the branch menu to open, to switch or to create; the menu clears it. */
  branchMenu = $state<"switch" | "create" | null>(null);

  private loading: Promise<void> | null = null;
  private again = false;
  private disposed = false;
  private readonly offs: (() => void)[];

  constructor(
    private readonly transport: Transport,
    readonly project: ProjectId,
  ) {
    let wasOpen = true;
    this.offs = [
      transport.onEvent((event) => {
        // `fs.git_changed` only fires when the tree's badges change; staging
        // in a terminal or editing a changed file again does not change them.
        const mine = (event.type === "fs.git_changed" || event.type === "fs.changed") && event.data.project === project;
        if (event.type === "info.resync" || mine) this.changed();
      }),
      transport.onStatus?.((status) => {
        if (status === "open" && !wasOpen) this.changed();
        wasOpen = status === "open";
      }) ?? (() => {}),
    ];
    live.set(project, this);
    void this.refresh();
  }

  dispose(): void {
    this.disposed = true;
    for (const off of this.offs) off();
    if (live.get(this.project) === this) live.delete(this.project);
  }

  /** Something in the repository changed: reload what is shown. */
  changed(): void {
    this.version++;
    void this.refresh();
  }

  /** Loads the status (and the log, when open); calls during a load add one more load. */
  refresh(): Promise<void> {
    if (this.loading) {
      this.again = true;
      return this.loading;
    }
    const run = async () => {
      do {
        this.again = false;
        await this.load();
      } while (this.again && !this.disposed);
      this.loading = null;
    };
    this.loading = run();
    return this.loading;
  }

  /** Resolves once the status load under way, if any, is done. */
  loaded(): Promise<void> {
    return this.loading ?? Promise.resolve();
  }

  /**
   * The last commit's summary (its first line), or null. The protocol has no
   * full message, so a body is not known here.
   */
  async lastSummary(): Promise<string | null> {
    try {
      const reply = await this.transport.call({ method: "git.log", params: { project: this.project, limit: 1 } });
      return reply.data.commits[0]?.summary ?? null;
    } catch {
      return null;
    }
  }

  async loadBranches(): Promise<void> {
    try {
      const reply = await this.transport.call({ method: "git.branches", params: { project: this.project } });
      this.branches = reply.data.branches;
    } catch (err) {
      this.error = { scope: "branch", message: `Could not list branches: ${message(err)}` };
    }
  }

  toggleLog(): void {
    this.logOpen = !this.logOpen;
    if (this.logOpen) void this.loadLog();
  }

  stage(paths: string[]): Promise<boolean> {
    return this.act(
      "files",
      () => this.call({ method: "git.stage", params: { project: this.project, paths } }),
      `Staged ${describePaths(paths)}.`,
    );
  }

  unstage(paths: string[]): Promise<boolean> {
    return this.act(
      "files",
      () => this.call({ method: "git.unstage", params: { project: this.project, paths } }),
      `Unstaged ${describePaths(paths)}.`,
    );
  }

  discard(paths: string[]): Promise<boolean> {
    return this.act(
      "files",
      async () => {
        const reply = await this.transport.call({ method: "git.discard", params: { project: this.project, paths } });
        this.discarded = { undo: reply.data.undo, paths };
      },
      `Discarded the changes to ${describePaths(paths)}. Undo is available.`,
    );
  }

  undoDiscard(): Promise<boolean> {
    const last = this.discarded;
    if (!last) return Promise.resolve(false);
    return this.act(
      "files",
      async () => {
        await this.call({ method: "git.undo_discard", params: { project: this.project, ...last } });
        this.discarded = null;
      },
      `Put back the changes to ${describePaths(last.paths)}.`,
    );
  }

  /** Commits what is staged; true when a commit was made. */
  commit(text: string, amend: boolean): Promise<boolean> {
    this.committed = null;
    return this.act(
      "commit",
      async () => {
        const reply = await this.transport.call({
          method: "git.commit",
          params: { project: this.project, message: text, amend },
        });
        this.committed = reply.data;
        this.notice = `${amend ? "Amended" : "Committed"} ${reply.data.commit.id.slice(0, 7)}: ${reply.data.commit.summary}`;
      },
      null,
      (err) =>
        err instanceof CoreError && err.code === "conflict"
          ? { message: "The commit was not made. Git or a hook refused it:", output: err.error.message }
          : { message: `The commit was not made: ${message(err)}` },
    );
  }

  switchTo(branch: string): Promise<boolean> {
    return this.act(
      "branch",
      async () => {
        await this.call({ method: "git.switch", params: { project: this.project, branch } });
        await this.loadBranches();
      },
      `Switched to branch ${branch}.`,
      (err) => ({ message: `Could not switch to ${branch}. ${sentence(message(err))}` }),
    );
  }

  createBranch(name: string): Promise<boolean> {
    return this.act(
      "branch",
      async () => {
        await this.call({ method: "git.create_branch", params: { project: this.project, name, switch: true } });
        await this.loadBranches();
      },
      `Created branch ${name} and switched to it.`,
      (err) => ({ message: `Could not create ${name}: ${message(err)}` }),
    );
  }

  stageHunk(path: string, hunk: Hunk, unstage: boolean): Promise<boolean> {
    return this.act(
      "diff",
      () => this.call({ method: "git.stage_hunk", params: { project: this.project, path, hunk, unstage } }),
      // The diff view announces it.
      null,
      (err) =>
        err instanceof CoreError && err.code === "conflict"
          ? { message: "The file changed since this diff was shown. The diff is up to date again; try once more." }
          : { message: message(err) },
    );
  }

  private async call(request: Parameters<Transport["call"]>[0]): Promise<void> {
    await this.transport.call(request);
  }

  /** Runs one action at a time, records its outcome and refreshes afterwards, also after a failure. */
  private async act(
    scope: GitErrorScope,
    run: () => Promise<void>,
    done: string | null,
    fail: (err: unknown) => Omit<GitError, "scope"> = (err) => ({ message: message(err) }),
  ): Promise<boolean> {
    if (this.busy) return false;
    this.busy = true;
    this.error = null;
    try {
      await run();
      if (done) this.notice = done;
      return true;
    } catch (err) {
      // Each scope shows its error in an alert; the status region would say it twice.
      this.error = { scope, ...fail(err) };
      this.notice = "";
      return false;
    } finally {
      this.busy = false;
      this.changed();
      await this.loading;
    }
  }

  private async load(): Promise<void> {
    try {
      const reply = await this.transport.call({ method: "git.status", params: { project: this.project } });
      if (this.disposed) return;
      this.state = reply.type === "git.status" ? { kind: "ready", status: reply.data } : { kind: "no_repo" };
    } catch (err) {
      if (this.disposed) return;
      this.state = { kind: "error", message: statusError(err) };
    }
    if (this.logOpen && this.state.kind === "ready") await this.loadLog();
  }

  private async loadLog(): Promise<void> {
    try {
      const reply = await this.transport.call({
        method: "git.log",
        params: { project: this.project, limit: LOG_LIMIT },
      });
      if (!this.disposed) this.log = reply.data.commits;
    } catch {
      // Before the first commit there is no log; the list stays empty.
      if (!this.disposed) this.log = [];
    }
  }
}
