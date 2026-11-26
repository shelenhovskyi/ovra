<script lang="ts">
// The git panel of one project: branch, commit box, conflicts, staged and
// unstaged files, the Undo of the last discard and recent commits. Results
// are announced in a live region; focus stays on the list after an action.
import { tick } from "svelte";
import { listCommands } from "../keys/commands";
import type { GitOperation, ProjectId } from "../lib/protocol";
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
import { describePaths, fileLists, GitModel } from "./model.svelte";

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

/** A repository with no commits whose files are not staged yet: the next step is a first commit. */
const firstCommit = $derived(
  !!status && !status.head && !status.operation && lists.changes.length > 0 && lists.staged.length === 0,
);

/** Stages every change, then hands over to the message: Commit stays the one primary. */
async function stageAll(): Promise<void> {
  if (model.busy || !(await model.stage(lists.changes.map((f) => f.path)))) return;
  await tick();
  focusMessage();
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
    </div>
    <AssistCard {assist} />
    {#if firstCommit}
      <div class="first" data-testid="git-first-commit">
        <p>Git is set up. Commit your files to keep a first version.</p>
        <button type="button" class="bordered" aria-disabled={model.busy} onclick={stageAll}>Stage all</button>
      </div>
    {/if}
    {#if status.operation}
      <p class="operation" data-testid="git-operation">
        {OPERATION[status.operation]} Resolve, stage and commit to finish.
      </p>
    {/if}
    <CommitBox {model} {status} {assist} />
    {#if model.discarded}
      <div class="undo" role="group" aria-label="Undo discard">
        <p>Discarded the changes to {describePaths(model.discarded.paths)}.</p>
        <button type="button" data-testid="git-undo-discard" aria-disabled={model.busy} onclick={undo}>Undo</button>
      </div>
    {/if}
    {#if model.error?.scope === "files"}
      <p class="error" role="alert" data-testid="git-files-error">{model.error.message}</p>
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
    <FileSection
      {model}
      kind="staged"
      title="Staged changes"
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
    {#if status.truncated > 0}
      <p class="hint" data-testid="git-truncated">
        {status.truncated.toLocaleString("en-US")} more changed {status.truncated === 1 ? "file is" : "files are"} not shown.
      </p>
    {/if}
    <!-- Push and pull stay git's own: a terminal is one press away. -->
    <div class="remote">
      <button type="button" data-testid="git-remote-hint" title="git push, git pull" onclick={openTerminal}
        ><Icon name="terminal" small />Push or pull in a terminal</button
      >
    </div>
    <div class="log">
      <button type="button" class="toggle" aria-expanded={model.logOpen} onclick={() => model.toggleLog()}>
        <Icon name={model.logOpen ? "chevron" : "chevron-right"} small /> Recent commits
      </button>
      {#if model.logOpen}
        {#if model.log === null}
          <p class="hint">Loading commits…</p>
        {:else if model.log.length === 0}
          <p class="hint">No commits yet.</p>
        {:else}
          <ul aria-label="Recent commits" data-testid="git-log">
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
  .remote {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    align-items: center;
    margin-top: 6px;
  }
  .remote button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
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
