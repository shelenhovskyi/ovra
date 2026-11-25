<script lang="ts">
// One server's output. It follows new lines until the person scrolls up, and
// again once they scroll back to the end. Colours are text runs, never HTML.
import { tick, untrack } from "svelte";
import type { DevServerId as ServerId } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { PALETTE, parseAnsi } from "./ansi";
import { MAX_LINES, ServerLog } from "./log.svelte";

let { id, name }: { id: ServerId; name: string } = $props();

const { transport } = useOvra();
// svelte-ignore state_referenced_locally
const log = new ServerLog(transport, id);
let box = $state<HTMLElement>();
let follow = $state(true);

$effect(() => {
  void untrack(() => log.load());
  return () => log.dispose();
});

$effect(() => {
  void log.lines;
  if (!untrack(() => follow)) return;
  void tick().then(() => {
    if (box) box.scrollTop = box.scrollHeight;
  });
});

function onScroll(): void {
  if (!box) return;
  follow = box.scrollTop + box.clientHeight >= box.scrollHeight - 4;
}
</script>

<div class="log-view" data-testid="server-log">
  {#if log.error}
    <p class="error" role="alert">Could not read the output: {log.error}</p>
  {/if}
  {#if log.dropped}
    <p class="muted">Only the last {MAX_LINES.toLocaleString("en")} lines are shown.</p>
  {/if}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex: the log scrolls with the keyboard. -->
  <div class="lines" role="log" aria-label={`Output of ${name}`} tabindex="0" bind:this={box} onscroll={onScroll}>
    {#if log.loading}
      <span class="muted">Loading…</span>
    {:else if log.lines.length === 0}
      <span class="muted">No output yet.</span>
    {/if}
    {#each log.lines as line (line.n)}
      <div class="line">{#each parseAnsi(line.text) as run, i (i)}<span
            class:bold={run.bold}
            style:color={run.fg === undefined ? null : PALETTE[run.fg]}
            style:background={run.bg === undefined ? null : PALETTE[run.bg]}
            >{run.text}</span
          >{/each}</div>
    {/each}
  </div>
  {#if !follow}
    <button type="button" class="follow" onclick={() => box && (box.scrollTop = box.scrollHeight)}>
      Jump to the newest line
    </button>
  {/if}
</div>

<style>
  .log-view {
    position: relative;
  }
  .lines {
    max-height: 240px;
    overflow: auto;
    padding: 4px 8px;
    font-family: ui-monospace, monospace;
    font-size: 12px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: #101013;
    border: 1px solid #2a2a31;
    border-radius: 3px;
  }
  .lines:focus-visible,
  .follow:focus-visible {
    outline: 2px solid #7aa2ff;
  }
  .follow {
    position: absolute;
    right: 12px;
    bottom: 8px;
    font: inherit;
    font-size: 12px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    cursor: pointer;
  }
  .muted {
    color: #b4b4c0;
    font-size: 12px;
  }
  .error {
    color: #ff8f8f;
  }
  .bold {
    font-weight: bold;
  }
</style>
