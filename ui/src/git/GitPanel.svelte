<script lang="ts">
// The git panel of one project: branch, commit box, conflicts, staged and
// unstaged files, the Undo of the last discard and recent commits. Results
// are announced in a live region; focus stays on the list after an action.
import { tick } from "svelte";
import { listCommands } from "../keys/commands";
import type { GitOperation, ProjectId } from "../lib/protocol";
import { useOvra } from "../shell/context";
import Icon from "../shell/Icon.svelte";
import Branches from "./Branches.svelte";
import CommitBox from "./CommitBox.svelte";
import FileSection from "./FileSection.svelte";
import { diffOpened, focusList, focusMessage, focusRow } from "./focus";
import { describePaths, fileLists, GitModel } from "./model.svelte";

let { project }: { project: ProjectId } = $props();
const { transport, workspace } = useOvra();

// svelte-ignore state_referenced_locally
const model = new GitModel(transport, project);
$effect(() => () => model.dispose());

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

  {#if model.state.kind === "loading"}
    <p class="hint">Loading git status…</p>
  {:else if model.state.kind === "no_repo"}
    <p data-testid="git-no-repo">This folder is not a git repository.</p>
    <p class="hint">Run <code>git init</code> in a terminal to start one.</p>
    <button type="button" onclick={openTerminal}>Open terminal</button>
    <button type="button" onclick={() => model.refresh()}>Check again</button>
  {:else if model.state.kind === "error"}
    <p class="error" role="alert" data-testid="git-error">{model.state.message}</p>
    <button type="button" onclick={() => model.refresh()}>Try again</button>
  {:else if status && fresh}
    <Branches {model} {status} />
    <p class="hint" data-testid="git-fresh">No changes yet.</p>
  {:else if status}
    <Branches {model} {status} />
    {#if status.operation}
      <p class="operation" data-testid="git-operation">
        {OPERATION[status.operation]} Resolve, stage and commit to finish.
      </p>
    {/if}
    <CommitBox {model} {status} />
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
    <div class="remote">
      <p class="hint" data-testid="git-remote-hint">
        Push and pull: <code>git push</code>, <code>git pull</code> in a terminal.
      </p>
      <button type="button" onclick={openTerminal}>Open terminal</button>
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
  code {
    font-family: var(--mono);
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
