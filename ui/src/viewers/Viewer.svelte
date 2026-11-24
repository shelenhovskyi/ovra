<script lang="ts">
// Shows workspace.openPath in view mode: markdown, images, diffs, plain text.
import { useOvra } from "../shell/context";
import ImageView from "./ImageView.svelte";
import { type Loaded, loadView, touches } from "./load";
import MarkdownView from "./MarkdownView.svelte";
import PatchView from "./PatchView.svelte";

const { transport, workspace } = useOvra();

let loaded = $state<Loaded | null>(null);

$effect(() => {
  const project = workspace.project?.id;
  const path = workspace.openPath;
  loaded = null;
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
</script>

<div class="viewer" data-panel="Viewer">
  {#if !workspace.openPath}
    <p class="note">Viewer: open a file from the tree.</p>
  {:else if !loaded}
    <p class="note">Loading {workspace.openPath}…</p>
  {:else if "error" in loaded}
    <p class="note error">{loaded.error}</p>
  {:else if loaded.view.kind === "markdown"}
    <MarkdownView
      text={loaded.view.text}
      path={workspace.openPath}
      open={(path) => workspace.openFile(path)}
    />
  {:else if loaded.view.kind === "image"}
    <ImageView src={loaded.view.src} alt={workspace.openPath} />
  {:else if loaded.view.kind === "patch"}
    <PatchView text={loaded.view.text} />
  {:else if loaded.view.kind === "text"}
    <pre class="text" data-testid="text-view">{loaded.view.text}</pre>
  {:else if loaded.view.kind === "binary"}
    <p class="note">Binary file ({loaded.view.mime}) cannot be shown.</p>
  {:else}
    <p class="note">This file is too large to show.</p>
  {/if}
</div>

<style>
  .viewer {
    height: 100%;
    overflow: auto;
  }
  .note {
    padding: 12px;
    margin: 0;
    color: #6f6f7a;
  }
  .error {
    color: #ff6b6b;
  }
  .text {
    margin: 0;
    padding: 8px 12px;
    font-family: ui-monospace, monospace;
    font-size: 12px;
    white-space: pre;
  }
</style>
