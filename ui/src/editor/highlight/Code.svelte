<script lang="ts">
// One line of code: coloured segments once highlighted, plain text until then.
import type { Segment } from "./static";

let { text, segments }: { text: string; segments?: readonly Segment[] | null | undefined } = $props();
</script>

{#if segments}{#each segments as segment, i (i)}{#if segment.cls}<span class={segment.cls}>{segment.text}</span
      >{:else}{segment.text}{/if}{/each}{:else}{text}{/if}
