<script lang="ts">
// The edited file's path and the symbols around the cursor, on one line in
// the editor's bar. A path segment reveals the file in the tree; `children`
// (the unsaved dot) follows the file name.
import type { EditorView } from "@codemirror/view";
import { type Snippet, tick } from "svelte";
import { commandItem } from "../menu/context-menu.svelte";
import { useOvra } from "../shell/context";
import { pathCrumbs, scopedOutline, symbolTrail } from "./breadcrumbs";
import { type FileSymbol, serverSymbols } from "./symbols";

let {
  path,
  view,
  cursor,
  revision,
  outline = true,
  children,
}: {
  path: string;
  /** The view that shows the file, once it does. */
  view: EditorView | null;
  /** The cursor's offset in the text. */
  cursor: number;
  /** Changes with the text, so the symbols are read again. */
  revision: number;
  /** False for texts too large to read an outline from on every change. */
  outline?: boolean;
  children?: Snippet;
} = $props();

const { workspace } = useOvra();

/** Symbols are read again this long after the last change. */
const SYMBOLS_DELAY_MS = 300;

const crumbs = $derived(pathCrumbs(path));
let symbols = $state.raw<FileSymbol[]>([]);
const trail = $derived(symbolTrail(symbols, cursor));

// A new view or file starts with no symbols; a change reads them again.
$effect(() => {
  void view;
  void path;
  symbols = [];
});
$effect(() => {
  void revision;
  const target = view;
  const file = path;
  if (!target) return;
  let live = true;
  const timer = setTimeout(async () => {
    let found: FileSymbol[] | null = null;
    try {
      found = await serverSymbols(target);
    } catch {
      // The outline stands in for a server that did not answer.
    }
    if (!live) return;
    if (!found && !outline) return;
    symbols = found ?? scopedOutline(target.state.doc, file, target.state.tabSize);
  }, SYMBOLS_DELAY_MS);
  return () => {
    live = false;
    clearTimeout(timer);
  };
});

/** Shows the file in the tree, switching the sidebar to Files first. */
async function reveal(): Promise<void> {
  if (workspace.sidebar !== "files") {
    workspace.sidebar = "files";
    await tick();
  }
  commandItem("tree.reveal")?.run();
}
</script>

<nav class="crumbs" aria-label="Breadcrumbs" data-testid="editor-path" title={path}>
  <ol>
    {#each crumbs as crumb, i (crumb.path)}
      <li>
        <button
          type="button"
          class:file={i === crumbs.length - 1}
          title="Reveal {crumb.path} in the file tree"
          onclick={reveal}>{crumb.name}</button
        >
        {#if i === crumbs.length - 1}{@render children?.()}{/if}
      </li>
    {/each}
    {#each trail as symbol}
      <li class="symbol" title={symbol.kind}>{symbol.name}</li>
    {/each}
  </ol>
</nav>

<style>
  .crumbs {
    min-width: 0;
    overflow: hidden;
    font-size: var(--fs-ui-sm);
  }
  ol {
    display: flex;
    align-items: center;
    margin: 0;
    padding: 0;
    list-style: none;
    white-space: nowrap;
  }
  li {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  li + li::before {
    content: "›";
    padding: 0 4px;
    color: var(--text-3);
  }
  button {
    min-height: var(--ctl-sm);
    padding: 0 2px;
    font: inherit;
    color: var(--text-2);
    background: none;
    border: none;
    border-radius: var(--r-control);
    cursor: pointer;
  }
  button:hover {
    color: var(--text);
  }
  .file {
    color: var(--text);
  }
  .symbol {
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text-2);
  }
</style>
