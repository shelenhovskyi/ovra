<script lang="ts">
// Test-only: provides the Ovra context and mounts the terminal panel, as the shell does.
import { type Ovra, provideOvra } from "../shell/context";
import Terminals from "./Terminals.svelte";

let { ovra }: { ovra: Ovra } = $props();
// svelte-ignore state_referenced_locally
provideOvra(ovra);
</script>

<button type="button" data-testid="outside">outside</button>
<Terminals />
