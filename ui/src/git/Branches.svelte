<script lang="ts">
// The current branch, and a menu to switch to another local branch or
// create one. Errors (an agent writes the checkout, changes would be
// overwritten, a bad name) stay in the menu, next to what was pressed.
// Up, Down, Home and End move between branches; typing on one goes to the
// filter field above them, where Enter switches to the first match.
import { tick, untrack } from "svelte";
import type { RepoStatus } from "../lib/protocol";
import Icon from "../shell/Icon.svelte";
import { branchNameProblem, type GitModel } from "./model.svelte";

let { model, status }: { model: GitModel; status: RepoStatus } = $props();

let open = $state(false);
let creating = $state(false);
let name = $state("");
let filter = $state("");
let toggleButton = $state<HTMLElement>();
let menu = $state<HTMLElement>();

const label = $derived(
  status.branch ??
    (status.head ? `No branch, at commit ${status.head.slice(0, 7)} (detached HEAD)` : "No branch (detached HEAD)"),
);

const shownBranches = $derived(
  (model.branches ?? []).filter((b) => b.name.toLowerCase().includes(filter.trim().toLowerCase())),
);

async function show(): Promise<void> {
  open = !open;
  creating = false;
  filter = "";
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
  menu?.querySelector<HTMLInputElement>("form input")?.focus();
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
  // Escape in a filled filter clears it first.
  if (event.target instanceof HTMLInputElement && event.target.dataset.filter !== undefined && filter) filter = "";
  else void close();
}

const branchButtons = () => [...(menu?.querySelectorAll<HTMLElement>("[data-branch]") ?? [])];

/** Arrows, Home and End on a branch; a typed letter goes on in the filter. */
function onbranchkeydown(event: KeyboardEvent, i: number): void {
  if (event.ctrlKey || event.metaKey || event.altKey) return;
  const last = branchButtons().length - 1;
  const to = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: last }[event.code];
  if (to !== undefined) {
    event.preventDefault();
    if (to < 0) menu?.querySelector<HTMLElement>("[data-filter]")?.focus();
    else branchButtons()[Math.min(to, last)]?.focus();
  } else if (event.key.length === 1 && event.key !== " ") {
    event.preventDefault();
    filter += event.key;
    menu?.querySelector<HTMLElement>("[data-filter]")?.focus();
  }
}

function onfilterkeydown(event: KeyboardEvent): void {
  const first = shownBranches[0];
  if (event.code === "ArrowDown") {
    event.preventDefault();
    branchButtons()[0]?.focus();
  } else if (event.code === "Enter" && first) {
    event.preventDefault();
    void switchTo(first.name, first.current);
  }
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
    <Icon name="chevron" small />
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
        <label class="filter">
          Filter branches
          <input
            type="text"
            data-filter
            spellcheck="false"
            autocomplete="off"
            bind:value={filter}
            onkeydown={onfilterkeydown}
          />
        </label>
        <p class="hint" role="status" data-testid="git-branch-matches">
          {#if filter.trim()}
            {shownBranches.length === 0
              ? "No branch matches."
              : `${shownBranches.length} of ${model.branches.length} branches. Enter switches to ${shownBranches[0]?.name}.`}
          {/if}
        </p>
        <ul>
          {#each shownBranches as branch, i (branch.name)}
            <li>
              <button
                type="button"
                data-branch
                onkeydown={(event) => onbranchkeydown(event, i)}
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
    color: var(--text);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  button:hover {
    background: var(--s3);
  }
  button:focus-visible,
  input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  button[aria-disabled="true"] {
    opacity: 0.6;
    cursor: default;
  }
  .current,
  .menu li button {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    min-width: 0;
    padding: 4px 6px;
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
    margin: 0 6px;
    font-size: 13px;
    color: var(--text-2);
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
    padding: 6px;
    background: var(--s2);
    border-radius: var(--radius);
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
    color: var(--text-2);
  }
  .filter {
    margin-bottom: 2px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 13px;
    color: var(--text-2);
  }
  input {
    box-sizing: border-box;
    width: 100%;
    padding: 4px 8px;
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 4px;
  }
  .buttons button,
  .new {
    padding: 2px 10px;
    color: var(--text-2);
    background: var(--s3);
  }
  .new {
    margin-top: 4px;
  }
  .hint,
  .error {
    margin: 4px 0 0;
    font-size: 13px;
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .error {
    color: var(--danger);
  }
  .hint:empty {
    margin: 0;
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
