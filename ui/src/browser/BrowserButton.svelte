<script lang="ts">
// The header button (and the "Agent browser" command) that shows or hides the
// browser pane. It counts the navigations waiting for an answer, so a hidden
// pane does not keep an agent waiting unseen.
import { registerCommand } from "../keys/commands";
import { useOvra } from "../shell/context";
import { sessionBrowsers } from "./browser.svelte";

const { transport } = useOvra();
const browsers = sessionBrowsers(transport);

let button = $state<HTMLButtonElement>();
let returnTo: HTMLElement | null = null;
let wasShown = false;
const waiting = $derived(browsers.asks.length);

$effect(() => registerCommand({ id: "browser.toggle", title: "Agent browser", keys: "Mod+Shift+B", run: toggle }));

// Hiding the pane gives focus back to where it was before, if focus went with the pane.
$effect(() => {
  const shown = browsers.shown;
  const hidden = wasShown && !shown;
  wasShown = shown;
  if (!hidden) return;
  const back = returnTo;
  returnTo = null;
  if (document.activeElement === document.body || !document.activeElement) (back?.isConnected ? back : button)?.focus();
});

function toggle(): void {
  if (!browsers.shown) {
    const active = document.activeElement;
    returnTo = active instanceof HTMLElement && active !== document.body ? active : null;
  }
  browsers.shown = !browsers.shown;
}
</script>

<button
  type="button"
  class="open-browser"
  data-testid="open-browser"
  aria-expanded={browsers.shown}
  aria-label={waiting > 0 ? `Browser, ${waiting} waiting for you` : "Browser"}
  bind:this={button}
  onclick={toggle}
>
  Browser
  {#if waiting > 0}<span class="badge" aria-hidden="true">{waiting}</span>{/if}
</button>

<style>
  button {
    font: inherit;
    padding: 2px 10px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    cursor: pointer;
  }
  button:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
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
