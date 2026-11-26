<script lang="ts">
// A screen's title, with Back to Home on every screen but Home, and the
// connection state when it is not open.
import type { ConnectionStatus } from "../lib/transport";

let {
  title,
  status,
  back = true,
  link,
}: { title: string; status: ConnectionStatus; back?: boolean; link?: { href: string; text: string } } = $props();

const offline = $derived(
  status === "open"
    ? null
    : status === "connecting"
      ? "Connecting to your computer…"
      : status === "refused"
        ? "Your computer refuses the connection. Reload the page to try again."
        : "Not connected. Trying again…",
);
</script>

<header>
  {#if back}<a class="back" href="#/">Back</a>{/if}
  <h1>{title}</h1>
  {#if link}<a class="link" href={link.href}>{link.text}</a>{/if}
</header>
{#if offline}<p class="offline" role="status" data-testid="connection">{offline}</p>{/if}

<style>
  header {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: 44px;
    margin-bottom: var(--sp-3);
  }
  h1 {
    font: var(--t-title1);
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .link {
    margin-left: auto;
  }
  .back,
  .link {
    color: var(--accent);
    text-decoration: none;
    min-height: 44px;
    display: inline-flex;
    align-items: center;
  }
  .offline {
    background: var(--warn-soft);
    color: var(--warn);
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--r-md);
  }
</style>
