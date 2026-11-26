<script lang="ts" module>
import type { AssistAction } from "./assist";

/** Menu items and palette titles of the agent actions. */
export const ACTION_LABEL: Record<AssistAction, string> = {
  commits: "Suggest commits",
  message: "Write commit message",
  pulled: "Summarize what's new",
  conflicts: "Explain conflicts",
};

/** One line on what each action does, for its tooltip. */
const ACTION_HINT: Record<AssistAction, string> = {
  commits: "An agent splits your changes into commits and writes their messages. You commit them.",
  message: "An agent writes a message for what is staged.",
  pulled: "An agent sums up the commits that came with the last pull, merge or rebase.",
  conflicts: "An agent reads the conflicted files and explains them.",
};
</script>

<script lang="ts">
// The git panel's one quiet button for agent actions, with a short menu of
// the actions that apply now. Up and Down move in the menu, Escape closes it.
import { tick } from "svelte";
import type { RepoStatus } from "../lib/protocol";
import Icon from "../shell/Icon.svelte";
import { Assist } from "./assist.svelte";

let { assist, status }: { assist: Assist; status: RepoStatus } = $props();

let open = $state(false);
let toggle = $state<HTMLElement>();
let menu = $state<HTMLElement>();

const actions = $derived(Assist.available(status));

async function show(): Promise<void> {
  if (assist.busy) return;
  open = !open;
  if (!open) return;
  await tick();
  menu?.querySelector<HTMLElement>("button")?.focus();
}

async function close(): Promise<void> {
  open = false;
  await tick();
  toggle?.focus();
}

function choose(action: AssistAction): void {
  open = false;
  // The item goes away: focus stays on the button, then the result takes it.
  toggle?.focus();
  void assist.run(action);
}

/** Down or Up on the closed button opens the menu, as in a native menu button. */
function ontogglekey(event: KeyboardEvent): void {
  if (open || (event.code !== "ArrowDown" && event.code !== "ArrowUp")) return;
  event.preventDefault();
  void show();
}

function onkeydown(event: KeyboardEvent): void {
  const items = [...(menu?.querySelectorAll<HTMLElement>("button") ?? [])];
  const at = items.indexOf(document.activeElement as HTMLElement);
  if (event.code === "Escape") {
    event.preventDefault();
    void close();
  } else if (event.code === "Tab") {
    // Tab leaves the menu and goes on from the button.
    open = false;
  } else if (event.code === "ArrowDown" || event.code === "ArrowUp") {
    event.preventDefault();
    const step = event.code === "ArrowDown" ? 1 : -1;
    items[(at + step + items.length) % items.length]?.focus();
  } else if (event.code === "Home" || event.code === "End") {
    event.preventDefault();
    (event.code === "Home" ? items[0] : items.at(-1))?.focus();
  }
}

function onfocusout(event: FocusEvent): void {
  const to = event.relatedTarget as Node | null;
  if (open && to && !menu?.contains(to) && to !== toggle) open = false;
}
</script>

{#if actions.length}
  <div class="actions" {onfocusout}>
    <button
      type="button"
      class="toggle"
      data-testid="git-assist"
      aria-label="Agent actions"
      title={assist.busy ? "An agent is working on it" : "Agent actions"}
      aria-haspopup="menu"
      aria-expanded={open}
      aria-disabled={assist.busy}
      bind:this={toggle}
      onkeydown={ontogglekey}
      onclick={show}><Icon name="sparkles" small /></button
    >
    {#if open}
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div class="menu" role="menu" aria-label="Agent actions" tabindex="-1" bind:this={menu} {onkeydown}>
        {#each actions as action (action)}
          <button
            type="button"
            role="menuitem"
            title={ACTION_HINT[action]}
            data-testid={`git-assist-${action}`}
            onclick={() => choose(action)}>{ACTION_LABEL[action]}</button
          >
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .actions {
    position: relative;
    flex: none;
  }
  button {
    font: inherit;
    color: var(--text-2);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    cursor: default;
  }
  button:hover {
    background: var(--s3);
    color: var(--text);
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .toggle {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
  }
  .toggle[aria-disabled="true"] {
    opacity: 0.6;
  }
  .menu {
    position: absolute;
    right: 0;
    top: 30px;
    z-index: 5;
    display: flex;
    flex-direction: column;
    width: max-content;
    max-width: min(260px, calc(100vw - 32px));
    padding: 6px;
    background: var(--menu, var(--s2));
    border-radius: var(--r-lg, var(--radius));
    box-shadow: var(--card-shadow, none);
  }
  .menu button {
    min-height: 26px;
    padding: 0 8px;
    font-size: 13px;
    text-align: left;
    color: var(--text);
    white-space: normal;
  }
  .menu button:hover,
  .menu button:focus-visible {
    background: var(--accent);
    color: var(--accent-ink, white);
    outline: none;
  }
  @media (forced-colors: active) {
    .menu {
      border: 1px solid CanvasText;
    }
    .menu button:hover,
    .menu button:focus-visible {
      outline: 2px solid Highlight;
      outline-offset: -2px;
    }
  }
</style>
