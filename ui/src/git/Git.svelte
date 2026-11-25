<script lang="ts">
// Sidebar git view for the open project, and the "Show git" command.
import { registerCommand } from "../keys/commands";
import { useOvra } from "../shell/context";

const { workspace } = useOvra();

$effect(() =>
  registerCommand({
    id: "git.show",
    title: "Show git changes",
    keys: "Mod+Shift+G",
    run: () => {
      workspace.sidebar = "git";
    },
  }),
);
</script>

{#if workspace.project}
  <p class="empty">Git changes appear here.</p>
{/if}

<style>
  .empty {
    margin: 12px;
    color: #9a9aa6;
  }
</style>
