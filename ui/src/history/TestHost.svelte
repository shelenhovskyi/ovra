<script lang="ts">
// Test-only host: History with the shell's context, opened as the shell opens it.
import type { HistoryEntry } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { provideOvra } from "../shell/context";
import { mainView } from "../shell/main-view.svelte";
import type { Workspace } from "../shell/workspace.svelte";
import History from "./History.svelte";

let {
  transport,
  workspace,
  onopen,
}: { transport: Transport; workspace: Workspace; onopen: (entry: HistoryEntry) => void } = $props();

// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });
</script>

{#if mainView.current === "history"}<History {onopen} />{/if}
