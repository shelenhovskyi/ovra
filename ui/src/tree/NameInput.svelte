<script lang="ts">
// The inline name field of a tree row: Enter commits, Escape cancels, leaving
// it commits. Keys stay in the field, so the tree and the window's shortcuts
// do not see them.
import { untrack } from "svelte";

let {
  value: initial,
  range,
  problem,
  label,
  oncheck,
  oncommit,
  oncancel,
}: {
  value: string;
  /** What is selected first, e.g. the name without its extension. */
  range: [number, number];
  problem: string | null;
  label: string;
  oncheck: (name: string) => void;
  oncommit: (name: string, blur: boolean) => void;
  oncancel: () => void;
} = $props();

// svelte-ignore state_referenced_locally
let value = $state(initial);
let input: HTMLInputElement | undefined = $state();
const id = `name-${Math.random().toString(36).slice(2, 8)}`;

// Once, when the field appears: later renders must not move the caret.
$effect(() => {
  if (!input) return;
  input.focus();
  const [start, end] = untrack(() => range);
  input.setSelectionRange(start, end);
});

function onkeydown(e: KeyboardEvent): void {
  e.stopPropagation();
  if (e.code === "Enter" || e.code === "NumpadEnter") {
    e.preventDefault();
    oncommit(value, false);
  } else if (e.code === "Escape") {
    e.preventDefault();
    oncancel();
  }
}
</script>

<span class="field">
  <input
    bind:this={input}
    bind:value
    aria-label={label}
    aria-invalid={problem !== null}
    aria-describedby={problem ? id : undefined}
    spellcheck="false"
    autocomplete="off"
    oninput={() => oncheck(value)}
    {onkeydown}
    onblur={() => oncommit(value, true)}
    onclick={(e) => e.stopPropagation()}
    ondblclick={(e) => e.stopPropagation()}
  />
  {#if problem}<span {id} class="problem" role="alert">{problem}</span>{/if}
</span>

<style>
  .field {
    position: relative;
    flex: 1;
    min-width: 0;
  }
  input {
    box-sizing: border-box;
    width: 100%;
    height: var(--ctl-sm);
    padding: 0 6px;
    border: 1px solid var(--accent);
    border-radius: var(--r-control);
    background: var(--s3);
    color: var(--text);
    font: inherit;
    font-weight: 400;
    outline: none;
    user-select: text;
  }
  input[aria-invalid="true"] {
    border-color: var(--danger);
  }
  .problem {
    position: absolute;
    top: 26px;
    left: 0;
    right: 0;
    z-index: 2;
    padding: 4px 8px;
    border-radius: var(--radius-sm);
    background: var(--s4);
    box-shadow: var(--card-shadow);
    color: var(--danger);
    font-size: var(--fs-ui);
    white-space: normal;
  }
</style>
