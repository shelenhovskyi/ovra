<script lang="ts">
// Test-only host: the search view with the shell's context.
import type { Transport } from "../lib/transport";
import { provideOvra } from "../shell/context";
import type { Workspace } from "../shell/workspace.svelte";
import Search from "./Search.svelte";

let { transport, workspace }: { transport: Transport; workspace: Workspace } = $props();

// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });
</script>

<button type="button" data-testid="search-tab" onclick={() => (workspace.sidebar = "search")}>Search</button>
<Search />
