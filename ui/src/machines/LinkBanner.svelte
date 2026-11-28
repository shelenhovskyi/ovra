<script lang="ts">
// The bar a remote machine's window shows while its link is down: what
// happened, in words Ovra chose, with what `ssh` said folded under Details
// as plain text (it is the remote machine's own), and Reconnect. Nothing
// reconnects on its own: after `replaced` that would take the machine back
// from the other window, and that one would take it back again.
import type { RemoteLink, RemoteWindow } from "../lib/transport";
import { linkMessage } from "./machines.svelte";

let { remote }: { remote: RemoteWindow } = $props();

let link = $state<RemoteLink>({ state: "connecting", reason: null, details: null });
$effect(() => remote.onLink((next) => (link = next)));
const down = $derived(link.state === "closed" || link.state === "failed" || link.state === "stopped");
</script>

{#if down}
  <div class="banner" role="alert" data-testid="remote-banner">
    <p>{linkMessage(remote.machine.name, link)}</p>
    {#if link.details}
      <details>
        <summary>Details</summary>
        <pre>{link.details}</pre>
      </details>
    {/if}
    <button type="button" onclick={() => remote.reconnect()} data-testid="remote-reconnect">Reconnect</button>
  </div>
{/if}

<style>
  .banner {
    position: fixed;
    top: calc(var(--title-h, 40px) + var(--sp-2));
    left: 50%;
    z-index: 20;
    transform: translateX(-50%);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2) var(--sp-3);
    box-sizing: border-box;
    width: min(640px, calc(100vw - 2 * var(--sp-4)));
    padding: var(--sp-2) var(--sp-3);
    color: var(--text);
    background: var(--s4);
    border-radius: var(--r-md);
    box-shadow: var(--card-shadow);
  }
  p {
    flex: 1;
    margin: 0;
  }
  details {
    flex-basis: 100%;
    order: 3;
    font: var(--t-small);
    color: var(--text-2);
  }
  pre {
    max-height: 160px;
    margin: var(--sp-1) 0 0;
    overflow: auto;
    font-family: var(--mono);
    white-space: pre-wrap;
    word-break: break-all;
  }
  button {
    height: var(--ctl);
    padding: 0 var(--sp-3);
    font: inherit;
    font-weight: 500;
    color: var(--accent-ink);
    background: var(--accent);
    border: 0;
    border-radius: var(--r-md);
    cursor: default;
  }
</style>
