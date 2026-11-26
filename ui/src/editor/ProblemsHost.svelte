<script lang="ts">
// Test-only host: the Problems tab and the status bar's counts with the shell's context.
import type { Transport } from "../lib/transport";
import { provideOvra } from "../shell/context";
import type { Workspace } from "../shell/workspace.svelte";
import ProblemCounts from "./ProblemCounts.svelte";
import ProblemsDock from "./ProblemsDock.svelte";

let { transport, workspace }: { transport: Transport; workspace: Workspace } = $props();

// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });
</script>

<ProblemCounts />
<ProblemsDock />
