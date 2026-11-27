<script lang="ts">
// Test-only host: Settings > Remote machines with the shell's context.
import type { Transport } from "../lib/transport";
import { provideOvra } from "../shell/context";
import { Workspace } from "../shell/workspace.svelte";
import MachinesPane from "./MachinesPane.svelte";

let { transport }: { transport: Transport } = $props();

// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace: new Workspace() });
</script>

<MachinesPane />
