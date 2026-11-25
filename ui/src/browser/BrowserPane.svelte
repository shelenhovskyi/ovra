<script lang="ts">
// The agent browser in the main area, while shown (see BrowserButton):
// one session's pages as tabs, an address field that opens a page, the page
// itself, and the navigations agents wait for the person to allow.
import { tick, untrack } from "svelte";
import { agentSessions } from "../agents/sessions.svelte";
import { isEnded } from "../agents/status";
import type { PageId, PageInfo, SessionId } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";
import { type NavigationAsk, sessionBrowsers } from "./browser.svelte";
import PageView from "./PageView.svelte";

const { transport, workspace } = useOvra();
const browsers = sessionBrowsers(transport);
const sessions = agentSessions(transport);

let chosen = $state<SessionId | null>(null);
let activePage = $state<PageId | null>(null);
let address = $state("");
let error = $state<string | null>(null);
let heading = $state<HTMLElement>();
let pane = $state<HTMLElement>();
const tabButtons: Record<PageId, HTMLButtonElement> = $state({});

const project = $derived(workspace.project?.id ?? null);
/** Sessions that can open pages, and ended ones whose pages are still listed. */
const list = $derived(
  project ? sessions.grouped(project).filter((s) => !isEnded(s.status) || browsers.of(s.id).length > 0) : [],
);
const session = $derived(
  list.find((s) => s.id === chosen) ??
    [...list].reverse().find((s) => browsers.of(s.id).length > 0) ??
    list.at(-1) ??
    null,
);
const pages = $derived(session ? browsers.of(session.id) : []);
const page = $derived(pages.find((p) => p.id === activePage) ?? pages[0] ?? null);
/** Why the shown session's browser cannot run, as the core said it, made a sentence. */
const refusal = $derived.by(() => {
  const refused = browsers.unavailable;
  if (!refused || refused.session !== session?.id) return null;
  const reason = refused.reason.trim();
  return `${reason.charAt(0).toUpperCase()}${reason.slice(1)}${/[.!?]$/.test(reason) ? "" : "."}`;
});
const asks = $derived(browsers.asks.filter((a) => list.some((s) => s.id === a.session)));

$effect(() => {
  if (browsers.shown && project) untrack(() => sessions.load(project).catch(() => {}));
});

// Showing the pane moves focus to it: to the first ask waiting, if any.
$effect(() => {
  if (browsers.shown) void tick().then(() => (allowOf(0) ?? heading)?.focus());
});

/** The Allow button of the `at`th ask shown. */
function allowOf(at: number): HTMLButtonElement | null {
  return pane?.querySelectorAll<HTMLButtonElement>("[data-testid=browser-ask] button")[at * 2] ?? null;
}

/** Whether focus is in the pane, or nowhere, but not on the page itself. */
function focusIsFree(): boolean {
  const active = document.activeElement;
  if (!active || active === document.body) return true;
  return !!pane?.contains(active) && !active.closest("[data-testid=browser-stage]");
}

// A new ask gets focus, unless the person is busy elsewhere.
let asked = 0;
$effect(() => {
  const count = asks.length;
  const grew = count > asked;
  asked = count;
  if (grew && browsers.shown) void tick().then(() => focusIsFree() && allowOf(count - 1)?.focus());
});

// A refusal is about one session: showing another one forgets it, so coming back tries again.
$effect(() => {
  const shown = session?.id ?? null;
  untrack(() => {
    if (browsers.unavailable && browsers.unavailable.session !== shown) browsers.unavailable = null;
  });
});

// The field shows the address of the page on screen.
$effect(() => {
  const url = page?.url ?? "";
  untrack(() => (address = url));
});

function text(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}

async function run(action: () => Promise<void>): Promise<void> {
  error = null;
  try {
    await action();
  } catch (err) {
    error = text(err);
  }
}

/** `localhost:5173` means `http://localhost:5173`. */
function withScheme(url: string): string {
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(url) || url.startsWith("about:") ? url : `http://${url}`;
}

function open(): Promise<void> {
  const target = session;
  const url = address.trim();
  if (!target || !url) return Promise.resolve();
  return run(async () => {
    const opened = await browsers.open(target.id, withScheme(url));
    if (opened) activePage = opened.id;
  });
}

function close(closing: PageInfo): Promise<void> {
  return run(() => browsers.close(closing));
}

async function answer(ask: NavigationAsk, allow: boolean): Promise<void> {
  await run(() => browsers.answer(ask, allow));
  await tick();
  // The answered ask is gone: go on to the next one, else to the page's tab or the address.
  if (pane?.contains(document.activeElement)) return;
  (
    allowOf(0) ??
    (page && tabButtons[page.id]) ??
    pane?.querySelector<HTMLElement>("#browser-address") ??
    heading
  )?.focus();
}

async function select(target: PageInfo): Promise<void> {
  activePage = target.id;
  await tick();
  tabButtons[target.id]?.focus();
}

function onTabKeydown(event: KeyboardEvent, at: number): void {
  const step = event.code === "ArrowRight" ? 1 : event.code === "ArrowLeft" ? -1 : 0;
  const target = pages[(at + step + pages.length) % pages.length];
  if (step === 0 || !target) return;
  event.preventDefault();
  void select(target);
}

function hide(): void {
  browsers.shown = false;
}

/** Escape hides the pane like the dialogs of the "More" menu, unless the page or a picker took it. */
function onPaneKeydown(event: KeyboardEvent): void {
  if (event.code !== "Escape" || event.defaultPrevented || event.isComposing) return;
  if (event.target instanceof Element && event.target.closest("[data-testid=browser-stage]")) return;
  event.preventDefault();
  event.stopPropagation();
  hide();
}
</script>

{#if browsers.shown}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <section
    class="browser"
    aria-labelledby="browser-title"
    data-testid="browser"
    bind:this={pane}
    onkeydown={onPaneKeydown}
  >
    <header>
      <h2 id="browser-title" tabindex="-1" bind:this={heading}>Browser</h2>
      {#if list.length > 0}
        <label>
          Session
          <select
            value={session?.id ?? null}
            onchange={(event) => {
              chosen = event.currentTarget.value;
              activePage = null;
            }}
            data-testid="browser-session"
          >
            {#each list as s (s.id)}
              <option value={s.id}>{sessions.name(s.id)}</option>
            {/each}
          </select>
        </label>
      {/if}
      <button type="button" class="close" aria-keyshortcuts="Escape" onclick={hide}>Hide</button>
    </header>

    {#each asks as ask (`${ask.page} ${ask.origin}`)}
      <div class="ask" role="group" aria-label={`Let the agent open ${ask.origin}?`} data-testid="browser-ask">
        <span>
          <span class="muted">{sessions.name(ask.session)}:</span>
          Let the agent open <strong>{ask.origin}</strong>?
        </span>
        <button type="button" onclick={() => void answer(ask, true)}>Allow</button>
        <button type="button" onclick={() => void answer(ask, false)}>Deny</button>
      </div>
    {/each}

    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}

    {#if refusal}
      <div class="warn" role="note" data-testid="browser-unavailable">
        <p>{refusal}</p>
        <button type="button" onclick={() => (browsers.unavailable = null)}>Try again</button>
      </div>
    {:else if !project}
      <p class="muted">Open a project to use the agent browser.</p>
    {:else if !session}
      <p class="muted">Each agent gets its own browser; its pages show here.</p>
    {:else}
      <form
        class="address"
        onsubmit={(event) => {
          event.preventDefault();
          void open();
        }}
      >
        <label for="browser-address">Address</label>
        <input id="browser-address" type="text" spellcheck="false" autocomplete="off" bind:value={address} />
        <button type="submit" disabled={!address.trim() || isEnded(session.status)}>Open in new tab</button>
      </form>
      {#if pages.length > 0}
        <div class="tabs" role="tablist" aria-label="Pages">
          {#each pages as p, i (p.id)}
            <span class="tab">
              <button
                type="button"
                role="tab"
                aria-selected={p.id === page?.id}
                tabindex={p.id === page?.id ? 0 : -1}
                title={p.url}
                bind:this={tabButtons[p.id]}
                onclick={() => void select(p)}
                onkeydown={(event) => onTabKeydown(event, i)}>{p.title || p.url}</button
              >
              <button type="button" class="x" aria-label={`Close ${p.title || p.url}`} onclick={() => void close(p)}
                >×</button
              >
            </span>
          {/each}
        </div>
      {/if}
      {#if page}
        {#key page.id}
          <div class="tabpanel" role="tabpanel" aria-label={page.title || page.url}>
            <PageView {page} />
          </div>
        {/key}
      {:else}
        <p class="muted">No pages open. The agent opens them, or open one above.</p>
      {/if}
    {/if}
  </section>
{/if}

<style>
  .browser {
    display: flex;
    flex-direction: column;
    gap: 6px;
    flex: 1 1 0;
    min-height: 0;
    padding: 6px 12px 8px;
    box-sizing: border-box;
    background: var(--s1);
  }
  header,
  .address,
  .ask {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }
  .close {
    margin-left: auto;
    color: var(--text-2);
    background: transparent;
  }
  label,
  .muted {
    color: var(--text-2);
    font-size: 13px;
  }
  .address input {
    flex: 1;
    min-width: 120px;
  }
  .ask {
    padding: 6px 10px;
    background: var(--warn-soft);
    border-radius: var(--radius);
  }
  .warn {
    color: var(--warn);
  }
  .warn p {
    margin: 0 0 6px;
  }
  .error {
    margin: 0;
    color: var(--danger);
  }
  .tabs {
    display: flex;
    gap: 2px;
    overflow-x: auto;
  }
  .tab {
    display: flex;
    flex: none;
  }
  .tab [role="tab"] {
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    border-bottom-color: transparent;
  }
  .tab [role="tab"] {
    color: var(--text-2);
    background: transparent;
  }
  .tab [aria-selected="true"] {
    color: var(--text);
    background: var(--s3);
    font-weight: 500;
  }
  .tab .x {
    color: var(--text-2);
    background: transparent;
  }
  .tabpanel {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  button,
  input,
  select {
    font: inherit;
    padding: 4px 10px;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--radius);
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    cursor: default;
    opacity: 0.6;
  }
  button:hover {
    background: var(--s4);
  }
  .ask button:first-of-type {
    color: var(--accent-ink);
    background: var(--accent);
  }
  @media (forced-colors: active) {
    button,
    input,
    select,
    .ask {
      border: 1px solid ButtonText;
    }
  }
</style>
