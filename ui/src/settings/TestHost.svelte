<script lang="ts">
// Test-only host: the Settings view with the shell's context, opened as the shell opens it.
import type { Transport } from "../lib/transport";
import { provideOvra } from "../shell/context";
import { mainView } from "../shell/main-view.svelte";
import type { Workspace } from "../shell/workspace.svelte";
import Settings from "./Settings.svelte";

let { transport, workspace }: { transport: Transport; workspace: Workspace } = $props();

// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });
</script>

{#if mainView.current === "settings"}<Settings />{/if}
