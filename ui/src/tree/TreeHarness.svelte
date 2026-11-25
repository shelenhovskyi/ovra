<script lang="ts">
// Test-only: provides the Ovra context and mounts the sidebar tree, as the shell does.
import { type Ovra, provideOvra } from "../shell/context";
import Tree from "./Tree.svelte";

let { ovra }: { ovra: Ovra } = $props();
// svelte-ignore state_referenced_locally
provideOvra(ovra);
</script>

<Tree />
