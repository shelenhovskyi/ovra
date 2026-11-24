<script lang="ts">
// Test-only: provides the Ovra context and mounts the palette while it is open, as the shell does.
import { type Ovra, provideOvra } from "../shell/context";
import Palette from "./Palette.svelte";

let { ovra }: { ovra: Ovra } = $props();
// svelte-ignore state_referenced_locally
provideOvra(ovra);
</script>

{#if ovra.workspace.paletteOpen}<Palette />{/if}
