<script lang="ts">
// Test-only host: the browser's header button and pane with the shell's context.
import type { Transport } from "../lib/transport";
import { provideOvra } from "../shell/context";
import type { Workspace } from "../shell/workspace.svelte";
import BrowserButton from "./BrowserButton.svelte";
import BrowserPane from "./BrowserPane.svelte";

let { transport, workspace }: { transport: Transport; workspace: Workspace } = $props();

// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });
</script>

<BrowserButton />
<BrowserPane />
