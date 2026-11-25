<script lang="ts">
// Test-only host: the Servers pane and the Servers tab of the dock with the shell's context.
import type { Transport } from "../lib/transport";
import { provideOvra } from "../shell/context";
import type { Workspace } from "../shell/workspace.svelte";
import ServersDock from "./ServersDock.svelte";
import ServersPanel from "./ServersPanel.svelte";

let { transport, workspace }: { transport: Transport; workspace: Workspace } = $props();

// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });
</script>

<ServersPanel />
<ServersDock />
