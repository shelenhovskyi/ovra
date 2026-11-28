<script lang="ts">
// Test-only host: a view frame with a text field and a checkbox inside, shown while its view is.
import { mainView } from "./main-view.svelte";
import ViewFrame from "./ViewFrame.svelte";
</script>

{#if mainView.current === "audit"}
  <ViewFrame name="audit" testid="frame">
    <input aria-label="Match" />
    <input type="checkbox" aria-label="Tick" />
  </ViewFrame>
{/if}
