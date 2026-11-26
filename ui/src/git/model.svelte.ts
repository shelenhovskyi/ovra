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
export type GitErrorScope = "files" | "commit" | "branch" | "diff" | "setup" | "sync";

/** A failed action; `output` is a hook's or git's text, shown as is. */
export type GitError = { scope: GitErrorScope; message: string; output?: string };

export type Discarded = { undo: string; paths: string[] };

/** Commits shown under "History". */
const LOG_LIMIT = 20;

/** After this long a commit or a branch switch says it still runs, and offers Cancel. */
export const SLOW_MS = 1500;
/** How often the panel asks the remote for new commits while the window is shown. */
export const FETCH_EVERY_MS = 3 * 60_000;
/** Coming back to the window fetches again only once this long has passed. */
export const FETCH_AGAIN_MS = 60_000;
/** After a failed fetch (no network, a sign-in it could not ask for), the next waits this long. */
export const FETCH_AFTER_FAILURE_MS = 30 * 60_000;

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
  untracked: "new file",
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

/**
 * Whether the panel shows staging (two lists, Stage and Unstage): the person
 * asked for it, a merge or the like is under way, or part of the changes is
 * staged already (in a terminal, or a hunk), which committing everything
 * would sweep in. Otherwise Commit takes every change.
 */
export function showsStaging(status: RepoStatus, choosing: boolean): boolean {
  const { conflicts, staged, changes } = fileLists(status.files);
  return choosing || !!status.operation || conflicts.length > 0 || (staged.length > 0 && changes.length > 0);
}

/** Every changed file once, for the one list the panel shows without staging. */
export function changedFiles(files: readonly StatusFile[]): StatusFile[] {
  return files.filter((f) => f.unstaged !== "conflicted");
}

const live = new SvelteMap<ProjectId, GitModel>();

/**
 * Each project's auto fetches, kept while the app runs: the panel's model is made anew for
 * each project shown, and switching back must not fetch (or ask for a sign-in) again at once.
 */
type FetchState = {
  /** When the last `git.fetch` started; 0 before the first. */
  at: number;
  /** It failed for want of the network or a sign-in: the next waits `FETCH_AFTER_FAILURE_MS`. */
  failed: boolean;
  /** The branch and upstream it was for; another one may well work. */
  on: string;
};
const fetches = new Map<ProjectId, FetchState>();

/** A fetch that failed this way will likely fail again soon: git could not reach or sign in to the remote. */
function remoteFailure(err: unknown): boolean {
  return err instanceof CoreError && (err.code === "unavailable" || err.code === "internal");
}

/** The panel's model of `project`, while the panel is mounted. */
export function gitModel(project: ProjectId): GitModel | undefined {
  return live.get(project);
}

export class GitModel {
  state = $state<GitState>({ kind: "loading" });
  /** Null until the branch menu first opens. */
  branches = $state.raw<BranchInfo[] | null>(null);
  /** Null until "History" first opens. */
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
  /** A commit or branch switch still running after `SLOW_MS`: git or the person's hooks are at work. */
  slow = $state<"commit" | "branch" | null>(null);
  /** Cancel was pressed for the running action. */
  stopping = $state(false);
  /** A palette command asks the branch menu to open, to switch or to create; the menu clears it. */
  branchMenu = $state<"switch" | "create" | null>(null);
  /** The person chose which files go in the commit ("Choose files to commit"): staging shows. */
  choosing = $state(false);
  /** The next commit replaces the last one ("Change last commit…"). */
  amend = $state(false);
  /** "Get commits", "Send commits" or "Publish branch" is under way. */
  syncing = $state<"pull" | "push" | "publish" | null>(null);
  /** How many commits the last "Get commits" brought, until dismissed. */
  got = $state<number | null>(null);

  private loading: Promise<void> | null = null;
  private again = false;
  /** The fetch under way, with its status reload: "Get commits" and "Send commits" wait for it, so their refs win. */
  private fetching: Promise<void> | null = null;
  private readonly fetches: FetchState;
  private disposed = false;
  private readonly offs: (() => void)[];

  constructor(
    private readonly transport: Transport,
    readonly project: ProjectId,
  ) {
    let wasOpen = true;
    this.fetches = fetches.get(project) ?? { at: 0, failed: false, on: "" };
    fetches.set(project, this.fetches);
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
      this.fetchWhenShown(),
    ];
    live.set(project, this);
    void this.refresh().then(() => this.fetch());
  }

  /**
   * Asks the remote for new commits now and then, so "Get N commits" shows without a terminal:
   * every `FETCH_EVERY_MS` while the window is shown, and when the person comes back to it.
   */
  private fetchWhenShown(): () => void {
    if (typeof window === "undefined") return () => {};
    const shown = () => document.visibilityState === "visible";
    const timer = setInterval(() => {
      if (shown()) void this.fetch(FETCH_AGAIN_MS);
    }, FETCH_EVERY_MS);
    const back = () => {
      if (shown()) void this.fetch(FETCH_AGAIN_MS);
    };
    window.addEventListener("focus", back);
    document.addEventListener("visibilitychange", back);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", back);
      document.removeEventListener("visibilitychange", back);
    };
  }

  /**
   * `git.fetch` of the branch's upstream, unless one ran less than `gap` ms ago or the branch has
   * none. Quiet: the network may be away, and nothing the person asked for failed. The core
   * announces `fs.git_changed`, which reloads the status; reloading here too covers a lost event.
   */
  async fetch(gap = 0): Promise<void> {
    const status = this.state.kind === "ready" ? this.state.status : null;
    if (!status?.upstream || this.fetching || this.busy || this.disposed) return;
    const state = this.fetches;
    const wait = state.failed ? Math.max(gap, FETCH_AFTER_FAILURE_MS) : gap;
    if (state.at > 0 && Date.now() - state.at < wait) return;
    state.at = Date.now();
    const run = async () => {
      try {
        await this.call({ method: "git.fetch", params: { project: this.project } });
        state.failed = false;
      } catch (err) {
        // Not after a branch switch meanwhile, say: the next fetch may well work.
        state.failed = remoteFailure(err);
        return;
      }
      if (!this.disposed) await this.refresh();
    };
    this.fetching = run();
    await this.fetching;
    this.fetching = null;
  }

  /**
   * Waits for the fetch under way, holding `busy` so that no other action (nor a second
   * sync) starts meanwhile; the caller starts its `act` right after, before anything else runs.
   */
  private async afterFetch(): Promise<void> {
    this.busy = true;
    try {
      await this.fetching;
    } finally {
      this.busy = false;
    }
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

  /** `git init` in the project folder, nothing more; true when there is a repository now. */
  init(): Promise<boolean> {
    return this.act(
      "setup",
      () => this.call({ method: "git.init", params: { project: this.project } }),
      "Set up git. Changes now show here.",
      (err) => ({ message: `Could not set up git: ${message(err)}` }),
    );
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
    // "Got N commits" is old news once the person moves on.
    this.got = null;
    return this.act(
      "commit",
      async () => {
        const reply = await this.transport.call({
          method: "git.commit",
          params: { project: this.project, message: text, amend },
        });
        this.committed = reply.data;
        // The commit box shows and reads out which commit it made.
        this.notice = "";
      },
      null,
      (err) =>
        err instanceof CoreError && err.code === "conflict"
          ? {
              message: this.stopping ? "You stopped the commit." : "The commit was not made:",
              output: err.error.message,
            }
          : { message: `The commit was not made: ${message(err)}` },
      "commit",
    );
  }

  /** Stops the commit or switch that runs, with its hooks; the action then fails and says so. */
  async cancel(): Promise<void> {
    if (!this.slow || this.stopping) return;
    this.stopping = true;
    try {
      await this.transport.call({ method: "git.cancel", params: { project: this.project } });
    } catch (err) {
      this.stopping = false;
      this.notice = `Could not stop it: ${message(err)}`;
    }
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
      "branch",
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
      "branch",
    );
  }

  /** Fetches the upstream and fast-forwards to it; `count` is what the status said was new. */
  pull(count: number): Promise<boolean> {
    return this.sync("pull", count);
  }

  /** Sends the branch's commits to its upstream. */
  push(count: number): Promise<boolean> {
    return this.sync("push", count);
  }

  /** Sends a branch without upstream to a new branch on `remote` and connects the two. */
  async publish(branch: string, remote: string): Promise<boolean> {
    if (this.busy) return false;
    this.syncing = "publish";
    await this.afterFetch();
    this.got = null;
    const done = await this.act(
      "sync",
      () => this.call({ method: "git.publish", params: { project: this.project } }),
      `Published ${branch} to ${remote}.`,
      (err) => ({ message: `Could not publish the branch. ${sentence(message(err))}` }),
    );
    this.syncing = null;
    return done;
  }

  private async sync(what: "pull" | "push", count: number): Promise<boolean> {
    if (this.busy) return false;
    this.syncing = what;
    // A fetch recording the upstream after this would put it back where it was.
    await this.afterFetch();
    this.got = null;
    // The fetch may have found more.
    const status = this.state.kind === "ready" ? this.state.status : null;
    const n = status ? (what === "pull" ? status.behind : status.ahead) : count;
    const commits = `${n} ${n === 1 ? "commit" : "commits"}`;
    const done = await this.act(
      "sync",
      () => this.call({ method: what === "pull" ? "git.pull" : "git.push", params: { project: this.project } }),
      what === "pull" ? `Got ${commits}.` : `Sent ${commits}.`,
      (err) => ({ message: `Could not ${what === "pull" ? "get" : "send"} the commits. ${sentence(message(err))}` }),
    );
    this.syncing = null;
    // The remote answered: fetching on its own may go on.
    if (done) this.fetches.failed = false;
    if (done && what === "pull") this.got = n;
    return done;
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

  /**
   * Runs one action at a time, records its outcome and refreshes afterwards, also after a failure.
   * A `slow` one (it runs the person's hooks) offers Cancel once it has run for `SLOW_MS`.
   */
  private async act(
    scope: GitErrorScope,
    run: () => Promise<void>,
    done: string | null,
    fail: (err: unknown) => Omit<GitError, "scope"> = (err) => ({ message: message(err) }),
    slow?: "commit" | "branch",
  ): Promise<boolean> {
    if (this.busy) return false;
    this.busy = true;
    this.error = null;
    const timer = slow ? setTimeout(() => (this.slow = slow), SLOW_MS) : undefined;
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
      clearTimeout(timer);
      this.slow = null;
      this.stopping = false;
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
      if (reply.type === "git.status") {
        const on = `${reply.data.branch ?? ""}\n${reply.data.upstream ?? ""}`;
        if (on !== this.fetches.on) this.fetches.failed = false;
        this.fetches.on = on;
      }
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
