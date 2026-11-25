<script lang="ts">
// The header's "More" menu: the panels a newcomer rarely needs, each with a
// line on what it is for and its shortcut. An item runs the panel's own
// command and carries its name, so the menu, the palette and the shortcuts
// open the same dialog under the same name. Arrow keys, Home, End and a first
// letter move between items; Escape closes and gives focus back to the button.
import { tick } from "svelte";
import { sessionBrowsers } from "../browser/browser.svelte";
import { type Command, listCommands } from "../keys/commands";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import { useOvra } from "./context";
import { menuTarget } from "./menu";

const ITEMS = [
  { command: "audit.toggle", id: "audit", about: "Everything agents did, and who approved it" },
  { command: "policy.toggle", id: "policy", about: "Rules for what agents may do in this project" },
  { command: "network.toggle", id: "network", about: "Which websites agents may reach, and secrets" },
  { command: "servers.toggle", id: "servers", about: "Your project's dev servers, run for agents" },
  { command: "browser.toggle", id: "browser", about: "The web browser an agent uses" },
] as const;

const { transport } = useOvra();
const browsers = sessionBrowsers(transport);
const waiting = $derived(browsers.asks.length);

let open = $state(false);
let button = $state<HTMLButtonElement>();
let menu = $state<HTMLElement>();
/** Where the menu sits: under the button, right edges aligned (the header clips what overflows it). */
let place = $state("");
/** The panels' commands, read when the menu opens: their names and shortcuts. */
let commands = $state<Command[]>([]);

const entries = $derived(
  ITEMS.flatMap((item) => {
    const command = commands.find((c) => c.id === item.command);
    if (!command) return [];
    const keys = command.keys ? liveKeys(command.keys, inBrowserTab()) : undefined;
    return [{ ...item, name: command.title, keys, shown: keys ? formatShortcut(keys, isApple()) : undefined }];
  }),
);

function items(): HTMLElement[] {
  return [...(menu?.querySelectorAll<HTMLElement>("[role^=menuitem]") ?? [])];
}

async function show(focus: "first" | "last"): Promise<void> {
  const at = button?.getBoundingClientRect();
  place = at ? `top: ${at.bottom + 4}px; right: ${Math.max(0, window.innerWidth - at.right)}px;` : "";
  commands = listCommands();
  open = true;
  await tick();
  const all = items();
  (focus === "first" ? all[0] : all[all.length - 1])?.focus();
}

function close(refocus: boolean): void {
  open = false;
  if (refocus) button?.focus();
}

function choose(command: string): void {
  // Focus goes back to the button first, so the dialog returns it there when it closes.
  close(true);
  void listCommands()
    .find((c) => c.id === command)
    ?.run();
}

function onButtonKeydown(event: KeyboardEvent): void {
  if (event.code === "ArrowDown" || event.code === "ArrowUp") {
    event.preventDefault();
    void show(event.code === "ArrowDown" ? "first" : "last");
  }
}

function onMenuKeydown(event: KeyboardEvent): void {
  if (event.code === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    close(true);
    return;
  }
  if (event.code === "Tab") {
    close(false);
    return;
  }
  const next = menuTarget(event, items());
  if (!next) return;
  event.preventDefault();
  next.focus();
}

// A press anywhere outside the menu and its button closes it.
$effect(() => {
  if (!open) return;
  const onPointerDown = (event: PointerEvent) => {
    const target = event.target as Node | null;
    if (!menu?.contains(target) && !button?.contains(target)) close(false);
  };
  document.addEventListener("pointerdown", onPointerDown, true);
  return () => document.removeEventListener("pointerdown", onPointerDown, true);
});
</script>

<div class="more">
  <button
    type="button"
    id="more-button"
    data-testid="more"
    aria-haspopup="menu"
    aria-expanded={open}
    aria-controls={open ? "more-menu" : undefined}
    aria-label={waiting > 0 ? `More, ${waiting} waiting for you` : "More"}
    bind:this={button}
    onclick={() => (open ? close(false) : void show("first"))}
    onkeydown={onButtonKeydown}
  >
    More
    {#if waiting > 0}<span class="badge" aria-hidden="true">{waiting}</span>{/if}
  </button>
  {#if open}
    <div id="more-menu" class="menu" role="menu" aria-labelledby="more-button" tabindex="-1" style={place} bind:this={menu} onkeydown={onMenuKeydown}>
      {#each entries as item (item.command)}
        {@const browser = item.command === "browser.toggle"}
        <button
          type="button"
          role={browser ? "menuitemcheckbox" : "menuitem"}
          aria-checked={browser ? browsers.shown : undefined}
          tabindex="-1"
          data-testid={`open-${item.id}`}
          aria-label={browser && waiting > 0 ? `${item.name}, ${waiting} waiting for you` : item.name}
          aria-describedby={`more-about-${item.id}`}
          aria-keyshortcuts={item.keys}
          onclick={() => choose(item.command)}
        >
          <span class="name">
            {item.name}
            {#if browser && waiting > 0}<span class="badge" aria-hidden="true">{waiting}</span>{/if}
            {#if item.shown}<kbd aria-hidden="true">{item.shown}</kbd>{/if}
          </span>
          <span class="about" id={`more-about-${item.id}`}>{item.about}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .more {
    position: relative;
  }
  button {
    font: inherit;
    color: #e6e6e6;
    cursor: pointer;
  }
  .more > button {
    padding: 2px 10px;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
  }
  button:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  .menu {
    position: fixed;
    z-index: 100;
    min-width: 280px;
    padding: 4px;
    background: #1e1e24;
    border: 1px solid #4a4a57;
    border-radius: 4px;
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.5);
  }
  .menu button {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    width: 100%;
    padding: 6px 8px;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: 3px;
  }
  .menu button:hover,
  .menu button:focus {
    background: #2a2a33;
  }
  .name {
    display: flex;
    align-items: center;
    width: 100%;
  }
  kbd {
    margin-left: auto;
    padding-left: 16px;
    font: inherit;
    font-size: 12px;
    color: #9a9aa6;
  }
  .about {
    font-size: 12px;
    color: #9a9aa6;
  }
  .badge {
    margin-left: 4px;
    padding: 0 5px;
    font-size: 11px;
    color: #101013;
    background: #ffd36b;
    border-radius: 8px;
  }
</style>
