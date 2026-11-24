<script lang="ts">
// Test-only host: gives Viewer the context the shell normally provides.
import type { Transport } from "../lib/transport";
import { provideOvra } from "../shell/context";
import type { Workspace } from "../shell/workspace.svelte";
import Viewer from "./Viewer.svelte";

let { transport, workspace }: { transport: Transport; workspace: Workspace } = $props();

// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });
</script>

<Viewer />
