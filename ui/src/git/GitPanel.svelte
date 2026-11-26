<script lang="ts">
// The git panel of one project, for people new to git: the branch, what is
// new on the remote or not sent yet, then either "Everything is saved" or the
// changed files and one Commit. Staging (two lists) shows only when asked for
// in the More menu, during a merge or the like, or when part of the changes
// is staged already. Results are announced in a live region; focus stays on
// the list after an action.
import { tick } from "svelte";
import { listCommands } from "../keys/commands";
import type { GitOperation, ProjectId } from "../lib/protocol";
import { openContextMenu } from "../menu/context-menu.svelte";
import { useOvra } from "../shell/context";
import EmptyState from "../shell/EmptyState.svelte";
import Icon from "../shell/Icon.svelte";
import AgentActions from "./AgentActions.svelte";
import AssistCard from "./AssistCard.svelte";
import { Assist } from "./assist.svelte";
import Branches from "./Branches.svelte";
import CommitBox from "./CommitBox.svelte";
import FileSection from "./FileSection.svelte";
import { diffOpened, focusList, focusMessage, focusRow } from "./focus";
import { changedFiles, describePaths, fileLists, GitModel, showsStaging } from "./model.svelte";

let { project }: { project: ProjectId } = $props();
const { transport, workspace } = useOvra();

// svelte-ignore state_referenced_locally
const model = new GitModel(transport, project);
// svelte-ignore state_referenced_locally
const assist = new Assist(transport, model);
$effect(() => () => {
  assist.dispose();
  model.dispose();
});

let terminalNote = $state("");

const status = $derived(model.state.kind === "ready" ? model.state.status : null);
const lists = $derived(fileLists(status?.files ?? []));
/** A repository with no commits and nothing changed: nothing to stage, commit or list yet. */
const fresh = $derived(
  !!status &&
    !status.head &&
    status.files.length === 0 &&
    !status.operation &&
    status.truncated === 0 &&
    !model.discarded,
);

/** Staging shows: the commit takes what is staged, not every change. */
const staging = $derived(!!status && showsStaging(status, model.choosing));
/** Nothing to commit: no change, no merge or the like under way. */
const clean = $derived(!!status && status.files.length === 0 && !status.operation && status.truncated === 0);
const changed = $derived(changedFiles(status?.files ?? []));
const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;

let moreButton = $state<HTMLElement>();

/** The More menu: staging, changing the last commit, a terminal. */
function more(): void {
  const box = moreButton?.getBoundingClientRect();
  const forced = !!status && showsStaging(status, false);
  openContextMenu({ x: box?.left ?? 0, y: box?.bottom ?? 0, returnFocus: moreButton }, [
    !forced && {
      label: model.choosing ? "Commit all changes" : "Choose files to commit",
      run: () => {
        model.choosing = !model.choosing;
      },
    },
    !!status?.head && {
      label: model.amend ? "Keep the last commit as it is" : "Change last commit…",
      run: async () => {
        model.amend = !model.amend;
        await tick();
        if (model.amend) focusMessage();
      },
    },
    "separator",
    { label: "Open terminal", run: openTerminal },
  ]);
}

const OPERATION: Record<GitOperation, string> = {
  merge: "A merge is in progress.",
  rebase: "A rebase is in progress.",
  cherry_pick: "A cherry-pick is in progress.",
  revert: "A revert is in progress.",
  bisect: "A bisect is in progress.",
};

function openTerminal(): void {
  const command = listCommands().find((c) => c.id === "terminal.new");
  terminalNote = command ? "" : "No terminal is available here.";
  void command?.run();
}

/** `git init`, then focus on the panel it opens up: a changed file, else its first button. */
async function setUp(): Promise<void> {
  if (model.busy || !(await model.init())) return;
  await tick();
  focusList("changes");
  const panel = document.querySelector<HTMLElement>("[data-testid=git]");
  if (!panel?.contains(document.activeElement)) panel?.querySelector<HTMLElement>("button")?.focus();
}

function showDiff(path: string, staged: boolean): void {
  diffOpened({ project, path, staged });
  workspace.gitDiff = { project, path, staged };
}

/** The path whose diff of one side is shown, for its row to say so. */
function shownDiff(staged: boolean): string | null {
  const diff = workspace.gitDiff;
  return diff?.project === project && diff.staged === staged ? diff.path : null;
}

async function undo(): Promise<void> {
  const path = model.discarded?.paths[0];
  if (model.busy || path === undefined) return;
  await model.undoDiscard();
  await tick();
  // Back on the file that came back, else on the list or the commit message.
  focusRow(path, false);
}

/** "5 min ago", "3 days ago", or a date for older commits. */
function ago(seconds: number, now = Date.now() / 1000): string {
  const minutes = Math.max(0, Math.round((now - seconds) / 60));
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.round(hours / 24);
  if (days < 30) return `${days} ${days === 1 ? "day" : "days"} ago`;
  return new Date(seconds * 1000).toLocaleDateString("en-US", { year: "numeric", month: "short", day: "numeric" });
}
</script>

<section class="git" aria-labelledby="git-title" data-testid="git">
  <h2 id="git-title" class="visually-hidden">Git</h2>
  <p class="visually-hidden" role="status" data-testid="git-announce">{model.notice}</p>
  <!-- A hook or gpg may wait for input Ovra cannot give: say so, and let the person stop it. -->
  {#if model.slow}
    <div class="first" data-testid="git-running">
      <p role="status">
        {model.stopping ? "Stopping…" : `${model.slow === "commit" ? "Committing" : "Switching branches"}: git or a hook is still running.`}
      </p>
      <button type="button" class="bordered" aria-disabled={model.stopping} onclick={() => void model.cancel()}
        >Cancel</button
      >
    </div>
  {/if}

  {#if model.state.kind === "loading"}
    <p class="hint">Loading git status…</p>
  {:else if model.state.kind === "no_repo"}
    <div class="no-repo" data-testid="git-no-repo">
      <EmptyState headline="Not a git repository" line="Git keeps a history, so you can review and undo changes." />
      <!-- The panel's one primary action, so a newcomer sees a button, not a line of text. -->
      <button type="button" class="primary" onclick={() => void setUp()}>Set up git</button>
    </div>
    {#if model.error?.scope === "setup"}
      <p class="error" role="alert" data-testid="git-setup-error">{model.error.message}</p>
    {/if}
  {:else if model.state.kind === "error"}
    <p class="error" role="alert" data-testid="git-error">{model.state.message}</p>
    <button type="button" onclick={() => model.refresh()}>Try again</button>
  {:else if status && fresh}
    <Branches {model} {status} />
    <EmptyState
      testid="git-fresh"
      headline="No changes"
      line="Git is set up. Add files, then commit them to keep a first version."
    />
  {:else if status}
    <div class="top">
      <Branches {model} {status} />
      <AgentActions {assist} {status} />
      <button
        type="button"
        class="icon"
        aria-label="More"
        title="More"
        aria-haspopup="menu"
        data-testid="git-more"
        bind:this={moreButton}
        onclick={more}><Icon name="more" /></button
      >
    </div>
    {#if status.upstream && (status.behind > 0 || status.ahead > 0 || model.error?.scope === "sync")}
      <div class="sync" data-testid="git-sync">
        {#if status.behind > 0}
          <div class="line">
            <p>
              <strong>{plural(status.behind, "new commit")} on {status.upstream}</strong>
              <span>Someone sent them from elsewhere. Get them to stay up to date.</span>
            </p>
            <button
              type="button"
              class="primary"
              data-testid="git-pull"
              aria-disabled={model.busy}
              onclick={() => void model.pull(status.behind)}
              >{model.syncing === "pull" ? "Getting…" : `Get ${plural(status.behind, "commit")}`}</button
            >
          </div>
        {/if}
        {#if status.ahead > 0}
          <div class="line">
            <p>
              <strong>{plural(status.ahead, "commit")} not sent yet</strong>
              <span
                >{status.behind > 0
                  ? "Get the new commits first, then send yours."
                  : `They are only on this computer. Send them to ${status.upstream}.`}</span
              >
            </p>
            {#if status.behind === 0}
              <button
                type="button"
                class="bordered"
                data-testid="git-push"
                aria-disabled={model.busy}
                onclick={() => void model.push(status.ahead)}
                >{model.syncing === "push" ? "Sending…" : `Send ${plural(status.ahead, "commit")}`}</button
              >
            {/if}
          </div>
        {/if}
        {#if model.error?.scope === "sync"}
          <p class="error" role="alert" data-testid="git-sync-error">{model.error.message}</p>
          <button type="button" class="bordered" onclick={openTerminal}>Open terminal</button>
        {/if}
      </div>
    {/if}
    {#if !status.upstream && status.branch && status.head && status.publish_to}
      {@const branch = status.branch}
      {@const remote = status.publish_to}
      <div class="sync" data-testid="git-publish-card">
        <div class="line">
          <p>
            <strong>{branch} is only on this computer</strong>
            <span>Publish it to {remote} to keep a copy there and share it.</span>
          </p>
          <button
            type="button"
            class="bordered"
            data-testid="git-publish"
            aria-disabled={model.busy}
            onclick={() => void model.publish(branch, remote)}
            >{model.syncing === "publish" ? "Publishing…" : "Publish branch"}</button
          >
        </div>
        {#if model.error?.scope === "sync"}
          <p class="error" role="alert" data-testid="git-sync-error">{model.error.message}</p>
        {/if}
      </div>
    {/if}
    {#if model.got !== null}
      <div class="sync" data-testid="git-got">
        <p><strong>Got {plural(model.got, "commit")}</strong></p>
        <div class="buttons">
          {#if status.head && !assist.busy}
            <button
              type="button"
              class="bordered"
              onclick={() => {
                model.got = null;
                void assist.run("pulled");
              }}>What's new?</button
            >
          {/if}
          <button type="button" onclick={() => (model.got = null)}>Dismiss</button>
        </div>
      </div>
    {/if}
    <AssistCard {assist} />
    {#if status.operation}
      <p class="operation" data-testid="git-operation">
        {OPERATION[status.operation]} Resolve, stage and commit to finish.
      </p>
    {/if}
    {#if model.discarded}
      <div class="undo" role="group" aria-label="Undo discard">
        <p>Discarded the changes to {describePaths(model.discarded.paths)}.</p>
        <button type="button" data-testid="git-undo-discard" aria-disabled={model.busy} onclick={undo}>Undo</button>
      </div>
    {/if}
    {#if model.error?.scope === "files"}
      <p class="error" role="alert" data-testid="git-files-error">{model.error.message}</p>
    {/if}
    {#if clean}
      <EmptyState
        testid="git-clean"
        headline="Everything is saved"
        line="No changes since your last commit. Edit files, or give an agent a task, and they show up here."
      />
      {#if model.committed && !model.amend}
        <p class="ok" role="status" data-testid="git-committed">
          Committed {model.committed.commit.id.slice(0, 7)}: {model.committed.commit.summary}
        </p>
      {/if}
    {/if}
    {#if lists.conflicts.length > 0}
      <FileSection
        {model}
        kind="conflicts"
        title="Conflicts"
        files={lists.conflicts}
        shownDiff={null}
        onopen={(path) => workspace.openFile(path, "edit")}
        ondiff={showDiff}
        onfocuslost={focusMessage}
      />
    {/if}
    {#if staging}
      <FileSection
        {model}
        kind="staged"
        title="Staged: goes in the commit"
        files={lists.staged}
        shownDiff={shownDiff(true)}
        onopen={(path) => workspace.openFile(path)}
        ondiff={showDiff}
        onfocuslost={() => focusList("changes")}
      />
      <FileSection
        {model}
        kind="changes"
        title="Changes"
        files={lists.changes}
        shownDiff={shownDiff(false)}
        onopen={(path) => workspace.openFile(path)}
        ondiff={showDiff}
        onfocuslost={focusMessage}
      />
    {:else if changed.length > 0}
      <FileSection
        {model}
        kind="changes"
        simple
        title={`${plural(changed.length, "changed file")}`}
        files={changed}
        shownDiff={shownDiff(false) ?? shownDiff(true)}
        onopen={(path) => workspace.openFile(path)}
        ondiff={showDiff}
        onfocuslost={focusMessage}
      />
    {/if}
    {#if status.truncated > 0}
      <p class="hint" data-testid="git-truncated">
        {status.truncated.toLocaleString("en-US")} more changed {status.truncated === 1 ? "file is" : "files are"} not shown.
      </p>
    {/if}
    {#if !clean || model.amend}
      <CommitBox {model} {status} {assist} {staging} />
    {/if}
    <div class="log">
      <button type="button" class="toggle" aria-expanded={model.logOpen} onclick={() => model.toggleLog()}>
        <Icon name={model.logOpen ? "chevron" : "chevron-right"} small /> History
      </button>
      {#if model.logOpen}
        {#if model.log === null}
          <p class="hint">Loading commits…</p>
        {:else if model.log.length === 0}
          <p class="hint">No commits yet.</p>
        {:else}
          <ul aria-label="History" data-testid="git-log">
            {#each model.log as commit (commit.id)}
              <!-- No title: it would read the full 40-character id before each summary. -->
              <li>
                <span class="summary" title={commit.summary}>{commit.summary}</span>
                <span class="meta">{commit.id.slice(0, 7)} · {commit.author} · {ago(commit.time)}</span>
              </li>
            {/each}
          </ul>
        {/if}
      {/if}
    </div>
  {/if}
  {#if terminalNote}
    <p class="hint" role="status">{terminalNote}</p>
  {/if}
</section>

<style>
  .top {
    display: flex;
    align-items: flex-start;
    gap: 4px;
  }
  .top > :global(:first-child) {
    flex: 1;
  }
  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--control-h-sm);
    height: var(--control-h-sm);
    padding: 0;
  }
  .sync {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    font-size: 13px;
    background: var(--accent-soft);
    border-radius: var(--radius);
  }
  .sync .line {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .sync p {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .sync .buttons {
    display: flex;
    gap: 4px;
  }
  .sync span {
    color: var(--text-2);
  }
  /* History sits at the bottom of the panel. */
  .log {
    margin-top: auto;
  }
  .git {
    display: flex;
    flex-direction: column;
    gap: 8px;
    height: 100%;
    min-height: 0;
    padding: 12px 10px;
    background: var(--s1);
    box-sizing: border-box;
    overflow-x: hidden;
    overflow-y: auto;
  }
  p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .hint {
    font-size: 13px;
    color: var(--text-2);
  }
  .error {
    font-size: 13px;
    color: var(--danger);
  }
  .ok {
    font-size: 13px;
    color: var(--ok);
  }
  .operation {
    padding: 8px 10px;
    font-size: 13px;
    background: var(--warn-soft);
    border-radius: var(--radius);
  }
  button {
    align-self: flex-start;
    padding: 2px 10px;
    font: inherit;
    font-size: 13px;
    color: var(--text-2);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  button:hover {
    color: var(--text);
    background: var(--s3);
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  button[aria-disabled="true"] {
    opacity: 0.6;
    cursor: default;
  }
  .no-repo {
    display: flex;
    flex-direction: column;
  }
  /* In line with the empty state's text. */
  .no-repo .primary {
    margin-inline: 16px;
  }
  .primary,
  .primary:hover {
    height: var(--control-h-sm);
    padding: 0 12px;
    font-weight: 500;
    color: var(--accent-ink);
    background: var(--accent);
  }
  .bordered {
    height: var(--control-h-sm);
    color: var(--text);
    background: var(--s3);
  }
  .bordered:hover {
    background: var(--s4);
  }
  .first {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 10px;
    font-size: 13px;
    background: var(--accent-soft);
    border-radius: var(--radius);
  }
  .undo {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    align-items: center;
    padding: 6px 10px;
    font-size: 13px;
    background: var(--s3);
    border-radius: var(--radius);
  }
  .undo p {
    flex: 1 1 8em;
    min-width: 0;
  }
  .log .toggle {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 4px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-2);
    background: transparent;
    border-color: transparent;
  }
  .log ul {
    margin: 2px 0 0;
    padding: 0;
    list-style: none;
  }
  .log li {
    display: flex;
    flex-direction: column;
    min-width: 0;
    padding: 3px 4px 3px 10px;
  }
  .summary,
  .meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    font-size: 13px;
    color: var(--text-2);
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
