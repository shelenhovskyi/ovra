<script lang="ts">
// Test-only host: the approvals with the shell's context and key handling.
import Keybindings from "../keys/Keybindings.svelte";
import type { Transport } from "../lib/transport";
import { provideOvra } from "../shell/context";
import type { Workspace } from "../shell/workspace.svelte";
import Approvals from "./Approvals.svelte";

let { transport, workspace }: { transport: Transport; workspace: Workspace } = $props();

// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });
</script>

<Keybindings />
<button type="button" data-testid="elsewhere">Elsewhere</button>
<Approvals />
