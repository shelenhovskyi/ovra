<script lang="ts">
// What an agent says, with its Markdown shown: lists, code, emphasis, links.
// `renderChat` sanitizes the agent's text; links never navigate the window,
// a click hands the link to `onlink`. `streaming` skips code colours while
// the text still grows.
import { paintCodeBlocks } from "../viewers/paint-code";
import { renderChat } from "./markdown";

let {
  text,
  streaming = false,
  onlink,
}: { text: string; streaming?: boolean; onlink?: (href: string) => void } = $props();

const html = $derived(renderChat(text));
let root = $state<HTMLElement>();

$effect(() => {
  void html;
  if (root && !streaming) paintCodeBlocks(root);
});

function onclick(event: MouseEvent): void {
  const anchor = (event.target as Element).closest("a");
  if (!anchor) return;
  event.preventDefault();
  const href = anchor.getAttribute("href");
  if (href) onlink?.(href);
}
</script>

<!-- Links are buttons for the mouse; the keyboard reaches them as links (Enter clicks). -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="md"
  data-testid="agent-text"
  bind:this={root}
  {onclick}
  onauxclick={(event) => {
    // A middle click would open the link in a new window, past onlink.
    if ((event.target as Element).closest("a")) event.preventDefault();
  }}
>
  <!-- Safe: renderChat escapes raw HTML and keeps only an allowlist of tags and attributes. -->
  {@html html}
</div>

<style>
  .md {
    margin-top: 2px;
    line-height: 1.55;
    overflow-wrap: anywhere;
  }
  .md > :global(:first-child) {
    margin-top: 0;
  }
  .md > :global(:last-child) {
    margin-bottom: 0;
  }
  .md :global(p),
  .md :global(ul),
  .md :global(ol),
  .md :global(pre),
  .md :global(blockquote),
  .md :global(table) {
    margin: 0 0 8px;
  }
  .md :global(ul),
  .md :global(ol) {
    padding-left: 20px;
  }
  .md :global(ul) {
    list-style: disc;
  }
  .md :global(ul ul) {
    list-style: circle;
  }
  .md :global(li + li) {
    margin-top: 2px;
  }
  .md :global(li > ul),
  .md :global(li > ol) {
    margin: 2px 0 0;
  }
  .md :global(h1),
  .md :global(h2),
  .md :global(h3),
  .md :global(h4),
  .md :global(h5),
  .md :global(h6) {
    margin: 12px 0 4px;
    font-size: 14px;
    font-weight: 600;
  }
  .md :global(.md-h1) {
    font-size: 16px;
  }
  .md :global(.md-h2) {
    font-size: 15px;
  }
  .md :global(a) {
    color: var(--accent);
  }
  .md :global(code) {
    font-family: var(--mono);
    font-size: 12.5px;
    background: var(--s3);
    padding: 1px 4px;
    border-radius: var(--radius-xs);
  }
  /* --s1: comments in code keep 4.5:1 on it, as in the Markdown viewer. */
  .md :global(pre) {
    max-height: 360px;
    overflow: auto;
    background: var(--s1);
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    white-space: pre;
    overflow-wrap: normal;
  }
  .md :global(pre code) {
    background: none;
    padding: 0;
  }
  .md :global(blockquote) {
    padding-left: 10px;
    border-left: 3px solid var(--s4);
    color: var(--text-2);
  }
  .md :global(table) {
    display: block;
    overflow-x: auto;
    border-collapse: collapse;
    font-size: 13px;
  }
  .md :global(th),
  .md :global(td) {
    border: 1px solid var(--s4);
    padding: 3px 8px;
  }
  .md :global(hr) {
    border: none;
    height: 1px;
    background: var(--s3);
  }
</style>
