<script lang="ts">
import type { CoreInfo } from "./lib/protocol";
import type { Transport } from "./lib/transport";

let { transport }: { transport: Transport } = $props();

let info = $state<CoreInfo | null>(null);
let error = $state<string | null>(null);

$effect(() => {
  transport
    .call({ method: "get_info" })
    .then((response) => {
      info = response.data;
    })
    .catch((err: unknown) => {
      error = err instanceof Error ? err.message : String(err);
    });
});
</script>

<main>
  <h1>Ovra</h1>
  {#if info}
    <p data-testid="status">Connected to {info.name} {info.version} ({info.mode})</p>
  {:else if error}
    <p data-testid="status" class="error">Core unavailable: {error}</p>
  {:else}
    <p data-testid="status">Connecting…</p>
  {/if}
</main>

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    font-family: system-ui, sans-serif;
    background: #16161a;
    color: #e6e6e6;
  }
  main {
    padding: 2rem;
  }
  .error {
    color: #ff6b6b;
  }
</style>
