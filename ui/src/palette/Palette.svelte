<script lang="ts">
// Command palette: fuzzy search over the registered commands. The shell mounts
// it while `workspace.paletteOpen` is true; it closes by setting it to false.
import { listCommands } from "../keys/commands";
import { runCommand } from "../keys/keybindings";
import { formatShortcut, isApple } from "../keys/shortcut";
import { useOvra } from "../shell/context";
import { filterCommands, type Item, segments } from "./fuzzy";

const { workspace } = useOvra();
const commands = listCommands();
const apple = isApple();

let query = $state("");
let selected = $state(0);
let input: HTMLInputElement | undefined = $state();
let list: HTMLUListElement | undefined = $state();
const items = $derived(filterCommands(commands, query));

$effect(() => {
  const previous = document.activeElement;
  input?.focus();
  return () => {
    if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
  };
});

$effect(() => {
  list?.querySelector(`[data-index="${selected}"]`)?.scrollIntoView?.({ block: "nearest" });
});

function close(): void {
  workspace.paletteOpen = false;
}

function run(item: Item | undefined): void {
  if (!item) return;
  close();
  void runCommand(item.command);
}

function onkeydown(event: KeyboardEvent): void {
  const count = items.length;
  switch (event.key) {
    case "ArrowDown":
      if (count) selected = (selected + 1) % count;
      break;
    case "ArrowUp":
      if (count) selected = (selected - 1 + count) % count;
      break;
    case "Enter":
      run(items[selected]);
      break;
    case "Escape":
      close();
      break;
    default:
      return;
  }
  event.preventDefault();
}

function onfocusout(event: FocusEvent): void {
  const next = event.relatedTarget;
  if (!(next instanceof Node && event.currentTarget instanceof Node && event.currentTarget.contains(next))) close();
}
</script>

<div class="backdrop">
  <div class="palette" role="dialog" aria-modal="true" aria-label="Command palette" {onfocusout}>
    <input
      bind:this={input}
      bind:value={query}
      oninput={() => (selected = 0)}
      {onkeydown}
      type="text"
      role="combobox"
      aria-expanded="true"
      aria-controls="palette-list"
      aria-activedescendant={items.length ? `palette-item-${selected}` : undefined}
      aria-label="Command"
      placeholder="Type a command"
      spellcheck="false"
      autocomplete="off"
    />
    <ul id="palette-list" role="listbox" aria-label="Commands" bind:this={list}>
      {#each items as item, index (item.command.id)}
        <li
          id="palette-item-{index}"
          data-index={index}
          role="option"
          aria-selected={index === selected}
          onmousedown={(event) => event.preventDefault()}
          onmousemove={() => (selected = index)}
          onclick={() => run(item)}
          onkeydown={() => {}}
        >
          <span class="title">
            {#each segments(item.command.title, item.positions) as part}
              {#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}
            {/each}
          </span>
          {#if item.command.keys}<kbd>{formatShortcut(item.command.keys, apple)}</kbd>{/if}
        </li>
      {:else}
        <li class="empty" role="presentation">No matching commands</li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 12vh;
    background: rgb(0 0 0 / 0.35);
    z-index: 100;
  }
  .palette {
    width: min(600px, calc(100vw - 32px));
    background: #1e1e24;
    border: 1px solid #34343d;
    border-radius: 8px;
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.5);
    overflow: hidden;
  }
  input {
    box-sizing: border-box;
    width: 100%;
    padding: 10px 12px;
    border: 0;
    border-bottom: 1px solid #2a2a31;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 14px;
    outline: none;
  }
  ul {
    max-height: 50vh;
    margin: 0;
    padding: 4px 0;
    overflow-y: auto;
    list-style: none;
  }
  li {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 6px 12px;
    cursor: pointer;
  }
  li[aria-selected="true"] {
    background: #2d3a5c;
  }
  mark {
    background: none;
    color: #7aa2ff;
    font-weight: 600;
  }
  kbd {
    font: inherit;
    font-size: 11px;
    color: #9a9aa6;
  }
  .empty {
    color: #6f6f7a;
    cursor: default;
  }
</style>
