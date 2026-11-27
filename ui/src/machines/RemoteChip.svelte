<script lang="ts">
// "on <name>" in a remote machine's title bar (ADR 0016), with the link's
// dot and Disconnect. The name comes from the app, never from the machine,
// so a remote window cannot pass for the laptop's own.
import type { RemoteLink, RemoteWindow } from "../lib/transport";
import { linkMessage } from "./machines.svelte";

let { remote }: { remote: RemoteWindow } = $props();

let link = $state<RemoteLink>({ state: "connecting", reason: null, details: null });
$effect(() => remote.onLink((next) => (link = next)));
</script>

<span class="chip" data-testid="remote-chip" data-state={link.state} title={linkMessage(remote.machine.name, link)}>
  <span class="dot" aria-hidden="true"></span>
  <span class="on">on {remote.machine.name}</span>
  <span class="visually-hidden">{link.state === "open" ? "connected" : link.state === "connecting" ? "connecting" : "disconnected"}</span>
  {#if link.state === "open" || link.state === "connecting"}
    <button type="button" onclick={() => remote.disconnect()} data-testid="remote-disconnect">Disconnect</button>
  {/if}
</span>

<style>
  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
    padding: 0 var(--sp-1) 0 var(--sp-2);
    height: var(--ctl-sm);
    border-radius: var(--r-sm);
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
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
