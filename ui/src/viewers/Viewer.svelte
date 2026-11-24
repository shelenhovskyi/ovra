<script lang="ts">
// Shows workspace.openPath in view mode: markdown, images, diffs, plain text.
import { useOvra } from "../shell/context";
import ImageView from "./ImageView.svelte";
import Lines from "./Lines.svelte";
import { formatSize, type Loaded, loadView, touches } from "./load";
import MarkdownView from "./MarkdownView.svelte";
import PatchView from "./PatchView.svelte";

const { transport, workspace } = useOvra();

let loaded = $state<Loaded | null>(null);
let notice = $state("");
let root = $state<HTMLElement | null>(null);
let content = $state<HTMLElement | null>(null);
// Whether keyboard focus was in the viewer, so it can follow to the next file
// (the focused link or button is gone once the content changes).
let hadFocus = false;

$effect(() => {
  const project = workspace.project?.id;
  const path = workspace.openPath;
  loaded = null;
  notice = "";
  if (!project || !path) return;
  let latest = 0;
  const load = () => {
    const seq = ++latest;
    void loadView(transport, project, path).then((result) => {
      if (seq === latest) loaded = result;
    });
  };
  load();
  const off = transport.onEvent((event) => {
    if (touches(event, project, path)) load();
  });
  return () => {
    latest = -1;
    off();
  };
});

$effect(() => {
  if (!loaded || !content || !hadFocus) return;
  const active = document.activeElement;
  if (active === null || active === document.body || !root?.contains(active)) {
    const target = content.querySelector<HTMLElement>("[tabindex='0']") ?? content;
    target.focus();
  }
});

async function openExternal() {
  const project = workspace.project?.id;
  const path = workspace.openPath;
  if (!project || !path) return;
  try {
    await transport.call({ method: "open.external", params: { project, path, line: null } });
    notice = `Opened ${path} in its default app.`;
  } catch {
    notice = `Could not open ${path} in another app.`;
  }
}
</script>

<div
  class="viewer"
  data-panel="Viewer"
  bind:this={root}
  onfocusin={() => (hadFocus = true)}
  onfocusout={(event) => {
    if (event.relatedTarget instanceof Node && !root?.contains(event.relatedTarget)) hadFocus = false;
  }}
>
  {#if !workspace.openPath}
    <p class="note">Open a file from the tree on the left.</p>
  {:else}
    <div class="bar">
      <span class="path" title={workspace.openPath}>{workspace.openPath}</span>
      {#if !loaded || !("view" in loaded) || (loaded.view.kind !== "binary" && loaded.view.kind !== "too_large")}
        <button type="button" onclick={openExternal}>Open in default app</button>
      {/if}
    </div>
    <p class="notice" role="status">{notice}</p>
    <div class="content" bind:this={content} tabindex="-1">
      {#if !loaded}
        <p class="note">Loading {workspace.openPath}…</p>
      {:else if "error" in loaded}
        <p class="note error">{loaded.error}</p>
      {:else if loaded.view.kind === "markdown"}
        <MarkdownView
          text={loaded.view.text}
          path={workspace.openPath}
          open={(path) => workspace.openFile(path)}
          notify={(message) => (notice = message)}
        />
      {:else if loaded.view.kind === "image"}
        <ImageView src={loaded.view.src} path={workspace.openPath} />
      {:else if loaded.view.kind === "patch"}
        <PatchView text={loaded.view.text} />
      {:else if loaded.view.kind === "text"}
        <Lines items={loaded.view.text.split(/\r?\n/)} label="File text" testid="text-view">
          {#snippet row(line)}
            <div class="text-line">{line}</div>
          {/snippet}
        </Lines>
      {:else if loaded.view.kind === "binary"}
        <p class="note">
          This is a binary file ({loaded.view.mime}, {formatSize(loaded.size)}) and cannot be shown here.
          <button type="button" onclick={openExternal}>Open in default app</button>
        </p>
      {:else}
        <p class="note">
          This file is {formatSize(loaded.size)}, too large to show here.
          <button type="button" onclick={openExternal}>Open in default app</button>
        </p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .viewer {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 12px;
    border-bottom: 1px solid #2a2a31;
    font-size: 12px;
  }
  .path {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #b4b4c0;
  }
  .notice {
    margin: 0;
    padding: 0 12px;
    font-size: 12px;
    color: #b4b4c0;
  }
  .notice:empty {
    display: none;
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow: auto;
    outline: none;
  }
  .note {
    padding: 12px;
    margin: 0;
    color: #8a8a96;
  }
  .error {
    color: #ff6b6b;
  }
  .text-line {
    padding: 0 12px;
  }
</style>
