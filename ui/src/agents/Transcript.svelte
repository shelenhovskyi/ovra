<script lang="ts">
// One session's messages. Focusable so the keyboard can scroll it. Screen
// readers hear only what Claude says (a separate live region), not every
// tool output. What the person wrote, what Claude says and errors stay in
// full; each tool call is a one-line step that opens for its details, and
// reads and searches in a row fold into one group (`steps.ts`). It follows
// new messages only while scrolled to within 60 px of the end.
import { untrack } from "svelte";
import { describeTool } from "../approvals/describe";
import type { AgentMessage, AgentStatus, Billing, MessageFrom, Stuck, ToolCall } from "../lib/protocol";
import Icon from "../shell/Icon.svelte";
import AgentText from "./AgentText.svelte";
import { turnsOf, turnsText } from "./checkpoints";
import { isAuthFailure, isInterrupt, withoutEchoes } from "./errors";
import LiveActivity from "./LiveActivity.svelte";
import { plainChat } from "./markdown";
import { usageText } from "./sessions.svelte";
import {
  entries,
  foldLabel,
  inFolder,
  keepSame,
  noteLine,
  notesLabel,
  type Step,
  stepIcon,
  stepLabel,
  stepMeta,
  turnCost,
  verbAndObject,
} from "./steps";
import { ideResult } from "./tool-results";

/**
 * `agent`: the provider's display name, such as `Claude`, for the speaker label.
 * `billing`: who pays for the session, which says what its cost means.
 * `workerName`: a task's worker by its session id, so tool lines name
 * workers as the person knows them instead of by id.
 * `status` and `partial` (the text Claude is writing) show at the end what
 * a working session does now. `onretry`, when given, puts Retry on an error
 * that ended the last turn. `root`: the session's folder; files in it show
 * relative to it. `denied` tells a step the person denied from one that
 * failed, whatever the agent was told. `ongoback`, when given, puts "Go
 * back to here" on the person's message of each turn in `checkpoints`
 * (`docs/specs/checkpoints.md`); `goBackBlocked` says why it cannot now.
 * `undone`: the turns a go-back undid, shown greyed under a divider.
 * `onlink`: a link the person clicked in what the agent said.
 */
let {
  messages,
  agent = "Agent",
  billing = null,
  root = null,
  workerName = () => undefined,
  status = null,
  stuck = null,
  partial = "",
  onretry,
  denied: answeredNo = () => false,
  checkpoints = new Set<number>(),
  goBackBlocked = null,
  undone = null,
  ongoback,
  onlink,
}: {
  messages: AgentMessage[];
  agent?: string;
  billing?: Billing | null;
  root?: string | null;
  workerName?: (session: string) => string | undefined;
  status?: AgentStatus | null;
  /** Why the working turn may be stuck (`SessionInfo.stuck`). */
  stuck?: Stuck | null;
  partial?: string;
  onretry?: () => void;
  /** Whether the person's own answer denied this call (known while this window watched it). */
  denied?: (use: { name: string; input: unknown }) => boolean;
  checkpoints?: ReadonlySet<number>;
  goBackBlocked?: string | null;
  undone?: { from: number; to: number } | null;
  ongoback?: (turn: number, text: string) => void;
  onlink?: (href: string) => void;
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

/** A tool call in words, with a worker's id replaced by its name and files relative to the session's folder. */
function describe(tool: string, input: unknown, call: ToolCall | undefined) {
  const fields = typeof input === "object" && input !== null ? (input as Record<string, unknown>) : null;
  const name = typeof fields?.agent_id === "string" ? workerName(fields.agent_id) : undefined;
  return describeTool(tool, name ? { ...fields, agent_id: name } : input, inFolder(call, root));
}

/** The file a step works on, in full, for its tooltip. */
function fileOf(step: Step): string | null {
  return step.use?.call?.paths?.[0] ?? null;
}

const capital = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/** The entries last shown: their rows need no update when a message comes. */
let previous: ReturnType<typeof entries> = [];
/**
 * Without usage (a fake agent, say) a turn's end line would say nothing;
 * Claude's copy of the error its turn ended with would say it twice.
 * Entries that did not change stay the same objects (`keepSame`).
 */
const shown = $derived.by(() => {
  const next = entries(
    withoutEchoes(messages).filter((m) => m.kind !== "turn_end" || m.usage.input_tokens + m.usage.output_tokens > 0),
  );
  previous = keepSame(previous, next);
  return previous;
});

const turnOf = $derived(turnsOf(messages));
const latest = $derived(Math.max(0, ...turnOf.values()));
/** Each entry's turn, from 1; 0 before the first message. */
const turnAt = $derived(
  shown.map((entry) => {
    const key =
      entry.kind === "message"
        ? entry.message
        : entry.kind === "notes"
          ? entry.first
          : (entry.steps[0]?.use ?? entry.steps[0]?.result);
    return key ? (turnOf.get(key) ?? 0) : 0;
  }),
);
/** The person's first message in each turn: "Go back to here" sits on it. */
const opens = $derived.by(() => {
  const seen = new Set<number>();
  const out = new Set<AgentMessage>();
  for (const message of messages) {
    const turn = turnOf.get(message) ?? 0;
    if (message.kind !== "user_text" || message.from !== "person" || seen.has(turn)) continue;
    seen.add(turn);
    out.add(message);
  }
  return out;
});
/** The turn whose go-back the person is asked to confirm. */
let asking = $state<number | null>(null);
const isUndone = (turn: number) => undone !== null && turn >= undone.from && turn <= undone.to;

/** The question opens under its message: it scrolls into sight, its buttons too. */
function inSight(node: HTMLElement): void {
  node.scrollIntoView?.({ block: "nearest" });
}

function closeOnEscape(event: KeyboardEvent): void {
  if (event.code !== "Escape") return;
  event.stopPropagation();
  asking = null;
}

function lines(text: string): string {
  const n = text.split("\n").length;
  return n === 1 ? "1 line" : `${n} lines`;
}

/** The last thing the agent said, for the live region. */
const spoken = $derived.by(() => {
  for (let i = messages.length - 1; i >= 0; i--) {
    const message = messages[i];
    if (message?.kind === "assistant_text") {
      const text = plainChat(message.text);
      return `${agent}: ${text.length > MAX_SPOKEN ? `${text.slice(0, MAX_SPOKEN)}…` : text}`;
    }
  }
  return "";
});

/** Scrolled further than this from the end, new messages no longer pull the view down. */
const FOLLOW_PX = 60;
/** The person scrolled up to read: "Jump to latest" brings the view back. */
let detached = $state(false);
/** Scrolled down from the top: the top line fades out under what sits above, never cut through its letters. */
let faded = $state(false);
/** How many entries there were when the person scrolled away: the rest are new to them. */
let seen = $state(0);
const fresh = $derived(detached ? Math.max(0, shown.length - seen) : 0);

function onscroll(): void {
  if (!box) return;
  faded = box.scrollTop > 0;
  const away = box.scrollHeight - box.scrollTop - box.clientHeight > FOLLOW_PX;
  if (away && !detached) seen = shown.length;
  detached = away;
}

function jump(): void {
  if (!box) return;
  box.scrollTop = box.scrollHeight;
  // The button goes away: focus moves to the log it scrolled, not to the page.
  box.focus({ preventScroll: true });
  detached = false;
}

// Follow new messages, and the text that streams in, unless scrolled away.
// The person's own message always brings the view down: they want to see the answer.
$effect(() => {
  void [messages.length, partial, status];
  const last = messages[messages.length - 1];
  const sent = last?.kind === "user_text" && last.from === "person";
  if (!box || (!sent && untrack(() => detached))) return;
  box.scrollTop = box.scrollHeight;
  detached = false;
});

// A card opening under the log makes it shorter without a scroll: keep the end in view.
$effect(() => {
  if (!box || typeof ResizeObserver === "undefined") return;
  const log = box;
  const observer = new ResizeObserver(() => {
    if (!untrack(() => detached)) log.scrollTop = log.scrollHeight;
  });
  observer.observe(log);
  return () => observer.disconnect();
});

/**
 * The error that ended the last turn: it gets Retry. Only the turn's end
 * line (with its usage, from the real driver) may follow it.
 */
const lastError = $derived.by(() => {
  for (let i = shown.length - 1; i >= 0; i--) {
    const entry = shown[i];
    if (entry?.kind === "message" && entry.message.kind === "turn_end") continue;
    return entry?.kind === "message" && entry.message.kind === "error" && !isInterrupt(entry.message)
      ? entry
      : undefined;
  }
  return undefined;
});
</script>

{#snippet card(step: Step)}
  {@const sub = step.use?.parent || step.result?.parent ? "Subagent: " : ""}
  {@const failed = step.result?.is_error ?? false}
  {@const said = failed && step.use !== null && answeredNo(step.use)}
  {@const denied = said || (failed && (step.result?.output.startsWith(REJECTED) ?? false))}
  {@const summary = step.use ? describe(step.use.name, step.use.input, step.use.call) : null}
  {@const plain = step.use && step.result && !failed ? ideResult(step.use.name, step.result.output) : null}
  {@const parts = verbAndObject(stepLabel(step, summary?.action ?? ""), summary?.object ?? "")}
  {@const meta = failed ? "" : stepMeta(step)}
  {@const file = fileOf(step)}
  <details class="step" class:failed={failed && !denied} class:sub={sub !== ""}>
    <summary title={file ?? step.use?.name}>
      <Icon name={stepIcon(step)} small />
      {#if denied && !summary}
        <span class="label">{sub}Not run: you denied it or interrupted the turn</span>
      {:else}
        <span class="label" class:verb={parts.object !== ""}>{sub}{parts.verb}</span>
        <!-- A file is cut from the left, so its name stays in sight. -->
        {#if parts.object}<code class="object" class:path={file !== null}><bdi>{parts.object}</bdi></code>{/if}
      {/if}
      <!-- The person's own No is no failure; the CLI's words alone may also mean an interrupt. -->
      {#if denied && summary}<span class="flag quiet">{said ? "Denied" : "Not run"}</span>{/if}
      {#if failed && !denied}<span class="flag">failed</span>{/if}
      {#if meta}<span class="meta">{meta}</span>{/if}
    </summary>
    <div class="body">
      {#if summary}
        {#each summary.details as detail}
          <p class="detail" data-testid="tool-detail"><span class="label">{capital(detail.label)}:</span> {detail.text}</p>
        {/each}
      {/if}
      {#if plain && step.result}
        <!-- Ovra's tool answered the agent in JSON with ids: the sentence, the raw answer folded. -->
        <p>{plain}</p>
        <details class="raw">
          <summary class="detail">What {agent} got</summary>
          <pre>{clip(step.result.output)}</pre>
        </details>
      {:else if step.result?.output && !denied}
        <p class="detail">Output, {lines(step.result.output)}:</p>
        <pre>{clip(step.result.output)}</pre>
      {/if}
    </div>
  </details>
{/snippet}

<p class="spoken" aria-live="polite">{spoken}</p>
<div class="frame">
<!-- Focusable so arrow keys and Page Up/Down scroll it. A log, but not a
     live one: the line above speaks Claude's text, not every tool output.
     The log holds a list rather than being one, so the entries keep their
     list semantics (a role on the <ol> itself would drop them). -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="transcript"
  class:faded
  role="log"
  aria-live="off"
  aria-label="Transcript"
  tabindex="0"
  bind:this={box}
  {onscroll}
  data-testid="transcript"
>
<ol class="entries">
  {#each shown as entry, i (i)}
    {@const turn = turnAt[i] ?? 0}
    {#if undone && turn === undone.from && turnAt[i - 1] !== turn}
      <li class="set-back" data-testid="set-back-divider">
        <span class="divider">Files set back to before this message</span>
      </li>
    {/if}
    {#if entry.kind === "steps" && entry.steps.length === 1 && entry.steps[0]}
      <li class="steps" class:undone={isUndone(turn)}>{@render card(entry.steps[0])}</li>
    {:else if entry.kind === "steps"}
      <li class="steps" class:undone={isUndone(turn)}>
        <details class="group">
          <summary>{foldLabel(entry.steps)}</summary>
          <div class="cards">
            {#each entry.steps as step, n (n)}{@render card(step)}{/each}
          </div>
        </details>
      </li>
    {:else if entry.kind === "notes"}
      <!-- What Ovra told the agent: one quiet line; what the agent read, folded. -->
      <li class="notes" class:undone={isUndone(turn)}>
        {#if entry.notes.length === 1 && !entry.notes[0]?.rest}
          <span class="line">{notesLabel(entry.notes)}</span>
        {:else}
          <details class="group">
            <summary>{notesLabel(entry.notes)}</summary>
            <div class="cards">
              {#each entry.notes as { line, rest }, n (n)}
                {#if entry.notes.length > 1}<p>{noteLine(line)}</p>{/if}
                {#if rest}
                  <p class="detail">Message sent to {agent}:</p>
                  <pre>{clip(rest)}</pre>
                {/if}
              {/each}
            </div>
          </details>
        {/if}
      </li>
    {:else}
      {@const message = entry.message}
      {@const sub = "parent" in message && message.parent ? "Subagent " : ""}
      <li
        class={isInterrupt(message) ? "interrupted" : message.kind}
        class:sub={sub !== ""}
        class:undone={isUndone(turn)}
      >
        {#if message.kind === "user_text"}
          <span class="who">{FROM[message.from]}</span>
          <p>{message.text}</p>
          {#if ongoback && checkpoints.has(turn) && opens.has(message)}
            <!-- Quiet until hovered or focused; while a turn runs it says why it waits. -->
            <button
              type="button"
              class="back"
              class:shown={asking === turn}
              data-testid="go-back"
              aria-disabled={goBackBlocked !== null}
              aria-expanded={asking === turn}
              title={goBackBlocked ?? "Set the files back to before this message"}
              onclick={() => (asking = asking === turn ? null : turn)}>Go back to here</button
            >
            {#if asking === turn && goBackBlocked}
              <p class="why">{goBackBlocked}</p>
            {:else if asking === turn}
              <!-- Escape from either button closes it. -->
              <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
              <div
                class="confirm"
                role="group"
                aria-label="Go back to here?"
                data-testid="go-back-confirm"
                onkeydown={closeOnEscape}
                {@attach inSight}
              >
                <p>Undo the agent's changes from {turnsText(turn, Math.max(turn, latest))}?</p>
                <p class="why">Your own changes are kept. Ignored files, such as node_modules, are not restored.</p>
                <span class="buttons">
                  <button type="button" class="quiet" onclick={() => (asking = null)}>Cancel</button>
                  <button
                    type="button"
                    class="quiet strong"
                    onclick={() => {
                      asking = null;
                      ongoback(turn, message.text);
                    }}>Go back</button
                  >
                </span>
              </div>
            {/if}
          {/if}
        {:else if message.kind === "assistant_text"}
          <span class="who">{sub}{agent}</span>
          <AgentText text={message.text} {onlink} />
        {:else if message.kind === "turn_end"}
          <!-- A thin divider, not a row: the full numbers on hover. -->
          <span class="divider" title={usageText(message.usage, billing)}>Turn ended · {turnCost(message.usage, billing)}</span>
        {:else if isInterrupt(message)}
          <p>You interrupted this turn.</p>
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
  {#if status}<LiveActivity {agent} {status} {messages} {partial} {root} {stuck} {onlink} />{/if}
</ol>
</div>
{#if detached}
  <!-- A small round button in its own row under the log: it never covers a line. -->
  {@const label = `${fresh > 0 ? `${fresh} new · ` : ""}Jump to latest`}
  <button type="button" class="jump" class:fresh={fresh > 0} data-testid="jump-to-latest" aria-label={label} title={label} onclick={jump}
    >{#if fresh > 0}<span aria-hidden="true">{fresh} new</span>{/if}<Icon name="chevron" small /></button
  >
{/if}
</div>

<style>
  /* Never squeezed to nothing by the cards under it: they scroll instead. */
  .frame {
    position: relative;
    flex: 1;
    min-height: 96px;
    display: flex;
    flex-direction: column;
  }
  .jump {
    position: relative;
    flex: none;
    align-self: flex-end;
    margin: 2px 12px 4px;
    width: 28px;
    height: 28px;
    padding: 0;
    display: grid;
    place-items: center;
    color: var(--text-2);
    background: var(--s3);
    border: none;
    border-radius: 50%;
  }
  .jump:hover {
    color: var(--text);
    background: var(--s4);
  }
  /* New messages below: the count in words, on a pill that grows from the circle. */
  .jump.fresh {
    width: auto;
    padding: 0 8px 0 10px;
    display: flex;
    gap: 4px;
    align-items: center;
    font: var(--t-small);
    color: var(--accent);
    border-radius: var(--r-pill);
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
  .back {
    margin-top: 4px;
    height: 24px;
    padding: 0 8px;
    font: inherit;
    font-size: 13px;
    color: var(--text-2);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    opacity: 0;
  }
  .user_text:hover .back,
  .back:focus-visible,
  .back.shown {
    opacity: 1;
  }
  .back:hover:not([aria-disabled="true"]),
  .quiet:hover {
    color: var(--text);
    background: var(--s4);
  }
  .back[aria-disabled="true"] {
    cursor: default;
  }
  .why {
    font-size: 13px;
    color: var(--text-2);
  }
  .confirm {
    margin-top: 6px;
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
    background: var(--s2);
    border-radius: var(--radius);
  }
  .confirm .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 4px;
  }
  .quiet {
    height: 28px;
    padding: 0 8px;
    font: inherit;
    font-size: 13px;
    color: var(--text-2);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
  }
  .strong {
    color: var(--text);
    font-weight: 500;
  }
  /* Turns a go-back undid: still there to read, set apart by tone. */
  .undone {
    opacity: 0.6;
  }
  @media (prefers-reduced-motion: no-preference) {
    .back {
      transition: opacity 0.1s;
    }
  }
  .object {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono);
    font-size: 12px;
  }
  /* Right to left, so the ellipsis eats the folders, not the file's name. */
  .object.path {
    direction: rtl;
    text-align: left;
  }
  .step > summary .label.verb {
    flex: none;
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
  /* The list lays out as the log's own children. */
  .entries {
    display: contents;
    list-style: none;
  }
  .transcript.faded {
    mask-image: linear-gradient(to bottom, transparent, currentColor 20px);
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
  .notes,
  .interrupted,
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
  .flag.quiet {
    color: var(--text-2);
  }
  .raw > summary {
    width: fit-content;
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
