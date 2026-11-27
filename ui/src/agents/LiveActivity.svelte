<script lang="ts">
// The end of a working session's transcript: the text the agent is writing as it
// streams in, and one line with what it does now and for how long, so a long
// turn never looks frozen. Not a live region: the transcript's own region
// speaks the agent's finished text.
import type { AgentMessage, AgentStatus, Stuck } from "../lib/protocol";
import AgentText from "./AgentText.svelte";
import { elapsed, liveStep } from "./steps";
import { stuckLine } from "./stuck";

/**
 * `agent`: the provider's display name. `root`: the session's folder; files in it show relative to it.
 * `stuck`: the core's hint that the turn may be stuck, one quiet line under what it does.
 */
let {
  agent = "Agent",
  status,
  messages,
  partial = "",
  root = null,
  stuck = null,
  onlink,
}: {
  agent?: string;
  status: AgentStatus;
  messages: AgentMessage[];
  partial?: string;
  root?: string | null;
  stuck?: Stuck | null;
  onlink?: (href: string) => void;
} = $props();

const hint = $derived(status.state === "working" ? stuckLine(stuck) : null);

const step = $derived(liveStep(status, messages, partial !== "", root));
let since = $state(Date.now());
let now = $state(Date.now());

// Each new activity counts from zero.
$effect.pre(() => {
  void step;
  since = Date.now();
  now = since;
});
$effect(() => {
  if (!step) return;
  const timer = setInterval(() => {
    now = Date.now();
  }, 1000);
  return () => clearInterval(timer);
});
const time = $derived(now - since >= 1000 ? elapsed(now - since) : "");
</script>

{#if partial && status.state === "working"}
  <li class="assistant_text streaming" data-testid="streaming-text">
    <span class="who">{agent}</span>
    <AgentText text={partial} streaming {onlink} />
  </li>
{/if}
{#if step}
  <li class="live" data-testid="live-activity">
    <span class="dots" aria-hidden="true"><i></i><i></i><i></i></span>
    <span class="what">{step}</span>
    {#if time}<span class="time">· {time}</span>{/if}
  </li>
{/if}
{#if hint}
  <!-- A hint, not an alarm: Stop and the reply box are where they always are. -->
  <li class="stuck" data-testid="stuck-hint">{hint} Stop it, or send it a hint.</li>
{/if}

<style>
  .who {
    display: block;
    font-size: 13px;
    color: var(--text-2);
  }
  .live {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-2);
    min-width: 0;
  }
  .what {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .stuck {
    font-size: 13px;
    color: var(--text-2);
    border-left: 2px solid var(--warn);
    padding-left: 8px;
  }
  .time {
    flex: none;
    font-variant-numeric: tabular-nums;
  }
  .dots {
    display: inline-flex;
    gap: 3px;
    flex: none;
  }
  .dots i {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .dots i:nth-child(2) {
    animation-delay: 0.2s;
  }
  .dots i:nth-child(3) {
    animation-delay: 0.4s;
  }
  @keyframes pulse {
    0%,
    80%,
    100% {
      opacity: 0.25;
    }
    40% {
      opacity: 1;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .dots i {
      animation: none;
      opacity: 0.6;
    }
  }
  @media (forced-colors: active) {
    .dots i {
      background: CanvasText;
    }
  }
</style>
