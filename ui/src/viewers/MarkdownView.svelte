<script lang="ts">
import { highlightLines, languageNamed } from "../editor/highlight/static";
import { useOvra } from "../shell/context";
import { resolveLink } from "./links";
import { exists, loadView } from "./load";
import { renderMarkdown } from "./markdown";

let {
  text,
  path,
  open,
  notify,
}: { text: string; path: string; open: (path: string) => void; notify: (message: string) => void } = $props();

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

// Code blocks with a known language get the editor's colours. The text is
// set through textContent, never as HTML.
$effect(() => {
  void html;
  if (!root) return;
  for (const code of root.querySelectorAll<HTMLElement>("pre > code[class^='language-']")) {
    const language = languageNamed(code.className.slice("language-".length).split(" ")[0] ?? "");
    const lines = (code.textContent ?? "").replace(/\n$/, "").split("\n");
    void highlightLines(language, lines).then((painted) => {
      if (!painted || !root?.contains(code)) return;
      code.replaceChildren(
        ...painted.flatMap((segments, i) => [
          ...(i > 0 ? ["\n"] : []),
          ...segments.map(({ text, cls }) => {
            if (!cls) return text;
            const span = document.createElement("span");
            span.className = cls;
            span.textContent = text;
            return span;
          }),
        ]),
      );
    });
  }
});

function onclick(event: MouseEvent) {
  const anchor = (event.target as Element).closest("a");
  if (!anchor) return;
  // The viewer never navigates the window away from the IDE.
  event.preventDefault();
  const link = resolveLink(path, anchor.getAttribute("href") ?? "");
  if (link.kind === "file") void openFile(link.path);
  else if (link.kind === "web") void openWeb(link.url);
  else if (link.kind === "anchor") root?.querySelector(`[id="${CSS.escape(link.id)}"]`)?.scrollIntoView();
  else notify("This link cannot be opened from Ovra.");
}

async function openFile(target: string) {
  const project = workspace.project?.id;
  if (!project) return;
  if (await exists(transport, project, target)) open(target);
  else notify(`The link points to ${target}, which was not found.`);
}

async function openWeb(url: string) {
  notify(`Opening ${url} in your browser…`);
  try {
    await transport.call({ method: "open.url", params: { url } });
  } catch {
    notify(`Could not open ${url} in a browser.`);
  }
}
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<article class="markdown" data-testid="markdown-view" aria-label="Preview of {path.slice(path.lastIndexOf('/') + 1)}" bind:this={root} {onclick}>
  <!-- Safe: renderMarkdown escapes raw HTML and drops unsafe URLs. -->
  {@html html}
</article>

<style>
  .markdown {
    max-width: 820px;
    padding: 20px 32px;
    line-height: 1.6;
    font-size: 14px;
  }
  .markdown :global(a) {
    color: var(--accent);
  }
  .markdown :global(code) {
    font-family: var(--mono);
    font-size: 13px;
    background: var(--s3);
    padding: 1px 5px;
    border-radius: var(--radius-xs);
  }
  /* --s1, not --s3: comments in code keep 4.5:1 on it. */
  .markdown :global(pre) {
    background: var(--s1);
    padding: 12px 14px;
    border-radius: var(--radius);
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
    border: 1px solid var(--s4);
    padding: 4px 10px;
  }
  .markdown :global(blockquote) {
    margin: 0;
    padding-left: 12px;
    border-left: 3px solid var(--s3);
    color: var(--text-2);
  }
  .markdown :global(img) {
    max-width: 100%;
  }
  .markdown :global(h1),
  .markdown :global(h2) {
    font-weight: 600;
  }
</style>
