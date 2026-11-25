<script lang="ts">
// One list of the git panel: "Conflicts", "Staged changes" or "Changes".
// A row opens its file; small buttons show the diff, stage, unstage or
// discard it. Discard asks first, in the row, starting on Cancel.
import { tick } from "svelte";
import type { GitChange, StatusFile } from "../lib/protocol";
import { CHANGE_LABEL, CHANGE_LETTER, type GitModel } from "./model.svelte";

type Kind = "conflicts" | "staged" | "changes";

let {
  model,
  kind,
  title,
  files,
  onopen,
  ondiff,
  onfocuslost,
}: {
  model: GitModel;
  kind: Kind;
  title: string;
  files: StatusFile[];
  onopen: (path: string) => void;
  ondiff: (path: string, staged: boolean) => void;
  /** Focus had nowhere to go in this list after an action. */
  onfocuslost: () => void;
} = $props();

let section = $state<HTMLElement>();
let list = $state<HTMLElement>();
let confirming = $state<string | null>(null);

const change = (file: StatusFile): GitChange => (kind === "staged" ? file.staged : file.unstaged) ?? "modified";
const name = (path: string) => path.slice(path.lastIndexOf("/") + 1);
const folder = (path: string) => path.slice(0, Math.max(path.lastIndexOf("/"), 0));

/** Runs an action of the row at `index`, then focuses the same action of the row now there. */
async function run(action: string, index: number, call: () => Promise<boolean>): Promise<void> {
  if (model.busy || files.length === 0) return;
  await call();
  await tick();
  const buttons = [...(list?.querySelectorAll<HTMLElement>(`[data-action="${action}"]`) ?? [])];
  const target =
    buttons[Math.min(index, buttons.length - 1)] ??
    section?.querySelector<HTMLElement>("button:not([aria-disabled='true'])");
  if (target) target.focus();
  else onfocuslost();
}

async function askDiscard(path: string): Promise<void> {
  confirming = path;
  await tick();
  list?.querySelector<HTMLElement>("[data-cancel-discard]")?.focus();
}

async function cancelDiscard(): Promise<void> {
  const path = confirming;
  confirming = null;
  await tick();
  list?.querySelector<HTMLElement>(`[data-action="discard"][data-path="${CSS.escape(path ?? "")}"]`)?.focus();
}

function open(file: StatusFile): void {
  // A deleted file has nothing to open; its diff shows what was in it.
  if (change(file) === "deleted") ondiff(file.path, kind === "staged");
  else onopen(file.path);
}

const all = $derived(files.flatMap((f) => (kind === "staged" && f.from ? [f.path, f.from] : [f.path])));
</script>

<section class="files" aria-labelledby={`git-${kind}-title`} data-testid={`git-${kind}`} bind:this={section}>
  <div class="head">
    <h3 id={`git-${kind}-title`}>{title} <span class="count">({files.length})</span></h3>
    {#if kind === "changes"}
      <button type="button" aria-disabled={model.busy || files.length === 0} onclick={() => run("stage-all", 0, () => model.stage(all))}
        >Stage all</button
      >
    {:else if kind === "staged"}
      <button
        type="button"
        aria-disabled={model.busy || files.length === 0}
        onclick={() => run("unstage-all", 0, () => model.unstage(all))}
        >Unstage all</button
      >
    {/if}
  </div>
  {#if files.length === 0}
    <p class="empty">{kind === "staged" ? "Nothing is staged." : "No changes."}</p>
  {/if}
  <ul bind:this={list}>
    {#each files as file, i (file.path)}
      {@const what = change(file)}
      <li>
        <div class="row">
          <button
            type="button"
            class="name"
            title={file.from ? `${file.from} → ${file.path}` : file.path}
            onclick={() => open(file)}
          >
            <span class="letter {what}" aria-hidden="true">{CHANGE_LETTER[what]}</span>
            <span class="file">{name(file.path)}</span>
            <span class="folder">{folder(file.path)}</span>
            <span class="visually-hidden">, {CHANGE_LABEL[what]}</span>
          </button>
          {#if kind !== "conflicts"}
            <button
              type="button"
              class="icon"
              data-action="diff"
              title="Show diff"
              aria-label={`Show diff of ${file.path}`}
              onclick={() => ondiff(file.path, kind === "staged")}>±</button
            >
          {/if}
          {#if kind === "changes"}
            <button
              type="button"
              class="icon"
              data-action="discard"
              data-path={file.path}
              title="Discard changes…"
              aria-label={`Discard changes to ${file.path}…`}
              aria-expanded={confirming === file.path}
              aria-disabled={model.busy}
              onclick={() => askDiscard(file.path)}>↺</button
            >
          {/if}
          {#if kind === "staged"}
            <button
              type="button"
              class="icon"
              data-action="unstage"
              title="Unstage"
              aria-label={`Unstage ${file.path}`}
              aria-disabled={model.busy}
              onclick={() => run("unstage", i, () => model.unstage(file.from ? [file.path, file.from] : [file.path]))}>−</button
            >
          {:else}
            <button
              type="button"
              class="icon"
              data-action="stage"
              title={kind === "conflicts" ? "Mark resolved (stage)" : "Stage"}
              aria-label={kind === "conflicts" ? `Mark ${file.path} resolved (stage)` : `Stage ${file.path}`}
              aria-disabled={model.busy}
              onclick={() => run("stage", i, () => model.stage([file.path]))}>+</button
            >
          {/if}
        </div>
        {#if confirming === file.path}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <div
            class="confirm"
            role="group"
            aria-labelledby={`git-discard-${i}`}
            onkeydown={(e) => {
              if (e.code === "Escape") {
                e.preventDefault();
                void cancelDiscard();
              }
            }}
          >
            <p id={`git-discard-${i}`}>
              Discard your changes to {name(file.path)}? {what === "untracked"
                ? "The file is deleted."
                : "It goes back to its staged or committed version."} You can undo this until the next discard.
            </p>
            <div class="buttons">
              <button
                type="button"
                class="danger"
                data-testid="git-confirm-discard"
                aria-disabled={model.busy}
                onclick={() => {
                  confirming = null;
                  void run("discard", i, () => model.discard([file.path]));
                }}>Discard</button
              >
              <button type="button" data-cancel-discard onclick={cancelDiscard}>Cancel</button>
            </div>
          </div>
        {/if}
      </li>
    {/each}
  </ul>
</section>

<style>
  .files {
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-top: 6px;
  }
  h3 {
    flex: 1;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    color: #9a9aa6;
  }
  .count {
    font-weight: normal;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .row {
    display: flex;
    align-items: center;
    min-width: 0;
  }
  button {
    font: inherit;
    color: #c8c8d0;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 3px;
    cursor: pointer;
  }
  button:hover {
    background: #24242c;
  }
  button:focus-visible {
    outline: 2px solid #4a6bb0;
    outline-offset: -2px;
  }
  button[aria-disabled="true"] {
    opacity: 0.6;
    cursor: default;
  }
  .head button {
    flex: none;
    padding: 0 4px;
    font-size: 11px;
    border-color: #3a3a44;
  }
  .name {
    display: flex;
    gap: 4px;
    align-items: baseline;
    flex: 1;
    min-width: 0;
    padding: 1px 2px;
    text-align: left;
    white-space: nowrap;
  }
  .letter {
    flex: none;
    width: 1.2ch;
    font-family: ui-monospace, monospace;
    font-size: 11px;
    font-weight: 600;
    color: #d8b25a;
  }
  .letter.added,
  .letter.untracked {
    color: #8fd19e;
  }
  .letter.deleted,
  .letter.conflicted {
    color: #ff8a8a;
  }
  .file {
    flex: none;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .folder {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 11px;
    color: #8a8a96;
  }
  .icon {
    flex: none;
    width: 22px;
    padding: 0;
    font-family: ui-monospace, monospace;
  }
  .confirm {
    margin: 2px 0 4px 12px;
    padding: 4px 6px;
    border-left: 2px solid #b05a4a;
  }
  .confirm p {
    margin: 0 0 4px;
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .buttons button {
    padding: 1px 8px;
    border-color: #3a3a44;
    background: #24242c;
  }
  .buttons .danger {
    border-color: #b05a4a;
  }
  .empty {
    margin: 2px 0;
    font-size: 12px;
    color: #8a8a96;
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
