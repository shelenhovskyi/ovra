<script lang="ts">
// The first three steps, ticked off by doing them: the agent is ready, give it
// a task, then answer a request or review what it changed. Inline in the
// task list, never over anything (least of all an approval); shown until
// all three are done or the person hides it, then never again. The step to
// do next says what to do now, with a button when there is something to open.
import Icon from "../shell/Icon.svelte";
import { hints } from "./hints.svelte";

let {
  agent,
  ready,
  started,
  ask,
  review,
}: {
  /** The provider's display name, such as `Claude`. */
  agent: string;
  ready: boolean;
  started: boolean;
  /** Opens the task that asks the person something, if one does. */
  ask?: (() => void) | undefined;
  /** Opens the changes that wait for review, if some do. */
  review?: (() => void) | undefined;
} = $props();

const isStarted = $derived(started || hints.has("started"));
/** The third step's line and button: what to do now, or what to wait for. */
const check = $derived.by((): { detail: string; action?: { label: string; run: () => void } } => {
  if (ask) return { detail: `${agent} asks you something.`, action: { label: "Open", run: ask } };
  if (review) return { detail: "Its changes are ready.", action: { label: "Review", run: review } };
  if (isStarted) return { detail: `${agent} is working. Its questions and changes show up here.` };
  return { detail: "Allow what it asks, then keep or discard its changes." };
});
const steps = $derived([
  { text: `${agent} is ready`, done: ready },
  { text: "Type a task and press Start", done: isStarted },
  { text: `Check ${agent}'s work`, done: hints.has("answered") || hints.has("reviewed"), ...check },
]);
const done = $derived(steps.filter((s) => s.done).length);
const next = $derived(steps.findIndex((s) => !s.done));
</script>

{#if !hints.has("steps_hidden") && done < steps.length}
  <section class="steps" aria-labelledby="first-steps-heading" data-testid="first-steps">
    <div class="head">
      <h3 id="first-steps-heading">Get started <span class="count">{done} of {steps.length}</span></h3>
      <button type="button" class="hide" onclick={() => hints.mark("steps_hidden")}>Hide</button>
    </div>
    <ol>
      {#each steps as step, n (step.text)}
        <li class:done={step.done} class:next={n === next} aria-current={n === next ? "step" : undefined}>
          <span class="mark" aria-hidden="true">{#if step.done}<Icon name="check" small />{:else}{n + 1}{/if}</span>
          <span class="text">
            <span class="title">{step.text}</span>
            {#if step.done}<span class="visually-hidden">, done</span>{/if}
            {#if n === next && "detail" in step}<span class="detail" data-testid="first-steps-detail">{step.detail}</span>{/if}
          </span>
          {#if n === next && "action" in step && step.action}
            <button type="button" class="act" onclick={step.action.run}>{step.action.label}</button>
          {/if}
        </li>
      {/each}
    </ol>
  </section>
{/if}

<style>
  .steps {
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    background: var(--s2);
    border-radius: var(--radius);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }
  .count {
    margin-left: 4px;
    font-weight: 400;
    color: var(--text-2);
  }
  .hide {
    height: 24px;
    padding: 0 8px;
    font: inherit;
    font-size: 13px;
    color: var(--text-2);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
  }
  .hide:hover {
    color: var(--text);
    background: var(--s3);
  }
  ol {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  li {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    font: var(--t-small);
    color: var(--text-2);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  li.next .title {
    color: var(--text);
    font-weight: 500;
  }
  li.done .title {
    text-decoration: line-through;
  }
  .detail {
    color: var(--text-2);
  }
  /* The mark is as tall as the title's line, so both share one middle. */
  .mark {
    flex: none;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    font: 500 11px / 1 var(--font);
    font-variant-numeric: tabular-nums;
    border-radius: 50%;
    background: var(--s3);
  }
  .act {
    flex: none;
    align-self: center;
    height: 24px;
    padding: 0 10px;
    font: var(--t-small);
    font-weight: 500;
    color: var(--accent-ink);
    background: var(--accent);
    border: none;
    border-radius: var(--radius-sm);
  }
  .act:hover {
    filter: brightness(1.08);
  }
  .next .mark {
    color: var(--accent-ink);
    background: var(--accent);
  }
  .done .mark {
    color: var(--ok);
    background: var(--ok-soft);
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
