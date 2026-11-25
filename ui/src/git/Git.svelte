<script lang="ts">
// Sidebar git view for the open project, and its palette commands: show
// the panel, stage all, commit, new branch, switch branch.
import { tick } from "svelte";
import { registerCommand } from "../keys/commands";
import { useOvra } from "../shell/context";
import { focusDiffRow, focusMessage } from "./focus";
import GitPanel from "./GitPanel.svelte";
import { fileLists, type GitModel, gitModel } from "./model.svelte";

const { workspace } = useOvra();

/** Shows the panel and waits for its first status, so focus lands on something visible. */
async function showPanel(): Promise<GitModel | undefined> {
  workspace.sidebar = "git";
  await tick();
  const model = workspace.project ? gitModel(workspace.project.id) : undefined;
  await model?.loaded();
  await tick();
  return model;
}

/** The commit message when there is a repository, else the panel's first button. */
function focusPanel(): void {
  const panel = document.querySelector<HTMLElement>("[data-testid=git]");
  (panel?.querySelector<HTMLElement>("textarea") ?? panel?.querySelector<HTMLElement>("button"))?.focus();
}

$effect(() => {
  const off = [
    registerCommand({
      id: "git.show",
      title: "Git: open panel",
      keys: "Mod+Shift+G",
      run: async () => {
        // Without a project there is no git to show: go where one is opened.
        if (!workspace.project) {
          document.getElementById("project-root")?.focus();
          return;
        }
        await showPanel();
        // Back from a diff: its row, to go on with the next file.
        const diff = workspace.gitDiff;
        if (!diff || diff.project !== workspace.project?.id || !focusDiffRow(diff)) focusPanel();
      },
    }),
    registerCommand({
      id: "git.stage_all",
      title: "Git: stage all changes",
      run: async () => {
        const model = await showPanel();
        if (model?.state.kind !== "ready") {
          focusPanel();
          return;
        }
        const paths = fileLists(model.state.status.files).changes.map((f) => f.path);
        if (paths.length === 0) model.notice = "There are no changes to stage.";
        else await model.stage(paths);
        focusMessage();
      },
    }),
    registerCommand({
      id: "git.commit",
      title: "Git: commit",
      run: async () => {
        await showPanel();
        focusPanel();
      },
    }),
    registerCommand({
      id: "git.new_branch",
      title: "Git: new branch…",
      run: async () => {
        const model = await showPanel();
        if (model?.state.kind === "ready") model.branchMenu = "create";
        else focusPanel();
      },
    }),
    registerCommand({
      id: "git.switch_branch",
      title: "Git: switch branch…",
      run: async () => {
        const model = await showPanel();
        if (model?.state.kind === "ready") model.branchMenu = "switch";
        else focusPanel();
      },
    }),
  ];
  return () => {
    for (const unregister of off) unregister();
  };
});
</script>

{#if workspace.project}
  {#key workspace.project.id}
    <GitPanel project={workspace.project.id} />
  {/key}
{/if}
