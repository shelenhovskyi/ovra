<script lang="ts">
// The header's "More" menu: the panels a newcomer rarely needs, each with a
// line on what it is for. An item runs the panel's own command, so the menu,
// the palette and the shortcuts open the same dialog. Arrow keys, Home and
// End move between items; Escape closes and gives focus back to the button.
import { tick } from "svelte";
import { sessionBrowsers } from "../browser/browser.svelte";
import { listCommands } from "../keys/commands";
import { useOvra } from "./context";

const ITEMS = [
  { command: "audit.toggle", name: "Audit log", about: "Everything agents did, and who approved it" },
  { command: "policy.toggle", name: "Policies", about: "Rules for what agents may do in this project" },
  { command: "network.toggle", name: "Network", about: "Which websites agents may reach, and secrets" },
  { command: "servers.toggle", name: "Servers", about: "Your project's dev servers, run for agents" },
  { command: "browser.toggle", name: "Browser", about: "The web browser an agent uses" },
] as const;

const { transport } = useOvra();
const browsers = sessionBrowsers(transport);
const waiting = $derived(browsers.asks.length);

let open = $state(false);
let button = $state<HTMLButtonElement>();
let menu = $state<HTMLElement>();
/** Where the menu sits: under the button, right edges aligned (the header clips what overflows it). */
let place = $state("");

function items(): HTMLElement[] {
  return [...(menu?.querySelectorAll<HTMLElement>("[role^=menuitem]") ?? [])];
}

async function show(focus: "first" | "last"): Promise<void> {
  const at = button?.getBoundingClientRect();
  place = at ? `top: ${at.bottom + 4}px; right: ${Math.max(0, window.innerWidth - at.right)}px;` : "";
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
  const all = items();
  const at = all.indexOf(document.activeElement as HTMLElement);
  let next: number | null = null;
  if (event.code === "ArrowDown") next = (at + 1) % all.length;
  else if (event.code === "ArrowUp") next = (at - 1 + all.length) % all.length;
  else if (event.code === "Home") next = 0;
  else if (event.code === "End") next = all.length - 1;
  else if (event.code === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    close(true);
    return;
  } else if (event.code === "Tab") {
    close(false);
    return;
  }
  if (next === null) return;
  event.preventDefault();
  all[next]?.focus();
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
      {#each ITEMS as item (item.command)}
        {@const browser = item.command === "browser.toggle"}
        <button
          type="button"
          role={browser ? "menuitemcheckbox" : "menuitem"}
          aria-checked={browser ? browsers.shown : undefined}
          tabindex="-1"
          aria-label={browser && waiting > 0 ? `${item.name}, ${waiting} waiting for you` : item.name}
          aria-describedby={`more-about-${item.name}`}
          onclick={() => choose(item.command)}
        >
          <span class="name">
            {item.name}
            {#if browser && waiting > 0}<span class="badge" aria-hidden="true">{waiting}</span>{/if}
          </span>
          <span class="about" id={`more-about-${item.name}`}>{item.about}</span>
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
