<script lang="ts">
// One session's messages. A log region, so screen readers hear new messages.
import { describeTool } from "../approvals/describe";
import type { AgentMessage } from "../lib/protocol";

let { messages }: { messages: AgentMessage[] } = $props();

const MAX_OUTPUT = 2000;
let box = $state<HTMLElement>();

function clip(text: string): string {
  if (text.length <= MAX_OUTPUT) return text;
  return `${text.slice(0, MAX_OUTPUT)}\n… ${text.length - MAX_OUTPUT} more characters`;
}

function usage(message: Extract<AgentMessage, { kind: "turn_end" }>): string {
  const { input_tokens, output_tokens, cost_micro_usd } = message.usage;
  const cost = cost_micro_usd === null ? "" : `, $${(cost_micro_usd / 1_000_000).toFixed(4)}`;
  return `Turn ended: ${input_tokens} tokens in, ${output_tokens} out${cost}`;
}

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

<ol class="transcript" role="log" aria-label="Transcript" bind:this={box} data-testid="transcript">
  {#each messages as message, i (i)}
    {@const sub = "parent" in message && message.parent ? "Subagent " : ""}
    <li class={message.kind} class:sub={sub !== ""}>
      {#if message.kind === "user_text"}
        <span class="who">You</span>
        <p>{message.text}</p>
      {:else if message.kind === "assistant_text"}
        <span class="who">{sub}Claude</span>
        <p>{message.text}</p>
      {:else if message.kind === "tool_use"}
        <span class="who">{sub}Tool {message.name}</span>
        <p>{describeTool(message.name, message.input).action}</p>
      {:else if message.kind === "tool_result"}
        <span class="who">{sub}{message.is_error ? "Tool failed" : "Tool result"}</span>
        {#if message.output}<pre>{clip(message.output)}</pre>{/if}
      {:else if message.kind === "turn_end"}
        <span class="who">{usage(message)}</span>
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
    flex: 1;
    min-height: 0;
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
  .tool_result .who:first-child,
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
