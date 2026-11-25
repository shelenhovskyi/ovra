<script lang="ts" module>
import { registerContextKeys } from "../keys/context-keys";

// This panel's own keys, for Settings > Keyboard; the panel handles them itself.
registerContextKeys(
  [
    { keys: "S", title: "Stage" },
    { keys: "U", title: "Unstage" },
    { keys: "O", title: "Open the file" },
    { keys: "Enter", title: "Show the diff" },
    { keys: "Delete", title: "Discard changes…" },
    { keys: "Shift+F10", title: "Open the row's menu (also the Menu key)" },
    { keys: "Mod+Enter", title: "Commit" },
  ].map((key) => ({ ...key, where: "Git" })),
);
</script>

<script lang="ts">
// Sidebar git view for the open project, and its palette commands: show
// the panel, set up git, stage all, commit, new branch, switch branch.
import { tick } from "svelte";
import { registerCommand } from "../keys/commands";
import { useOvra } from "../shell/context";
import { focusDiffRow, focusFirstRow, focusMessage } from "./focus";
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

/**
 * Says `text` from a live region of its own on the page: without a project the panel is
 * hidden, and its own region with it. The same text twice is still read out.
 */
function sayOutside(text: string): void {
  let region = document.getElementById("git-outside-announce");
  if (!region) {
    region = document.createElement("p");
    region.id = "git-outside-announce";
    region.setAttribute("role", "status");
    region.setAttribute("data-testid", "git-outside-announce");
    region.style.cssText = "position:absolute;width:1px;height:1px;overflow:hidden;clip-path:inset(50%);margin:0";
    document.body.append(region);
  }
  region.textContent = region.textContent === text ? `${text}\u00a0` : text;
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
      keys: "Ctrl+Shift+G",
      run: async () => {
        // Without a project there is no git to show: say so, and go where one is opened.
        if (!workspace.project) {
          sayOutside("Open a project first. Git shows the changes in a project's folder.");
          document.getElementById("project-root")?.focus();
          return;
        }
        const model = await showPanel();
        // Back from a diff: its row, to go on with the next file.
        const diff = workspace.gitDiff;
        if (diff && diff.project === workspace.project?.id && focusDiffRow(diff)) return;
        // Nothing staged: the first changed file, where S stages it; the message comes after.
        const status = model?.state.kind === "ready" ? model.state.status : null;
        if (status && fileLists(status.files).staged.length === 0 && focusFirstRow()) return;
        focusPanel();
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
      id: "git.init",
      title: "Git: set up git",
      // `git init` in a folder without git; the panel opens only to show why it failed.
      run: async () => {
        const model = workspace.project ? gitModel(workspace.project.id) : undefined;
        await model?.loaded();
        if (model?.state.kind !== "no_repo" || !(await model.init())) await showPanel();
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
