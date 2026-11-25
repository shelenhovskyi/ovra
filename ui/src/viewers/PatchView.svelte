<script lang="ts">
import Code from "../editor/highlight/Code.svelte";
import { highlightLines, type Segment } from "../editor/highlight/static";
import { languageFor } from "../editor/languages";
import Icon from "../shell/Icon.svelte";
import Lines from "./Lines.svelte";
import { changeStarts, fileName, fileStats, type PatchLine, parsePatch, patchRows } from "./patch";

let { text }: { text: string } = $props();

// The highlighter answers after a tick; by then the parent may have dropped
// the patch (null) while this view is torn down, so read it defensively.
const source = $derived(text ?? "");
const files = $derived(parsePatch(source));
const stats = $derived(files.map(fileStats));
const total = $derived(
  stats.reduce((sum, s) => ({ added: sum.added + s.added, removed: sum.removed + s.removed }), {
    added: 0,
    removed: 0,
  }),
);
/** Syntax colour per line, each file highlighted as one text once the worker answers. */
let painted = $state.raw(new WeakMap<PatchLine, Segment[]>());
$effect(() => {
  const shown = files;
  let live = true;
  const next = new WeakMap<PatchLine, Segment[]>();
  void Promise.all(
    shown.map(async (file) => {
      const lines = file.hunks.flatMap((hunk) => hunk.lines.filter((line) => line.kind !== "note"));
      const segments = await highlightLines(
        languageFor(fileName(file)),
        lines.map((line) => line.text),
      );
      lines.forEach((line, i) => {
        const found = segments?.[i];
        if (found) next.set(line, found);
      });
    }),
  ).then(() => {
    if (live) painted = next;
  });
  return () => {
    live = false;
  };
});
let collapsed = $state(new Set<number>());
const rows = $derived(patchRows(files, collapsed));
const rawLines = $derived(source.split(/\r?\n/));

const changes = $derived(changeStarts(rows));
/** Index into `changes` of the change last moved to, or -1. */
let current = $state(-1);

let list = $state<ReturnType<typeof Lines> | null>(null);

function toggle(file: number) {
  const next = new Set(collapsed);
  if (!next.delete(file)) next.add(file);
  collapsed = next;
  current = -1;
}

function jump(file: number) {
  if (collapsed.has(file)) toggle(file);
  const index = rows.findIndex((row) => row.kind === "file" && row.file === file);
  list?.scrollToIndex(index);
  const after = changes.findIndex((start) => start > index);
  current = (after < 0 ? changes.length : after) - 1;
}

/** Moves to the next (`step` 1) or previous (-1) change, scrolls it into view and announces it. */
function move(step: 1 | -1) {
  const next = current + step;
  if (changes.length === 0) return list?.announce("This patch has no changed lines.");
  if (next < 0 || next >= changes.length) return list?.announce(step > 0 ? "No more changes." : "No earlier changes.");
  current = next;
  const index = changes[next] ?? 0;
  list?.scrollToIndex(Math.max(0, index - 3));
  list?.focus();
  const row = rows[index];
  let header = index;
  while (header > 0 && rows[header]?.kind !== "file") header--;
  const owner = rows[header];
  const patched = owner?.kind === "file" ? files[owner.file] : undefined;
  const where = patched && files.length > 1 ? ` in ${fileName(patched)}` : "";
  if (row?.kind === "added" || row?.kind === "removed")
    list?.announce(
      `Change ${next + 1} of ${changes.length}${where}: ${LABEL[row.kind]} line ${row.kind === "added" ? row.new : row.old}: ${row.text}`,
    );
}

function onkeydown(event: KeyboardEvent) {
  if (event.ctrlKey || event.metaKey || event.altKey) return;
  // Match physical keys, so this also works on non-Latin layouts.
  if (event.code === "KeyN" || event.code === "KeyP") {
    event.preventDefault();
    move(event.code === "KeyN" ? 1 : -1);
  }
}

const LABEL = { context: "unchanged", added: "added", removed: "removed" } as const;
const SIGN = { context: " ", added: "+", removed: "-" } as const;
</script>

{#if files.length === 0}
  <Lines items={rawLines} label="Patch text" testid="patch-view">
    {#snippet row(line)}
      <div>{line}</div>
    {/snippet}
  </Lines>
{:else}
  <div class="patch">
    <div class="summary">
      <span data-testid="patch-summary">
        {files.length}
        {files.length === 1 ? "file" : "files"} changed,
        <span class="plus">+{total.added}</span>
        <span class="minus">−{total.removed}</span>
      </span>
      {#if files.length > 1}
        <label>
          Go to file
          <select onchange={(e) => jump(Number(e.currentTarget.value))}>
            {#each files as file, f (f)}
              <option value={f}>{fileName(file)} (+{stats[f]?.added} −{stats[f]?.removed})</option>
            {/each}
          </select>
        </label>
        <button type="button" onclick={() => (collapsed = new Set(files.map((_, f) => f)))}>Collapse all</button>
        <button type="button" onclick={() => (collapsed = new Set())}>Expand all</button>
      {/if}
      <button type="button" aria-keyshortcuts="P" onclick={() => move(-1)}>Previous change</button>
      <button type="button" aria-keyshortcuts="N" onclick={() => move(1)}>Next change</button>
    </div>
    <Lines bind:this={list} items={rows} label="Changes" testid="patch-view" {onkeydown}>
      {#snippet row(item, index)}
        {#if item.kind === "file"}
          {@const file = files[item.file]}
          <div class="file">
            <button type="button" aria-expanded={!collapsed.has(item.file)} onclick={() => toggle(item.file)}>
              <Icon name={collapsed.has(item.file) ? "chevron-right" : "chevron"} small />
              {#if file?.oldPath === null}
                <span class="badge">new</span> {file.newPath}
              {:else if file?.newPath === null}
                <span class="badge">deleted</span> {file.oldPath}
              {:else if file && file.oldPath !== file.newPath}
                {file.oldPath} → {file.newPath}
              {:else}
                {file?.newPath}
              {/if}
              <span class="plus">+{stats[item.file]?.added}</span>
              <span class="minus">−{stats[item.file]?.removed}</span>
            </button>
          </div>
        {:else if item.kind === "hunk"}
          <div class="hunk">{item.text}</div>
        {:else if item.kind === "note"}
          <div class="note">{item.text}</div>
        {:else}
          <div class="line {item.kind}" class:current={changes[current] === index}>
            <span class="no">{"old" in item ? item.old : ""}</span>
            <span class="no">{"new" in item ? item.new : ""}</span>
            <span class="sign" aria-hidden="true">{SIGN[item.kind]}</span>
            <span class="visually-hidden">{LABEL[item.kind]} line {"new" in item ? item.new : item.old}:</span>
            <span class="code"><Code text={item.text} segments={painted.get(item)} /></span>
          </div>
        {/if}
      {/snippet}
    </Lines>
  </div>
{/if}

<style>
  .patch {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .patch > :global(.lines) {
    flex: 1;
    min-height: 0;
  }
  .summary {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
    padding: 6px 16px;
    font-size: 13px;
  }
  .summary button,
  .summary select {
    min-height: var(--control-h-sm);
    padding: 2px 10px;
    font: inherit;
    color: var(--text-2);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .summary select {
    background: var(--s3);
    color: var(--text);
  }
  .summary button:hover {
    color: var(--text);
    background: var(--s3);
  }
  label {
    color: var(--text-2);
  }
  .summary select {
    max-width: 320px;
  }
  .plus {
    color: var(--ok);
  }
  .minus {
    color: var(--danger);
  }
  .file button {
    all: unset;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    cursor: pointer;
    font-weight: 600;
    padding: 0 6px;
  }
  .file button:focus-visible {
    outline: 2px solid var(--accent);
  }
  .file {
    background: var(--s3);
  }
  .hunk {
    color: var(--text-2);
    background: var(--accent-soft);
    padding: 0 6px;
  }
  .note {
    color: var(--text-2);
    padding: 0 6px;
  }
  .no {
    display: inline-block;
    width: 6ch;
    padding-right: 6px;
    text-align: right;
    color: var(--text-2);
    user-select: none;
  }
  .sign {
    display: inline-block;
    width: 2ch;
    text-align: center;
    user-select: none;
  }
  .line {
    border-left: 3px solid transparent;
  }
  .added {
    background: var(--ok-soft);
    border-left-color: var(--ok);
  }
  .removed {
    background: var(--danger-soft);
    border-left-color: var(--danger);
  }
  .current {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .badge {
    padding: 0 4px;
    border: 1px solid currentColor;
    border-radius: var(--radius-xs);
    font-size: 12px;
    font-weight: 400;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  @media (forced-colors: active) {
    .added {
      border-left: 3px solid CanvasText;
    }
    .removed {
      border-left: 3px dashed CanvasText;
    }
  }
</style>
