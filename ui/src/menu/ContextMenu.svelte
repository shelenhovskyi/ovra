<script lang="ts">
// The window's context menu, mounted once by the shell; `openContextMenu`
// (context-menu.svelte.ts) fills it. It opens at the asked point, kept inside
// the viewport. Arrow keys, Home, End and a first letter move between items,
// Enter or Space runs one; Escape, Tab or a press outside closes it and gives
// focus back to where it was.
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import { menuTarget } from "../shell/menu";
import { clampToViewport, closeContextMenu, contextMenu, type MenuItem } from "./context-menu.svelte";

let menu = $state<HTMLElement>();
let place = $state<{ x: number; y: number } | null>(null);
const open = $derived(contextMenu.open);

function items(): HTMLElement[] {
  return [...(menu?.querySelectorAll<HTMLElement>("[role=menuitem]") ?? [])];
}

// Each opening: keep the menu inside the window and focus its first item.
$effect(() => {
  const current = contextMenu.open;
  if (!current || !menu) {
    place = null;
    return;
  }
  const box = menu.getBoundingClientRect();
  place = clampToViewport(current.x, current.y, box, { width: window.innerWidth, height: window.innerHeight });
  (items().find((item) => item.getAttribute("aria-disabled") !== "true") ?? menu).focus();
});

// A press outside closes it; so does leaving the window or resizing it.
$effect(() => {
  if (!open) return;
  const onPointerDown = (event: PointerEvent) => {
    if (!menu?.contains(event.target as Node | null)) closeContextMenu(true);
  };
  const away = () => closeContextMenu(false);
  document.addEventListener("pointerdown", onPointerDown, true);
  window.addEventListener("blur", away);
  window.addEventListener("resize", away);
  return () => {
    document.removeEventListener("pointerdown", onPointerDown, true);
    window.removeEventListener("blur", away);
    window.removeEventListener("resize", away);
  };
});

function choose(item: MenuItem): void {
  if (item.disabled) return;
  // Focus goes back first, so what the item opens (a search field, a dialog) can take it.
  closeContextMenu(true);
  void (async () => {
    try {
      await item.run();
    } catch (error) {
      console.error(`menu item ${item.label} failed`, error);
    }
  })();
}

function onKeydown(event: KeyboardEvent): void {
  // The menu has the keyboard while it is open: no shortcut runs behind it.
  event.stopPropagation();
  if (event.code === "Escape" || event.code === "Tab") {
    event.preventDefault();
    closeContextMenu(true);
    return;
  }
  const next = menuTarget(event, items());
  if (!next) return;
  event.preventDefault();
  next.focus();
}

function shown(keys: string): string {
  return formatShortcut(liveKeys(keys, inBrowserTab()), isApple());
}
</script>

{#if open}
  <div
    class="menu"
    role="menu"
    tabindex="-1"
    data-testid="context-menu"
    style:left={`${place?.x ?? open.x}px`}
    style:top={`${place?.y ?? open.y}px`}
    bind:this={menu}
    onkeydown={onKeydown}
    oncontextmenu={(event) => event.preventDefault()}
  >
    {#each open.entries as entry, index (index)}
      {#if entry === "separator"}
        <div class="separator" role="separator"></div>
      {:else}
        <button
          type="button"
          role="menuitem"
          tabindex="-1"
          class:danger={entry.danger}
          aria-disabled={entry.disabled ? "true" : undefined}
          aria-keyshortcuts={entry.keys ? liveKeys(entry.keys, inBrowserTab()) : undefined}
          onclick={() => choose(entry)}
        >
          {entry.label}
          {#if entry.keys}<kbd aria-hidden="true">{shown(entry.keys)}</kbd>{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  /* docs/design/components.md, "Menus, context menus, tooltips". */
  .menu {
    position: fixed;
    z-index: 150;
    min-width: 200px;
    max-width: min(360px, calc(100vw - 8px));
    max-height: calc(100vh - 8px);
    overflow-y: auto;
    padding: 6px;
    background: var(--s4);
    border-radius: var(--radius-lg);
    box-shadow: var(--card-shadow);
  }
  .menu:focus {
    outline: none;
  }
  button {
    display: flex;
    align-items: center;
    width: 100%;
    min-height: 26px;
    padding: 0 10px;
    font: 13px / 18px var(--font);
    color: var(--text);
    text-align: left;
    white-space: nowrap;
    background: transparent;
    border: 0;
    border-radius: var(--radius-sm);
    cursor: default;
  }
  button:focus {
    outline: none;
  }
  .danger {
    color: var(--danger);
  }
  /* A right-click focuses the first item without marking it; keys mark the focused one. */
  button:hover,
  button:focus-visible {
    color: var(--accent-ink);
    background: var(--accent);
  }
  .danger:hover,
  .danger:focus-visible {
    background: var(--danger);
  }
  button[aria-disabled="true"] {
    color: var(--text-3);
    background: transparent;
  }
  kbd {
    margin-left: auto;
    padding-left: 24px;
    font: 12px var(--font);
    color: var(--text-2);
  }
  button:hover:not([aria-disabled="true"]) kbd,
  button:focus-visible:not([aria-disabled="true"]) kbd {
    color: inherit;
  }
  .separator {
    height: 1px;
    margin: 4px 6px;
    background: var(--s3);
  }
  @media (forced-colors: active) {
    .menu {
      border: 1px solid CanvasText;
    }
    button:focus-visible {
      outline: 2px solid Highlight;
    }
  }
</style>
