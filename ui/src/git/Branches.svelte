<script lang="ts">
// The current branch, and a menu to switch to another local branch or
// create one. Errors (an agent writes the checkout, changes would be
// overwritten, a bad name) stay in the menu, next to what was pressed.
import { tick, untrack } from "svelte";
import type { RepoStatus } from "../lib/protocol";
import { branchNameProblem, type GitModel } from "./model.svelte";

let { model, status }: { model: GitModel; status: RepoStatus } = $props();

let open = $state(false);
let creating = $state(false);
let name = $state("");
let toggleButton = $state<HTMLElement>();
let menu = $state<HTMLElement>();

const label = $derived(
  status.branch ?? (status.head ? `Detached HEAD at ${status.head.slice(0, 7)}` : "Detached HEAD"),
);

async function show(): Promise<void> {
  open = !open;
  creating = false;
  if (!open) return;
  if (model.error?.scope === "branch") model.error = null;
  await model.loadBranches();
  await tick();
  (menu?.querySelector<HTMLElement>("[aria-current='true']") ?? menu?.querySelector<HTMLElement>("button"))?.focus();
}

async function close(): Promise<void> {
  open = false;
  creating = false;
  await tick();
  toggleButton?.focus();
}

async function switchTo(branch: string, current: boolean): Promise<void> {
  if (current) return close();
  if (!model.busy && (await model.switchTo(branch))) await close();
}

async function startCreate(): Promise<void> {
  creating = true;
  name = "";
  await tick();
  menu?.querySelector<HTMLInputElement>("input")?.focus();
}

async function cancelCreate(): Promise<void> {
  creating = false;
  await tick();
  menu?.querySelector<HTMLElement>(".new")?.focus();
}

const problem = $derived(name.trim() ? branchNameProblem(name.trim()) : null);

async function create(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  const wanted = name.trim();
  if (!wanted || model.busy) return;
  // The reason is under the field already; pressing Create says it once more.
  if (problem) model.notice = `Could not create ${wanted}. ${problem}`;
  else if (await model.createBranch(wanted)) await close();
}

// "Git: switch branch…" and "Git: new branch…" from the palette.
$effect(() => {
  const wanted = model.branchMenu;
  if (!wanted) return;
  untrack(async () => {
    model.branchMenu = null;
    if (!open) await show();
    else (menu?.querySelector<HTMLElement>("[aria-current='true']") ?? menu?.querySelector("button"))?.focus();
    if (wanted === "create") await startCreate();
  });
});

function onkeydown(event: KeyboardEvent): void {
  if (event.code !== "Escape") return;
  event.preventDefault();
  event.stopPropagation();
  void close();
}
</script>

<div class="branch">
  <button
    type="button"
    class="current"
    data-testid="git-branch"
    title={label}
    aria-expanded={open}
    aria-controls="git-branch-menu"
    bind:this={toggleButton}
    onclick={show}
  >
    <span class="visually-hidden">Branch:</span>
    <span class="label">{label}</span>
    <span aria-hidden="true">▾</span>
  </button>
  {#if status.upstream}
    <p class="upstream" title={status.upstream}>
      <span class="name">{status.upstream}</span>
      <span class="counts"
        >{status.ahead === 0 && status.behind === 0
          ? "up to date"
          : [status.ahead ? `${status.ahead} ahead` : "", status.behind ? `${status.behind} behind` : ""]
              .filter(Boolean)
              .join(", ")}</span
      >
    </p>
  {/if}
  {#if !status.head}
    <p class="upstream">No commits yet.</p>
  {/if}
  {#if open}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="menu" id="git-branch-menu" role="group" aria-label="Branches" bind:this={menu} {onkeydown}>
      {#if model.branches === null}
        <p class="hint">Loading branches…</p>
      {:else}
        <ul>
          {#each model.branches as branch (branch.name)}
            <li>
              <button
                type="button"
                title={branch.commit ? `${branch.name}: ${branch.commit.summary}` : branch.name}
                aria-current={branch.current}
                aria-disabled={model.busy}
                onclick={() => switchTo(branch.name, branch.current)}
              >
                <span class="label">{branch.name}</span>
                {#if branch.current}<span class="tag">(current)</span>{/if}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      {#if creating}
        <form onsubmit={create}>
          <label>
            New branch name
            <input
              type="text"
              spellcheck="false"
              autocomplete="off"
              aria-invalid={!!problem}
              aria-describedby={problem ? "git-branch-name-problem" : undefined}
              bind:value={name}
            />
          </label>
          {#if problem}
            <p class="hint" id="git-branch-name-problem" data-testid="git-branch-name-problem">{problem}</p>
          {/if}
          <div class="buttons">
            <button type="submit" aria-disabled={!name.trim() || !!problem || model.busy}>Create and switch</button>
            <button type="button" onclick={cancelCreate}>Cancel</button>
          </div>
        </form>
      {:else}
        <button type="button" class="new" onclick={startCreate}>New branch…</button>
      {/if}
      {#if model.error?.scope === "branch"}
        <p class="error" role="alert" data-testid="git-branch-error">{model.error.message}</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .branch {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  button {
    font: inherit;
    color: #e6e6e6;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 3px;
    cursor: pointer;
  }
  button:hover {
    background: #24242c;
  }
  button:focus-visible,
  input:focus-visible {
    outline: 2px solid #4a6bb0;
    outline-offset: -2px;
  }
  button[aria-disabled="true"] {
    opacity: 0.6;
    cursor: default;
  }
  .current,
  .menu li button {
    display: flex;
    gap: 4px;
    width: 100%;
    min-width: 0;
    padding: 2px 4px;
    text-align: left;
  }
  .current {
    font-weight: 600;
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .upstream {
    display: flex;
    gap: 6px;
    margin: 0 4px;
    font-size: 12px;
    color: #9a9aa6;
    min-width: 0;
  }
  .upstream .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .counts {
    flex: none;
  }
  .menu {
    padding: 4px;
    background: #1e1e24;
    border: 1px solid #3a3a44;
    border-radius: 3px;
  }
  ul {
    max-height: 14em;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
  }
  .tag {
    flex: none;
    color: #9a9aa6;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 12px;
    color: #9a9aa6;
  }
  input {
    box-sizing: border-box;
    width: 100%;
    padding: 2px 4px;
    font: inherit;
    color: #e6e6e6;
    background: #16161a;
    border: 1px solid #3a3a44;
    border-radius: 3px;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 4px;
  }
  .buttons button,
  .new {
    padding: 1px 8px;
    border-color: #3a3a44;
    background: #24242c;
  }
  .new {
    margin-top: 4px;
  }
  .hint,
  .error {
    margin: 4px 0 0;
    font-size: 12px;
    color: #9a9aa6;
    overflow-wrap: anywhere;
  }
  .error {
    color: #ff6b6b;
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
