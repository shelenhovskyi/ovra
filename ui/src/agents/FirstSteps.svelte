<script lang="ts">
// The first three steps, ticked off by doing them: the agent is ready, give it
// a task, then answer a request or review what it changed. Inline in the
// task list, never over anything (least of all an approval); shown until
// all three are done or the person hides it, then never again.
import Icon from "../shell/Icon.svelte";
import { hints } from "./hints.svelte";

/** `agent`: the provider's display name, such as `Claude`. */
let { agent, ready, started }: { agent: string; ready: boolean; started: boolean } = $props();

const steps = $derived([
  { text: `${agent} is ready`, done: ready },
  { text: "Type a task and press Start", done: started || hints.has("started") },
  { text: "Allow a request, then review the changes", done: hints.has("answered") || hints.has("reviewed") },
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
          <span>{step.text}</span>
          {#if step.done}<span class="visually-hidden">, done</span>{/if}
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
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-2);
  }
  li.next {
    color: var(--text);
    font-weight: 500;
  }
  li.done span:not(.mark) {
    text-decoration: line-through;
  }
  .mark {
    flex: none;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    font-size: 12px;
    border-radius: 50%;
    background: var(--s3);
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
