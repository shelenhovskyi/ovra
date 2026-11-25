<script lang="ts">
// Test-only host: the git panel with the shell's context, and its diff where
// the shell puts it.
import type { Transport } from "../lib/transport";
import { provideOvra } from "../shell/context";
import type { Workspace } from "../shell/workspace.svelte";
import Git from "./Git.svelte";
import GitDiff from "./GitDiff.svelte";

let { transport, workspace }: { transport: Transport; workspace: Workspace } = $props();

// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });
</script>

<Git />
{#if workspace.gitDiff}
  <GitDiff target={workspace.gitDiff} />
{/if}
