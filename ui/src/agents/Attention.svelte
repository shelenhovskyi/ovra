<script lang="ts">
// The Agents header's "things to check": a small warning count, shown only
// when something would get in the way of a task (the setup check found a
// problem on this computer) or a decision waits (the project brings its own
// Claude settings, not trusted yet). Its menu names each and leads to it:
// Settings > Setup check, or the trust card with its warnings and lists.
import { tick, untrack } from "svelte";
import { openSettings } from "../settings/commands";
import { checkRows } from "../settings/setup-check";
import { useOvra } from "../shell/context";
import Icon from "../shell/Icon.svelte";
import { trustWaiting } from "./setup/trust-state.svelte";

const { transport, workspace } = useOvra();

let problems = $state(0);
let open = $state(false);
let toggle = $state<HTMLButtonElement>();
let menu = $state<HTMLElement>();
/** Where the menu opens: under the button, its right edge on the button's. */
let at = $state({ top: 0, right: 0 });

$effect(() => {
  const project = workspace.project?.id ?? null;
  untrack(() => {
    transport
      .call({ method: "setup.check", params: { project } })
      .then(({ data }) => (problems = checkRows(data).filter((row) => row.level === "problem").length))
      // A check that could not run says nothing here; Settings > Setup check says why.
      .catch(() => (problems = 0));
  });
});

const count = $derived((problems > 0 ? 1 : 0) + (trustWaiting.waiting ? 1 : 0));
const label = $derived(count === 1 ? "1 thing to check" : `${count} things to check`);

async function show(): Promise<void> {
  const box = toggle?.getBoundingClientRect();
  if (box) at = { top: box.bottom + 4, right: window.innerWidth - box.right };
  open = true;
  await tick();
  menu?.querySelector<HTMLElement>("[role=menuitem]")?.focus();
}

function close(refocus: boolean): void {
  open = false;
  if (refocus) toggle?.focus();
}

function pick(run: () => void): void {
  close(false);
  run();
}

function onkeydown(event: KeyboardEvent): void {
  const items = [...(menu?.querySelectorAll<HTMLElement>("[role=menuitem]") ?? [])];
  const at = items.indexOf(document.activeElement as HTMLElement);
  if (event.code === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    close(true);
  } else if (event.code === "ArrowDown" || event.code === "ArrowUp") {
    event.preventDefault();
    const step = event.code === "ArrowDown" ? 1 : -1;
    items[(at + step + items.length) % items.length]?.focus();
  }
}
</script>

{#if count > 0}
  <button
    type="button"
    class="badge"
    aria-haspopup="menu"
    aria-expanded={open}
    aria-label={label}
    title={label}
    data-testid="agents-attention"
    bind:this={toggle}
    onclick={() => (open ? close(false) : void show())}
    ><Icon name="warning" small /><span aria-hidden="true">{count}</span></button
  >
  {#if open}
    <div
      class="menu"
      role="menu"
      tabindex="-1"
      aria-label="Things to check"
      style:top="{at.top}px"
      style:right="{at.right}px"
      bind:this={menu}
      {onkeydown}
      onfocusout={(event) => {
        const to = event.relatedTarget;
        if (!(to instanceof Node && (menu?.contains(to) || to === toggle))) close(false);
      }}
    >
      {#if problems > 0}
        <button type="button" role="menuitem" data-testid="attention-setup" onclick={() => pick(() => openSettings("setup"))}>
          <Icon name="warning" small />
          <span class="text">
            <span class="name">Setup check found {problems === 1 ? "a problem" : `${problems} problems`}</span>
            <span class="what">Check setup</span>
          </span>
        </button>
      {/if}
      {#if trustWaiting.waiting}
        <button
          type="button"
          role="menuitem"
          data-testid="attention-trust"
          onclick={() => pick(() => trustWaiting.open?.())}
        >
          <Icon name="shield-alert" small />
          <span class="text">
            <span class="name">{trustWaiting.changed ? "Project settings changed" : "Project settings are off"}</span>
            <span class="what"
              >{trustWaiting.warnings > 0
                ? `${trustWaiting.warnings} ${trustWaiting.warnings === 1 ? "warning" : "warnings"} · `
                : ""}Review and trust…</span
            >
          </span>
        </button>
      {/if}
    </div>
  {/if}
{/if}

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: var(--ctl-sm);
    padding: 0 var(--sp-2);
    font: var(--t-caption);
    font-variant-numeric: tabular-nums;
    color: var(--warn);
    background: var(--warn-soft);
    border: 0;
    border-radius: var(--r-pill);
  }
  .badge:hover,
  .badge[aria-expanded="true"] {
    filter: brightness(1.1);
  }
  .menu {
    position: fixed;
    z-index: 20;
    width: max-content;
    max-width: 280px;
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: var(--s4);
    border-radius: var(--r-lg);
    box-shadow: var(--card-shadow);
  }
  .menu button {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    padding: 6px 10px;
    font: var(--t-small);
    color: var(--text);
    text-align: left;
    background: none;
    border: 0;
    border-radius: var(--r-sm);
  }
  .menu button :global(.icon) {
    flex: none;
    margin-top: 1px;
    color: var(--warn);
  }
  .menu button:hover,
  .menu button:focus-visible {
    background: var(--s3);
    outline: none;
  }
  .text {
    display: flex;
    flex-direction: column;
  }
  .name {
    font-weight: 500;
  }
  .what {
    color: var(--accent);
  }
  @media (forced-colors: active) {
    .badge,
    .menu {
      border: 1px solid CanvasText;
    }
  }
</style>
