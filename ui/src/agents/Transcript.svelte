<script lang="ts">
// One session's messages. Focusable so the keyboard can scroll it. Screen
// readers hear only what Claude says (a separate live region), not every
// tool output. What the person wrote, what Claude says and errors stay in
// full; each tool call is a one-line step that opens for its details, and
// reads and searches in a row fold into one group (`steps.ts`). It follows
// new messages only while scrolled to within 60 px of the end.
import { untrack } from "svelte";
import { describeTool } from "../approvals/describe";
import type { AgentMessage, AgentStatus, Billing, MessageFrom } from "../lib/protocol";
import Icon from "../shell/Icon.svelte";
import { isAuthFailure, withoutEchoes } from "./errors";
import LiveActivity from "./LiveActivity.svelte";
import { usageText } from "./sessions.svelte";
import { entries, foldLabel, type Step, stepIcon, stepLabel, stepMeta, turnCost, verbAndObject } from "./steps";
import { ideResult } from "./tool-results";

/**
 * `billing`: who pays for the session, which says what its cost means.
 * `workerName`: a task's worker by its session id, so tool lines name
 * workers as the person knows them instead of by id.
 * `status` and `partial` (the text Claude is writing) show at the end what
 * a working session does now. `onretry`, when given, puts Retry on an error
 * that ended the last turn.
 */
let {
  messages,
  billing = null,
  workerName = () => undefined,
  status = null,
  partial = "",
  onretry,
}: {
  messages: AgentMessage[];
  billing?: Billing | null;
  workerName?: (session: string) => string | undefined;
  status?: AgentStatus | null;
  partial?: string;
  onretry?: () => void;
} = $props();

/** Who wrote what an agent was sent: never "You" for text the person did not write. */
const FROM: Record<MessageFrom, string> = {
  person: "You",
  coordinator: "Coordinator",
  ovra: "Ovra",
  unknown: "Sender not recorded",
};
const MAX_OUTPUT = 2000;
const MAX_SPOKEN = 300;
/** How the claude CLI tells the model that a person denied or interrupted a tool call. */
const REJECTED = "The user doesn't want to proceed with this tool use";
let box = $state<HTMLElement>();

function clip(text: string): string {
  if (text.length <= MAX_OUTPUT) return text;
  return `${text.slice(0, MAX_OUTPUT)}\n… ${text.length - MAX_OUTPUT} more characters`;
}

/** A tool call in words, with a worker's id replaced by its name. */
function describe(tool: string, input: unknown) {
  const fields = typeof input === "object" && input !== null ? (input as Record<string, unknown>) : null;
  const name = typeof fields?.agent_id === "string" ? workerName(fields.agent_id) : undefined;
  return describeTool(tool, name ? { ...fields, agent_id: name } : input);
}

/**
 * Ovra's note to an agent: its first line is for the person, the rest
 * (ids, tool hints, a diff) for the agent, shown folded.
 */
function note(text: string): { line: string; rest: string } {
  const plain = text.startsWith("Ovra: ") ? text.slice("Ovra: ".length) : text;
  const end = plain.indexOf("\n");
  return end < 0 ? { line: plain, rest: "" } : { line: plain.slice(0, end), rest: plain.slice(end + 1).trim() };
}

const capital = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/**
 * Without usage (a fake agent, say) a turn's end line would say nothing;
 * Claude's copy of the error its turn ended with would say it twice.
 */
const shown = $derived(
  entries(
    withoutEchoes(messages).filter((m) => m.kind !== "turn_end" || m.usage.input_tokens + m.usage.output_tokens > 0),
  ),
);

function lines(text: string): string {
  const n = text.split("\n").length;
  return n === 1 ? "1 line" : `${n} lines`;
}

/** The last thing Claude said, for the live region. */
const spoken = $derived.by(() => {
  for (let i = messages.length - 1; i >= 0; i--) {
    const message = messages[i];
    if (message?.kind === "assistant_text") {
      const text = message.text.trim();
      return `Claude: ${text.length > MAX_SPOKEN ? `${text.slice(0, MAX_SPOKEN)}…` : text}`;
    }
  }
  return "";
});

/** Scrolled further than this from the end, new messages no longer pull the view down. */
const FOLLOW_PX = 60;
/** The person scrolled up to read: "Jump to latest" brings the view back. */
let detached = $state(false);

function onscroll(): void {
  if (box) detached = box.scrollHeight - box.scrollTop - box.clientHeight > FOLLOW_PX;
}

function jump(): void {
  if (box) box.scrollTop = box.scrollHeight;
  detached = false;
}

// Follow new messages, and the text that streams in, unless scrolled away.
$effect(() => {
  void [messages.length, partial, status];
  if (box && !untrack(() => detached)) box.scrollTop = box.scrollHeight;
});

/**
 * The error that ended the last turn: it gets Retry. Only the turn's end
 * line (with its usage, from the real driver) may follow it.
 */
const lastError = $derived.by(() => {
  for (let i = shown.length - 1; i >= 0; i--) {
    const entry = shown[i];
    if (entry?.kind === "message" && entry.message.kind === "turn_end") continue;
    return entry?.kind === "message" && entry.message.kind === "error" ? entry : undefined;
  }
  return undefined;
});
</script>

{#snippet card(step: Step)}
  {@const sub = step.use?.parent || step.result?.parent ? "Subagent: " : ""}
  {@const failed = step.result?.is_error ?? false}
  {@const denied = failed && (step.result?.output.startsWith(REJECTED) ?? false)}
  {@const summary = step.use ? describe(step.use.name, step.use.input) : null}
  {@const plain = step.use && step.result && !failed ? ideResult(step.use.name, step.result.output) : null}
  {@const parts = verbAndObject(step, stepLabel(step, summary?.action ?? ""))}
  {@const meta = failed ? "" : stepMeta(step)}
  <details class="step" class:failed={failed && !denied} class:sub={sub !== ""}>
    <summary title={step.use?.name}>
      <Icon name={stepIcon(step)} small />
      <span class="label"
        >{sub}{#if denied}{summary
            ? `Not run: ${summary.action}`
            : "Not run: you denied it or interrupted the turn"}{:else}{parts.verb}{#if parts.object}{" "}<code
              class="object">{parts.object}</code
            >{/if}{/if}</span
      >
      {#if failed && !denied}<span class="flag">failed</span>{/if}
      {#if meta}<span class="meta">{meta}</span>{/if}
    </summary>
    <div class="body">
      {#if summary}
        {#each summary.details as detail}
          <p class="detail" data-testid="tool-detail"><span class="label">{capital(detail.label)}:</span> {detail.text}</p>
        {/each}
      {/if}
      {#if plain}<p>{plain}</p>{/if}
      {#if step.result?.output && !denied}
        <p class="detail">{plain ? "What Claude got" : `Output, ${lines(step.result.output)}`}:</p>
        <pre>{clip(step.result.output)}</pre>
      {/if}
    </div>
  </details>
{/snippet}

<p class="spoken" aria-live="polite">{spoken}</p>
<div class="frame">
<!-- Focusable so arrow keys and Page Up/Down scroll it. A log, but not a
     live one: the line above speaks Claude's text, not every tool output. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<ol
  class="transcript"
  role="log"
  aria-live="off"
  aria-label="Transcript"
  tabindex="0"
  bind:this={box}
  {onscroll}
  data-testid="transcript"
>
  {#each shown as entry, i (i)}
    {#if entry.kind === "steps" && entry.steps.length === 1 && entry.steps[0]}
      <li class="steps">{@render card(entry.steps[0])}</li>
    {:else if entry.kind === "steps"}
      <li class="steps">
        <details class="group">
          <summary>{foldLabel(entry.steps)}</summary>
          <div class="cards">
            {#each entry.steps as step, n (n)}{@render card(step)}{/each}
          </div>
        </details>
      </li>
    {:else}
      {@const message = entry.message}
      {@const sub = "parent" in message && message.parent ? "Subagent " : ""}
      <li class={message.kind} class:sub={sub !== ""}>
        {#if message.kind === "user_text" && message.from === "ovra"}
          {@const { line, rest } = note(message.text)}
          <span class="who">{FROM.ovra}</span>
          <p>{line}</p>
          {#if rest}
            <details>
              <summary class="who">What Claude was told</summary>
              <pre>{clip(rest)}</pre>
            </details>
          {/if}
        {:else if message.kind === "user_text"}
          <span class="who">{FROM[message.from]}</span>
          <p>{message.text}</p>
        {:else if message.kind === "assistant_text"}
          <span class="who">{sub}Claude</span>
          <p>{message.text}</p>
        {:else if message.kind === "turn_end"}
          <!-- A thin divider, not a row: the full numbers on hover. -->
          <span class="divider" title={usageText(message.usage, billing)}>Turn ended · {turnCost(message.usage, billing)}</span>
        {:else if message.kind === "error"}
          <!-- A sign-in problem is explained once, above the transcript. -->
          <span class="who">Error</span>
          <p>{isAuthFailure(message.message) ? "Account signed out" : message.message}</p>
          {#if onretry && lastError === entry && !isAuthFailure(message.message)}
            <button type="button" class="retry" onclick={onretry}>Retry</button>
          {/if}
        {/if}
      </li>
    {/if}
  {/each}
  {#if status}<LiveActivity {status} {messages} {partial} />{/if}
</ol>
{#if detached}
  <button type="button" class="jump" data-testid="jump-to-latest" onclick={jump}>Jump to latest</button>
{/if}
</div>

<style>
  .frame {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .jump {
    position: absolute;
    left: 50%;
    bottom: 12px;
    transform: translateX(-50%);
    height: 28px;
    padding: 0 12px;
    font: inherit;
    font-size: 13px;
    color: var(--text);
    background: var(--s4);
    border: none;
    border-radius: 14px;
    box-shadow: var(--card-shadow);
  }
  .divider {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-2);
  }
  .divider::before,
  .divider::after {
    content: "";
    flex: 1;
    height: 1px;
    background: var(--s3);
  }
  .object {
    font-family: var(--mono);
    font-size: 12px;
  }
  .meta {
    flex: none;
    margin-left: auto;
    font-variant-numeric: tabular-nums;
  }
  .retry {
    margin-top: 8px;
    height: 28px;
    padding: 0 12px;
    font: inherit;
    font-size: 13px;
    color: var(--text);
    background: var(--s3);
    border: none;
    border-radius: var(--radius-sm);
  }
  .transcript {
    list-style: none;
    margin: 0;
    padding: 4px 16px 12px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow: auto;
    flex: 1;
    /* The panel keeps the reply box in sight: the transcript gives way. */
    min-height: 0;
  }
  .transcript:focus-visible {
    outline-offset: -2px;
  }
  .spoken {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  summary {
    cursor: pointer;
  }
  .who {
    display: block;
    font-size: 13px;
    color: var(--text-2);
  }
  p,
  pre {
    margin: 2px 0 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  /* What the person wrote stands apart by tone. */
  .user_text {
    padding: 8px 12px;
    background: var(--s3);
    border-radius: var(--radius);
  }
  /* Steps are quiet one-liners: what Claude says is the text to read. */
  .steps,
  .turn_end {
    font-size: 13px;
    color: var(--text-2);
  }
  .step > summary,
  .group > summary {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 24px;
    min-width: 0;
    list-style: none;
    border-radius: var(--radius-sm);
  }
  .step > summary::-webkit-details-marker,
  .group > summary::-webkit-details-marker {
    display: none;
  }
  .step > summary:hover,
  .group > summary:hover {
    color: var(--text);
  }
  .step > summary .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .group > summary::before {
    content: "▸";
  }
  .group[open] > summary::before {
    content: "▾";
  }
  .cards {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 4px 0 0 12px;
  }
  .flag {
    flex: none;
    color: var(--danger);
  }
  .body {
    margin: 4px 0 4px 23px;
  }
  .action {
    color: var(--text);
    font-family: var(--mono);
  }
  .detail .label {
    color: var(--text-2);
  }
  .detail {
    color: var(--text);
  }
  .sub {
    padding-left: 12px;
    box-shadow: inset 2px 0 var(--s4);
  }
  .error {
    padding: 8px 12px;
    background: var(--s2);
    border-radius: var(--radius);
  }
  .error .who {
    color: var(--danger);
    font-weight: 600;
  }
  pre {
    max-height: 200px;
    overflow: auto;
    font-size: 13px;
    background: var(--s2);
    padding: 6px 8px;
    border-radius: var(--radius-sm);
  }
</style>
