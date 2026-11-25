<script lang="ts">
// Test-only: the header's project switcher next to the sidebar tree, which
// owns the start screen and the Mod+O folder dialog command.
import Tree from "../tree/Tree.svelte";
import { type Ovra, provideOvra } from "./context";
import ProjectSwitcher from "./ProjectSwitcher.svelte";

let { ovra }: { ovra: Ovra } = $props();
// svelte-ignore state_referenced_locally
provideOvra(ovra);
// svelte-ignore state_referenced_locally
const { workspace } = ovra;
</script>

{#if workspace.project}<ProjectSwitcher project={workspace.project} />{/if}
<Tree />
