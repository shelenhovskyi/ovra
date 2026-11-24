<script lang="ts">
import Lines from "./Lines.svelte";
import { fileName, fileStats, parsePatch, patchRows } from "./patch";

let { text }: { text: string } = $props();

const files = $derived(parsePatch(text));
const stats = $derived(files.map(fileStats));
const total = $derived(
  stats.reduce((sum, s) => ({ added: sum.added + s.added, removed: sum.removed + s.removed }), {
    added: 0,
    removed: 0,
  }),
);
let collapsed = $state(new Set<number>());
const rows = $derived(patchRows(files, collapsed));
const rawLines = $derived(text.split(/\r?\n/));

let list = $state<ReturnType<typeof Lines> | null>(null);

function toggle(file: number) {
  const next = new Set(collapsed);
  if (!next.delete(file)) next.add(file);
  collapsed = next;
}

function jump(file: number) {
  if (collapsed.has(file)) toggle(file);
  const index = rows.findIndex((row) => row.kind === "file" && row.file === file);
  list?.scrollToIndex(index);
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
    </div>
    <Lines bind:this={list} items={rows} label="Changes" testid="patch-view">
      {#snippet row(item)}
        {#if item.kind === "file"}
          {@const file = files[item.file]}
          <div class="file">
            <button type="button" aria-expanded={!collapsed.has(item.file)} onclick={() => toggle(item.file)}>
              <span aria-hidden="true">{collapsed.has(item.file) ? "▸" : "▾"}</span>
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
          <div class="line {item.kind}">
            <span class="no">{"old" in item ? item.old : ""}</span>
            <span class="no">{"new" in item ? item.new : ""}</span>
            <span class="sign" aria-hidden="true">{SIGN[item.kind]}</span>
            <span class="visually-hidden">{LABEL[item.kind]} line {"new" in item ? item.new : item.old}:</span>
            <span class="code">{item.text}</span>
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
    gap: 12px;
    padding: 6px 12px;
    border-bottom: 1px solid #2a2a31;
  }
  .summary select {
    max-width: 320px;
  }
  .plus {
    color: #7fd18b;
  }
  .minus {
    color: #ff8a8a;
  }
  .file button {
    all: unset;
    cursor: pointer;
    font-weight: 600;
    padding: 0 6px;
  }
  .file button:focus-visible {
    outline: 2px solid #7aa2f7;
  }
  .file {
    background: #1d1d23;
  }
  .hunk {
    color: #a0a0e6;
    background: #1e1e2a;
    padding: 0 6px;
  }
  .note {
    color: #8a8a96;
    padding: 0 6px;
  }
  .no {
    display: inline-block;
    width: 6ch;
    padding-right: 6px;
    text-align: right;
    color: #8a8a96;
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
    background: #16301e;
    border-left-color: #3f9a55;
  }
  .removed {
    background: #3a1a1e;
    border-left-color: #c24a55;
  }
  .badge {
    padding: 0 4px;
    border: 1px solid currentColor;
    border-radius: 3px;
    font-size: 11px;
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
