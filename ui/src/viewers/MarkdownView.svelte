<script lang="ts">
import { useOvra } from "../shell/context";
import { resolveLink } from "./links";
import { loadView } from "./load";
import { renderMarkdown } from "./markdown";

let { text, path, open }: { text: string; path: string; open: (path: string) => void } = $props();

const { transport, workspace } = useOvra();

const html = $derived(renderMarkdown(text));
let root = $state<HTMLElement | null>(null);

// Project images referenced by the document are read through the core. Each
// image is requested once; a result for an image no longer shown is dropped.
$effect(() => {
  void html;
  const project = workspace.project?.id;
  if (!root || !project) return;
  for (const img of root.querySelectorAll<HTMLImageElement>("img[data-src]")) {
    const link = resolveLink(path, img.dataset.src ?? "");
    img.removeAttribute("data-src");
    if (link.kind !== "file") continue;
    void loadView(transport, project, link.path).then((result) => {
      if (root?.contains(img) && "view" in result && result.view.kind === "image") img.src = result.view.src;
    });
  }
});

function onclick(event: MouseEvent) {
  const anchor = (event.target as Element).closest("a");
  if (!anchor) return;
  // The viewer never navigates the window away from the IDE.
  event.preventDefault();
  const link = resolveLink(path, anchor.getAttribute("href") ?? "");
  if (link.kind === "file") open(link.path);
  else if (link.kind === "web") void transport.call({ method: "open.url", params: { url: link.url } }).catch(() => {});
  else if (link.kind === "anchor") root?.querySelector(`[id="${CSS.escape(link.id)}"]`)?.scrollIntoView();
}
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<article class="markdown" data-testid="markdown-view" bind:this={root} {onclick}>
  <!-- Safe: renderMarkdown escapes raw HTML and drops unsafe URLs. -->
  {@html html}
</article>

<style>
  .markdown {
    max-width: 860px;
    padding: 16px 24px;
    line-height: 1.6;
    font-size: 14px;
  }
  .markdown :global(a) {
    color: #7aa2f7;
  }
  .markdown :global(code) {
    font-family: ui-monospace, monospace;
    font-size: 12px;
    background: #22222a;
    padding: 1px 4px;
    border-radius: 3px;
  }
  .markdown :global(pre) {
    background: #1d1d23;
    padding: 10px 12px;
    border-radius: 4px;
    overflow: auto;
  }
  .markdown :global(pre code) {
    background: none;
    padding: 0;
  }
  .markdown :global(table) {
    border-collapse: collapse;
  }
  .markdown :global(th),
  .markdown :global(td) {
    border: 1px solid #2a2a31;
    padding: 4px 8px;
  }
  .markdown :global(blockquote) {
    margin: 0;
    padding-left: 12px;
    border-left: 3px solid #2a2a31;
    color: #9a9aa6;
  }
  .markdown :global(img) {
    max-width: 100%;
  }
  .markdown :global(h1),
  .markdown :global(h2) {
    border-bottom: 1px solid #2a2a31;
    padding-bottom: 4px;
  }
</style>
