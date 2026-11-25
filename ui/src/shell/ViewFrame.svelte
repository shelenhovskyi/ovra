<script lang="ts">
// The frame of a main-area view (main-view.svelte.ts): its title, what the
// view puts beside it, and the view's content, which scrolls on its own.
// The view's tab above it (in the shell) has the close button. The title
// takes focus when the view opens; Escape closes it, unless something
// inside used the key first.
import type { Snippet } from "svelte";
import { type MainViewName, mainView, VIEW_TITLES } from "./main-view.svelte";

let { name, testid, tools, children }: { name: MainViewName; testid?: string; tools?: Snippet; children: Snippet } =
  $props();

const id = $props.id();

function focusTitle(title: HTMLElement): void {
  // A view opened from a menu or a shortcut: the person reads it from the top.
  if (!title.closest("section")?.contains(document.activeElement)) title.focus();
}

function onkeydown(event: KeyboardEvent): void {
  if (event.code !== "Escape" || event.defaultPrevented) return;
  event.preventDefault();
  mainView.close();
}
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="view-frame" aria-labelledby={`${id}-title`} data-testid={testid} {onkeydown}>
  <header>
    <h2 id={`${id}-title`} tabindex="-1" {@attach focusTitle}>{VIEW_TITLES[name]}</h2>
    {@render tools?.()}
  </header>
  <div class="content">{@render children()}</div>
</section>

<style>
  .view-frame {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--s2);
  }
  header {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--sp-3);
    min-height: 44px;
    padding: var(--sp-1) var(--sp-2) 0 var(--sp-5);
  }
  h2 {
    margin: 0;
    font: var(--t-title2);
    letter-spacing: -0.02em;
  }
  .content {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    overflow: auto;
  }
</style>
