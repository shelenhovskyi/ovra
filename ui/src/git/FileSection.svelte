<script lang="ts">
// One list of the git panel: "Conflicts", "Staged changes" or "Changes".
// A row shows its diff (a conflict opens its file); small buttons open the
// file, stage, unstage or discard it. The list is one Tab stop: arrows,
// Home and End move between rows, and keys named under the list (while a row
// has focus) and in each row's description run its actions; a key that does
// nothing on a row says why. Discard asks first, in the row, starting on Cancel.
import { tick } from "svelte";
import type { GitChange, StatusFile } from "../lib/protocol";
import { rowOf } from "./focus";
import { CHANGE_LABEL, CHANGE_LETTER, type GitModel } from "./model.svelte";

type Kind = "conflicts" | "staged" | "changes";

let {
  model,
  kind,
  title,
  files,
  shownDiff,
  onopen,
  ondiff,
  onfocuslost,
}: {
  model: GitModel;
  kind: Kind;
  title: string;
  files: StatusFile[];
  /** The path whose diff of this list's side is shown, if any. */
  shownDiff: string | null;
  onopen: (path: string) => void;
  ondiff: (path: string, staged: boolean) => void;
  /** Focus had nowhere to go in this list after an action. */
  onfocuslost: () => void;
} = $props();

let list = $state<HTMLElement>();
let confirming = $state<string | null>(null);
/** The row that is the list's Tab stop. */
let current = $state<string | null>(null);
/** A row or one of its buttons has focus: the keys show under the list. */
let focused = $state(false);
const stop = $derived(files.some((f) => f.path === current) ? current : (files[0]?.path ?? null));

const KEYS: Record<Kind, string> = {
  conflicts: "Enter or O: open the file. S: mark resolved.",
  staged: "Enter: show the diff. U: unstage. O: open the file.",
  changes: "Enter: show the diff. S: stage. Delete: discard. O: open the file.",
};

/** Why a row key does nothing in this list, said instead of silence. */
const NO_KEY: Record<Kind, Partial<Record<string, string>>> = {
  conflicts: {
    KeyU: "This file is not staged: there is nothing to unstage.",
    Delete: "Resolve the conflict in the editor (Enter), then mark it resolved (S).",
  },
  staged: {
    KeyS: "This file is staged already.",
    Delete: "Unstage the file first (U), then discard it under Changes.",
  },
  changes: { KeyU: "This file is not staged: there is nothing to unstage." },
};

const change = (file: StatusFile): GitChange => (kind === "staged" ? file.staged : file.unstaged) ?? "modified";
const name = (path: string) => path.slice(path.lastIndexOf("/") + 1);
const folder = (path: string) => path.slice(0, Math.max(path.lastIndexOf("/"), 0));
const rows = () => [...(list?.querySelectorAll<HTMLElement>("[data-row]") ?? [])];

/** Runs an action of the row at `index`, then focuses the row now there. */
async function run(index: number, call: () => Promise<boolean>): Promise<void> {
  if (model.busy || files.length === 0) return;
  await call();
  await tick();
  const left = rows();
  const target = left[Math.min(index, left.length - 1)];
  if (target) target.focus();
  else onfocuslost();
}

async function askDiscard(path: string): Promise<void> {
  if (model.busy) return;
  confirming = path;
  await tick();
  list?.querySelector<HTMLElement>("[data-cancel-discard]")?.focus();
}

async function cancelDiscard(): Promise<void> {
  const path = confirming;
  confirming = null;
  await tick();
  if (path !== null) rowOf(list, path)?.focus();
}

/** The row's main action: its diff, or for a conflict its file in the editor. */
function primary(file: StatusFile): void {
  if (kind === "conflicts") onopen(file.path);
  else ondiff(file.path, kind === "staged");
}

const stage = (file: StatusFile, i: number) => run(i, () => model.stage([file.path]));
const unstage = (file: StatusFile, i: number) =>
  run(i, () => model.unstage(file.from ? [file.path, file.from] : [file.path]));

function onrowkeydown(event: KeyboardEvent, file: StatusFile, i: number): void {
  if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return;
  const to = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: files.length - 1 }[event.code];
  const code = event.code === "Backspace" ? "Delete" : event.code;
  const action = {
    KeyS: kind !== "staged" ? () => stage(file, i) : null,
    KeyU: kind === "staged" ? () => unstage(file, i) : null,
    Delete: kind === "changes" ? () => askDiscard(file.path) : null,
    KeyO: change(file) !== "deleted" ? () => onopen(file.path) : null,
  }[code];
  const why = code === "KeyO" ? "This file is deleted: there is nothing to open." : NO_KEY[kind][code];
  if (to !== undefined) {
    event.preventDefault();
    rows()[Math.max(0, Math.min(to, files.length - 1))]?.focus();
  } else if (action) {
    event.preventDefault();
    if (!event.repeat) void action();
  } else if (action === null && why) {
    event.preventDefault();
    model.notice = why;
  }
}

const all = $derived(files.flatMap((f) => (kind === "staged" && f.from ? [f.path, f.from] : [f.path])));
</script>

<section class="files" aria-labelledby={`git-${kind}-title`} data-testid={`git-${kind}`}>
  <div class="head">
    <h3 id={`git-${kind}-title`}>{title} <span class="count">({files.length})</span></h3>
    {#if kind === "changes"}
      <button type="button" aria-disabled={model.busy || files.length === 0} onclick={() => run(0, () => model.stage(all))}
        >Stage all</button
      >
    {:else if kind === "staged"}
      <button type="button" aria-disabled={model.busy || files.length === 0} onclick={() => run(0, () => model.unstage(all))}
        >Unstage all</button
      >
    {/if}
  </div>
  {#if files.length === 0}
    <p class="empty">{kind === "staged"
        ? "Nothing is staged. Staged changes (git calls this the index) go into the next commit."
        : "No changes."}</p>
  {/if}
  <ul
    aria-labelledby={`git-${kind}-title`}
    bind:this={list}
    onfocusin={() => (focused = true)}
    onfocusout={(event) => (focused = !!list?.contains(event.relatedTarget as Node | null))}
  >
    {#each files as file, i (file.path)}
      {@const what = change(file)}
      <li>
        <div class="row">
          <button
            type="button"
            class="name"
            data-row
            data-path={file.path}
            tabindex={file.path === stop ? 0 : -1}
            title={file.from ? `${file.from} → ${file.path}` : file.path}
            aria-current={file.path === shownDiff ? "true" : undefined}
            aria-describedby={`git-${kind}-keys`}
            onfocus={() => (current = file.path)}
            onclick={() => primary(file)}
            onkeydown={(event) => onrowkeydown(event, file, i)}
          >
            <span class="letter {what}" aria-hidden="true">{CHANGE_LETTER[what]}</span>
            <span class="file">{name(file.path)}</span>
            <span class="folder">{folder(file.path)}</span>
            <span class="visually-hidden">, {CHANGE_LABEL[what]}{kind === "staged" ? ", staged" : ""}</span>
          </button>
          {#if kind !== "conflicts" && what !== "deleted"}
            <button
              type="button"
              class="icon"
              tabindex="-1"
              title="Open file (O)"
              aria-label={`Open ${file.path}`}
              onclick={() => onopen(file.path)}>↗</button
            >
          {/if}
          {#if kind === "changes"}
            <button
              type="button"
              class="icon"
              tabindex="-1"
              title="Discard changes… (Delete)"
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
              tabindex="-1"
              title="Unstage (U)"
              aria-label={`Unstage ${file.path}`}
              aria-disabled={model.busy}
              onclick={() => unstage(file, i)}>−</button
            >
          {:else}
            <button
              type="button"
              class="icon"
              tabindex="-1"
              title={kind === "conflicts" ? "Mark resolved (S)" : "Stage (S)"}
              aria-label={kind === "conflicts" ? `Mark ${file.path} resolved (stage)` : `Stage ${file.path}`}
              aria-disabled={model.busy}
              onclick={() => stage(file, i)}>+</button
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
                  void run(i, () => model.discard([file.path]));
                }}>Discard</button
              >
              <button type="button" data-cancel-discard onclick={cancelDiscard}>Cancel</button>
            </div>
          </div>
        {/if}
      </li>
    {/each}
  </ul>
  <p id={`git-${kind}-keys`} class="keys" data-testid={`git-${kind}-keys`} hidden={!focused}>{KEYS[kind]}</p>
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
  /* Targets of at least 24x24 px (WCAG 2.5.8). */
  .name,
  .icon {
    min-height: 24px;
  }
  button {
    font: inherit;
    color: #c8c8d0;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 3px;
    cursor: pointer;
  }
  button:hover,
  .name[aria-current="true"] {
    background: #24242c;
  }
  .name[aria-current="true"] {
    border-left-color: #4a6bb0;
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
    padding: 3px 2px;
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
    width: 24px;
    height: 24px;
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
  .keys {
    margin: 2px 2px 0;
    font-size: 11px;
    color: #9a9aa6;
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
