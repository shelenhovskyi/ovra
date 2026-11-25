<script lang="ts">
// Sidebar git view for the open project, and the "Show git changes" command.
import { tick } from "svelte";
import { registerCommand } from "../keys/commands";
import { useOvra } from "../shell/context";
import GitPanel from "./GitPanel.svelte";

const { workspace } = useOvra();

$effect(() =>
  registerCommand({
    id: "git.show",
    title: "Show git changes",
    keys: "Mod+Shift+G",
    run: async () => {
      workspace.sidebar = "git";
      await tick();
      // The commit message when there is a repository, else the panel's first action.
      const panel = document.querySelector<HTMLElement>("[data-testid=git]");
      (
        panel?.querySelector<HTMLElement>("textarea") ??
        panel?.querySelector<HTMLElement>("button") ??
        panel?.querySelector<HTMLElement>("#git-title")
      )?.focus();
    },
  }),
);
</script>

{#if workspace.project}
  {#key workspace.project.id}
    <GitPanel project={workspace.project.id} />
  {/key}
{/if}
