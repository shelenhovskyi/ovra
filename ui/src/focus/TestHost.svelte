<script lang="ts">
// Test-only host: Focus with the shell's context.
import type { Transport } from "../lib/transport";
import { provideOvra } from "../shell/context";
import type { Workspace } from "../shell/workspace.svelte";
import FocusBar from "./FocusBar.svelte";
import FocusView from "./FocusView.svelte";

let { transport, workspace }: { transport: Transport; workspace: Workspace } = $props();

// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });
</script>

<FocusBar />
<FocusView />
