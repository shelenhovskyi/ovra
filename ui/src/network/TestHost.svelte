<script lang="ts">
// Test-only host: the Network or Keys pane with the shell's context.
import type { Transport } from "../lib/transport";
import { provideOvra } from "../shell/context";
import type { Workspace } from "../shell/workspace.svelte";
import KeysPane from "./KeysPane.svelte";
import NetworkPanel from "./NetworkPanel.svelte";

let {
  transport,
  workspace,
  pane = "network",
}: { transport: Transport; workspace: Workspace; pane?: "network" | "keys" } = $props();

// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });
</script>

{#if pane === "keys"}<KeysPane />{:else}<NetworkPanel />{/if}
