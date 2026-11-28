<script lang="ts">
// "on <name>" in a remote machine's title bar (ADR 0016), with the link's
// dot, Disconnect (Ovra there keeps running) and Stop (it ends, with its
// agents, after the person confirms). The name comes from the app, never
// from the machine, so a remote window cannot pass for the laptop's own.
import type { RemoteLink, RemoteWindow } from "../lib/transport";
import { linkMessage } from "./machines.svelte";

let { remote }: { remote: RemoteWindow } = $props();

let link = $state<RemoteLink>({ state: "connecting", reason: null, details: null });
let stopping = $state(false);
$effect(() =>
  remote.onLink((next) => {
    link = next;
    if (next.state !== "open") stopping = false;
  }),
);

function stop(): void {
  stopping = false;
  remote.stop();
}
</script>

<span class="chip" data-testid="remote-chip" data-state={link.state} title={linkMessage(remote.machine.name, link)}>
  <span class="dot" aria-hidden="true"></span>
  <span class="on">on {remote.machine.name}</span>
  <span class="visually-hidden">{link.state === "open" ? "connected" : link.state === "connecting" ? "connecting" : "disconnected"}</span>
  {#if link.state === "open" || link.state === "connecting"}
    <button type="button" onclick={() => remote.disconnect()} data-testid="remote-disconnect">Disconnect</button>
  {/if}
  {#if link.state === "open"}
    <button
      type="button"
      onclick={() => (stopping = !stopping)}
      aria-expanded={stopping}
      aria-label={`Stop Ovra on ${remote.machine.name}`}
      data-testid="remote-stop">Stop</button
    >
  {/if}
  {#if stopping}
    <span class="confirm" role="dialog" aria-label={`Stop Ovra on ${remote.machine.name}`}>
      <span>Stop Ovra on {remote.machine.name}? Agents running there stop. Their sessions can be resumed later.</span>
      <span class="actions">
        <button type="button" class="danger" onclick={stop} data-testid="remote-stop-confirm">Stop Ovra</button>
        <button type="button" onclick={() => (stopping = false)}>Cancel</button>
      </span>
    </span>
  {/if}
</span>

<style>
  .chip {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
    padding: 0 var(--sp-1) 0 var(--sp-2);
    height: var(--ctl-sm);
    border-radius: var(--r-control);
    color: var(--accent);
    background: var(--accent-soft);
    font: var(--t-caption);
    white-space: nowrap;
  }
  .on {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dot {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 50%;
    background: var(--text-3);
  }
  [data-state="open"] .dot {
    background: var(--ok);
  }
  [data-state="connecting"] .dot {
    background: var(--warn);
  }
  [data-state="failed"] .dot,
  [data-state="closed"] .dot {
    background: var(--danger);
  }
  button {
    height: 20px;
    padding: 0 var(--sp-2);
    font: inherit;
    color: var(--text);
    background: transparent;
    border: 0;
    border-radius: var(--r-xs);
    cursor: default;
  }
  button:hover {
    background: var(--s3);
  }
  .confirm {
    position: absolute;
    top: calc(100% + var(--sp-1));
    right: 0;
    z-index: 30;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    width: 280px;
    padding: var(--sp-2) var(--sp-3);
    color: var(--text);
    background: var(--s4);
    border-radius: var(--r-md);
    box-shadow: var(--card-shadow);
    font: var(--t-small);
    white-space: normal;
  }
  .actions {
    display: flex;
    gap: var(--sp-2);
  }
  .confirm button {
    height: var(--ctl-sm);
    background: var(--s3);
  }
  .confirm button.danger {
    color: var(--danger);
    background: var(--danger-soft);
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
