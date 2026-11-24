<script lang="ts">
// One session's messages. Focusable so the keyboard can scroll it. Screen
// readers hear only what Claude says (a separate live region), not every
// tool output; outputs are folded, since they are mostly written for the
// model.
import { describeTool } from "../approvals/describe";
import type { AgentMessage, MessageFrom } from "../lib/protocol";
import { usageText } from "./sessions.svelte";

let { messages }: { messages: AgentMessage[] } = $props();

/** Who wrote what an agent was sent: never "You" for text the person did not write. */
const FROM: Record<MessageFrom, string> = { person: "You", coordinator: "Coordinator", ovra: "Ovra" };
const MAX_OUTPUT = 2000;
const MAX_SPOKEN = 300;
/** How the claude CLI tells the model that a person denied or interrupted a tool call. */
const REJECTED = "The user doesn't want to proceed with this tool use";
let box = $state<HTMLElement>();

function clip(text: string): string {
  if (text.length <= MAX_OUTPUT) return text;
  return `${text.slice(0, MAX_OUTPUT)}\n… ${text.length - MAX_OUTPUT} more characters`;
}

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

// Follow new messages while the view is scrolled to the end.
let atEnd = true;
$effect.pre(() => {
  void messages.length;
  if (box) atEnd = box.scrollHeight - box.scrollTop - box.clientHeight < 24;
});
$effect(() => {
  void messages.length;
  if (box && atEnd) box.scrollTop = box.scrollHeight;
});
</script>

<p class="spoken" aria-live="polite">{spoken}</p>
<!-- Focusable so arrow keys and Page Up/Down scroll it. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<ol class="transcript" aria-label="Transcript" tabindex="0" bind:this={box} data-testid="transcript">
  {#each messages as message, i (i)}
    {@const sub = "parent" in message && message.parent ? "Subagent " : ""}
    <li class={message.kind} class:sub={sub !== ""}>
      {#if message.kind === "user_text"}
        <span class="who">{FROM[message.from]}</span>
        <p>{message.text}</p>
      {:else if message.kind === "assistant_text"}
        <span class="who">{sub}Claude</span>
        <p>{message.text}</p>
      {:else if message.kind === "tool_use"}
        <span class="who">{sub}Tool {message.name}</span>
        <p>{describeTool(message.name, message.input).action}</p>
      {:else if message.kind === "tool_result"}
        {#if message.is_error && message.output.startsWith(REJECTED)}
          <span class="who">{sub}Not run: you denied it or interrupted the turn</span>
        {:else if message.output}
          <details>
            <summary class="who">{sub}{message.is_error ? "Tool failed" : "Tool output"} ({lines(message.output)})</summary>
            <pre>{clip(message.output)}</pre>
          </details>
        {:else}
          <span class="who">{sub}{message.is_error ? "Tool failed" : "Tool finished"}</span>
        {/if}
      {:else if message.kind === "turn_end"}
        <span class="who">Turn ended: {usageText(message.usage)}</span>
      {:else if message.kind === "error"}
        <span class="who">Error</span>
        <p>{message.message}</p>
      {/if}
    </li>
  {/each}
</ol>

<style>
  .transcript {
    list-style: none;
    margin: 0;
    padding: 0 12px;
    overflow: auto;
    /* A real reading area that the person can make taller or shorter. */
    height: max(240px, 40vh);
    min-height: 120px;
    resize: vertical;
    flex: none;
  }
  .transcript:focus-visible {
    outline: 2px solid #7aa2ff;
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
  li {
    padding: 6px 0;
    border-bottom: 1px solid #23232a;
  }
  .sub {
    padding-left: 12px;
    border-left: 2px solid #4a4a57;
  }
  .who {
    font-size: 11px;
    color: #b4b4c0;
  }
  .tool_result summary,
  .error .who {
    font-weight: 600;
  }
  .turn_end .who {
    font-style: italic;
  }
  p,
  pre {
    margin: 2px 0 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  pre {
    max-height: 200px;
    overflow: auto;
    font-size: 12px;
    background: #121216;
    padding: 4px 6px;
    border-radius: 3px;
  }
</style>
