<script lang="ts">
// The rail's "More" menu: names only (Settings…, Audit log, Skills and instructions,
// Agent browser, Keyboard shortcuts), each with its shortcut and, as a tooltip, what it is for. An item runs
// the command of the same name, so the menu, the palette and the shortcuts open the
// same thing; the rest is in the palette and Settings. Arrow keys, Home, End and a
// first letter move between items; Escape closes and gives focus back to the button.
import { tick } from "svelte";
import { sessionBrowsers } from "../browser/browser.svelte";
import { type Command, listCommands } from "../keys/commands";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import { useOvra } from "./context";
import Icon from "./Icon.svelte";
import { menuTarget } from "./menu";

/** `name` where the menu says it shorter than the palette ("Open settings"). */
const ITEMS: { command: string; id: string; about: string; name?: string }[] = [
  { command: "settings.open", id: "settings", name: "Settings…", about: "Rules, network, agents and more" },
  { command: "audit.toggle", id: "audit", about: "Everything agents did, and who approved it" },
  {
    command: "catalog.toggle",
    id: "catalog",
    about: "The skills, instructions and commands this project gives agents",
  },
  { command: "browser.toggle", id: "browser", about: "The web browser an agent uses" },
  { command: "palette.shortcuts", id: "shortcuts", about: "Every command with a shortcut" },
];

const { transport } = useOvra();
const browsers = sessionBrowsers(transport);
const waiting = $derived(browsers.asks.length);

let open = $state(false);
let button = $state<HTMLButtonElement>();
let menu = $state<HTMLElement>();
/** Where the menu sits: beside the button, towards the window's middle. */
let place = $state("");
/** The panels' commands, read when the menu opens: their names and shortcuts. */
let commands = $state<Command[]>([]);

const entries = $derived(
  ITEMS.flatMap((item) => {
    const command = commands.find((c) => c.id === item.command);
    if (!command) return [];
    const keys = command.keys ? liveKeys(command.keys, inBrowserTab()) : undefined;
    return [
      { ...item, name: item.name ?? command.title, keys, shown: keys ? formatShortcut(keys, isApple()) : undefined },
    ];
  }),
);

function items(): HTMLElement[] {
  return [...(menu?.querySelectorAll<HTMLElement>("[role^=menuitem]") ?? [])];
}

async function show(focus: "first" | "last"): Promise<void> {
  const at = button?.getBoundingClientRect();
  place = at ? placeBeside(at) : "";
  commands = listCommands();
  open = true;
  await tick();
  const all = items();
  (focus === "first" ? all[0] : all[all.length - 1])?.focus();
}

function placeBeside(at: DOMRect): string {
  const across =
    at.left < window.innerWidth / 2
      ? `left: ${at.right + 8}px;`
      : `right: ${Math.max(0, window.innerWidth - at.left + 8)}px;`;
  const down =
    at.top < window.innerHeight / 2 ? `top: ${at.top}px;` : `bottom: ${Math.max(0, window.innerHeight - at.bottom)}px;`;
  return `${across} ${down}`;
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
  <!-- Styled by the rail it sits in (`rail-button`, `tip`). -->
  <button
    type="button"
    id="more-button"
    class="rail-button tip"
    data-tip="More"
    data-testid="more"
    aria-haspopup="menu"
    aria-expanded={open}
    aria-controls={open ? "more-menu" : undefined}
    aria-label={waiting > 0 ? `More, ${waiting} waiting for you` : "More"}
    bind:this={button}
    onclick={() => (open ? close(false) : void show("first"))}
    onkeydown={onButtonKeydown}
  >
    <Icon name="more" />
    {#if waiting > 0}<span class="rail-badge warn" aria-hidden="true">{waiting}</span>{/if}
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
          title={item.about}
          onclick={() => choose(item.command)}
        >
          {item.name}
          {#if browser && waiting > 0}<span class="badge" aria-hidden="true">{waiting}</span>{/if}
          {#if item.shown}<kbd aria-hidden="true">{item.shown}</kbd>{/if}
          <span class="visually-hidden" id={`more-about-${item.id}`}>{item.about}</span>
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
    color: var(--text);
    cursor: default;
  }
  .menu {
    position: fixed;
    z-index: 100;
    min-width: 240px;
    padding: 6px;
    background: var(--menu);
    backdrop-filter: var(--menu-blur);
    border-radius: var(--r-lg);
    box-shadow: var(--card-shadow);
  }
  .menu button {
    display: flex;
    align-items: center;
    width: 100%;
    padding: 7px 10px;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
  }
  .menu button:hover,
  .menu button:focus {
    background: var(--s3);
  }
  kbd {
    margin-left: auto;
    padding-left: 16px;
    font: 500 12px var(--font);
    color: var(--text-2);
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .badge {
    margin-left: 4px;
    min-width: 16px;
    padding: 0 4px;
    font: 600 12px / 16px var(--font);
    text-align: center;
    color: var(--accent-ink);
    background: var(--warn);
    border-radius: 8px;
  }
  .more > button .rail-badge.warn {
    background: var(--warn);
  }

  @media (forced-colors: active) {
    .menu {
      border: 1px solid CanvasText;
    }
  }
</style>
