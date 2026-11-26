<script lang="ts">
// One list of the git panel: "Conflicts", "Staged", "Changes", or without staging the one list of every change.
// A row shows its diff (a conflict opens its file); its hover slot stages,
// unstages or opens it, and its context menu (right click, Shift+F10, the
// Menu key) has every action, Discard last. The list is one Tab stop: arrows,
// Home and End move between rows, and keys named under the list (while a row
// has focus) and in each row's description run its actions; a key that does
// nothing on a row says why. Discard asks first, in the row, starting on Cancel.
import { tick } from "svelte";
import type { GitChange, StatusFile } from "../lib/protocol";
import { isMenuKey, type MenuEntry, openContextMenu } from "../menu/context-menu.svelte";
import FileIcon from "../shell/file-icons/FileIcon.svelte";
import Icon from "../shell/Icon.svelte";
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
  simple = false,
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
  /** The one list of every change, without staging: no Stage, no count, the title says it. */
  simple?: boolean;
} = $props();

let list = $state<HTMLElement>();
let confirming = $state<string | null>(null);
/** The row that is the list's Tab stop. */
let current = $state<string | null>(null);
/** A row or one of its buttons has focus: the keys show under the list. */
let focused = $state(false);
const stop = $derived(files.some((f) => f.path === current) ? current : (files[0]?.path ?? null));

const SIMPLE_KEYS = "Enter: show the diff. Delete: discard. O: open the file.";

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

const change = (file: StatusFile): GitChange =>
  (kind === "staged" ? file.staged : simple ? (file.unstaged ?? file.staged) : file.unstaged) ?? "modified";
/** A file of the simple list with nothing left to stage: its changes are on the staged side. */
const stagedOnly = (file: StatusFile) => kind === "staged" || (simple && file.unstaged === null);
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
  else ondiff(file.path, stagedOnly(file));
}

const stage = (file: StatusFile, i: number) => run(i, () => model.stage([file.path]));
const unstage = (file: StatusFile, i: number) =>
  run(i, () => model.unstage(file.from ? [file.path, file.from] : [file.path]));

/** The row's menu: show and open first, then stage or unstage, Discard last. */
function rowMenu(file: StatusFile, i: number): MenuEntry[] {
  const what = change(file);
  return [
    kind !== "conflicts" && { label: "Show diff", run: () => primary(file) },
    what !== "deleted" && { label: "Open file", keys: "O", run: () => onopen(file.path) },
    "separator",
    kind === "changes" && !simple && { label: "Stage", keys: "S", disabled: model.busy, run: () => stage(file, i) },
    kind === "conflicts" && { label: "Mark resolved", keys: "S", disabled: model.busy, run: () => stage(file, i) },
    kind === "staged" && { label: "Unstage", keys: "U", disabled: model.busy, run: () => unstage(file, i) },
    "separator",
    kind === "changes" &&
      !stagedOnly(file) && {
        label: "Discard changes…",
        keys: "Delete",
        danger: true,
        disabled: model.busy,
        run: () => askDiscard(file.path),
      },
  ];
}

function menu(event: MouseEvent | KeyboardEvent, file: StatusFile, i: number): void {
  const row = rowOf(list, file.path);
  row?.focus();
  openContextMenu(event, rowMenu(file, i));
}

function onrowkeydown(event: KeyboardEvent, file: StatusFile, i: number): void {
  if (isMenuKey(event)) {
    menu(event, file, i);
    return;
  }
  if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return;
  const to = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: files.length - 1 }[event.code];
  const code = event.code === "Backspace" ? "Delete" : event.code;
  const action = {
    KeyS: kind !== "staged" && !simple ? () => stage(file, i) : null,
    KeyU: kind === "staged" ? () => unstage(file, i) : null,
    Delete: kind === "changes" && !stagedOnly(file) ? () => askDiscard(file.path) : null,
    KeyO: change(file) !== "deleted" ? () => onopen(file.path) : null,
  }[code];
  const why =
    code === "KeyO"
      ? "This file is deleted: there is nothing to open."
      : simple
        ? code === "Delete"
          ? "This file's changes are staged: choose files to commit (in the More menu) to unstage them."
          : undefined
        : NO_KEY[kind][code];
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
    <h3 id={`git-${kind}-title`}>{title}{#if !simple}{" "}<span class="count">({files.length})</span>{/if}</h3>
    {#if simple}
      <!-- Nothing to stage here: Commit takes every change. -->
    {:else if kind === "changes"}
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
        ? "Nothing staged."
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
        <div
          class="row"
          class:selected={file.path === shownDiff}
          role="presentation"
          oncontextmenu={(event) => menu(event, file, i)}
        >
          <button
            type="button"
            class="name"
            data-row
            data-path={file.path}
            tabindex={file.path === stop ? 0 : -1}
            title={`${file.from ? `${file.from} → ${file.path}` : file.path} (${CHANGE_LETTER[what]}: ${CHANGE_LABEL[what]})`}
            aria-current={file.path === shownDiff ? "true" : undefined}
            aria-describedby={`git-${kind}-keys`}
            onfocus={() => (current = file.path)}
            onclick={() => primary(file)}
            onkeydown={(event) => onrowkeydown(event, file, i)}
          >
            <FileIcon name={name(file.path)} />
            <span class="file">{name(file.path)}</span>
            <span class="folder">{folder(file.path)}</span>
            <span class="visually-hidden">, {CHANGE_LABEL[what]}{kind === "staged" ? ", staged" : ""}</span>
          </button>
          <span class="mark {what}" aria-hidden="true" title={`${CHANGE_LETTER[what]}: ${CHANGE_LABEL[what]}`}
            >{CHANGE_LETTER[what]}</span
          >
          <!-- The hover slot: always laid out, so showing it moves nothing. Discard is in the menu, never here. -->
          <span class="slot" data-testid="git-row-actions">
            {#if simple}
              <!-- Open only: Commit takes every change. -->
            {:else if kind === "staged"}
              <button
                type="button"
                class="icon"
                tabindex="-1"
                title="Unstage (U)"
                aria-label={`Unstage ${file.path}`}
                aria-disabled={model.busy}
                onclick={() => unstage(file, i)}><Icon name="minus" small /></button
              >
            {:else}
              <button
                type="button"
                class="icon"
                tabindex="-1"
                title={kind === "conflicts" ? "Mark resolved (S)" : "Stage (S)"}
                aria-label={kind === "conflicts" ? `Mark ${file.path} resolved (stage)` : `Stage ${file.path}`}
                aria-disabled={model.busy}
                onclick={() => stage(file, i)}><Icon name={kind === "conflicts" ? "check" : "plus"} small /></button
              >
            {/if}
            {#if kind !== "conflicts" && what !== "deleted"}
              <button
                type="button"
                class="icon"
                tabindex="-1"
                title="Open file (O)"
                aria-label={`Open ${file.path}`}
                onclick={() => onopen(file.path)}><Icon name="external" small /></button
              >
            {/if}
          </span>
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
              Discard changes to {name(file.path)}? {what === "untracked"
                ? "The file is deleted."
                : "It goes back to its staged or committed version."} Undo stays until the next discard.
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
  <p id={`git-${kind}-keys`} class="keys" data-testid={`git-${kind}-keys`} hidden={!focused}>
    {simple ? SIMPLE_KEYS : KEYS[kind]}
  </p>
</section>

<style>
  .files {
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-top: 8px;
  }
  h3 {
    flex: 1;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-2);
  }
  .count {
    font-weight: 400;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  /* A 28 px row (--ctl: the shell hides --row) with a 4 px inset, so the
     selection (the shown diff) reads as a pill. */
  .row {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    min-width: 0;
    height: var(--ctl);
    margin: 0 var(--sp-1);
    padding-right: var(--sp-1);
    border-radius: var(--r-xs);
    color: var(--text-2);
  }
  .row:hover {
    background: var(--s3);
  }
  .row.selected {
    color: var(--text);
    background: var(--sel);
  }
  ul:focus-within .row.selected {
    background: var(--sel-focus);
  }
  button {
    font: inherit;
    color: inherit;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--r-xs);
    cursor: default;
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  /* A focused row rings as a whole, with its dot and actions, not just its name. */
  .row:has(.name:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .row .name:focus-visible {
    outline: none;
  }
  button[aria-disabled="true"] {
    opacity: 0.6;
  }
  .head button {
    flex: none;
    min-height: var(--ctl-sm);
    padding: 0 8px;
    font-size: 13px;
    color: var(--text-2);
    border-radius: var(--r-sm);
  }
  .head button:hover {
    color: var(--text);
    background: var(--s3);
  }
  .name {
    display: flex;
    gap: 6px;
    align-items: center;
    align-self: stretch;
    flex: 1;
    min-width: 0;
    padding: 0 6px;
    text-align: left;
    white-space: nowrap;
  }
  .row.selected .name {
    font-weight: 500;
  }
  /* Git status as a letter in the change's colour, as in the file tree: it
     stays readable without colour and in forced colours. The word is in the
     row's name for screen readers. */
  .mark {
    flex: none;
    width: 1ch;
    font-size: 11px;
    font-weight: 600;
    text-align: center;
    color: var(--warn);
  }
  .mark.added,
  .mark.untracked {
    color: var(--ok);
  }
  .mark.deleted,
  .mark.conflicted {
    color: var(--danger);
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
    font-size: 13px;
    color: var(--text-2);
  }
  /* The hover slot: two 24 px actions and a 4 px gap, reserved even while
     hidden. Shown on hover, while the row has focus and on the selected row. */
  .slot {
    flex: none;
    display: flex;
    gap: var(--sp-1);
    width: calc(2 * var(--ctl-sm) + var(--sp-1));
    opacity: 0;
  }
  .row:hover .slot,
  .row:focus-within .slot,
  .row.selected .slot {
    opacity: 1;
  }
  @media (hover: none), (forced-colors: active) {
    .slot {
      opacity: 1;
    }
  }
  /* Targets of at least 24x24 px (WCAG 2.5.8). */
  .icon {
    flex: none;
    width: var(--ctl-sm);
    height: var(--ctl-sm);
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--text-2);
  }
  .icon:hover {
    color: var(--text);
    background: var(--s4);
  }
  .confirm {
    margin: 2px 0 6px;
    padding: 8px 10px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
  }
  .confirm p {
    margin: 0 0 6px;
    font-size: 13px;
    overflow-wrap: anywhere;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .buttons button {
    padding: 2px 10px;
    color: var(--text);
    background: var(--s3);
  }
  .buttons .danger {
    color: var(--s1);
    background: var(--danger);
  }
  .keys {
    margin: 4px 4px 0;
    font-size: 13px;
    color: var(--text-2);
  }
  .empty {
    margin: 2px 6px;
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
