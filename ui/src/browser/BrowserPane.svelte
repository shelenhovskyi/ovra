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
const asks = $derived(browsers.asks.filter((a) => list.some((s) => s.id === a.session)));

$effect(() => {
  if (browsers.shown && project) untrack(() => sessions.load(project).catch(() => {}));
});

// Showing the pane moves focus to it.
$effect(() => {
  if (browsers.shown) void tick().then(() => heading?.focus());
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

function answer(ask: NavigationAsk, allow: boolean): Promise<void> {
  return run(() => browsers.answer(ask, allow));
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
</script>

{#if browsers.shown}
  <section class="browser" aria-labelledby="browser-title" data-testid="browser">
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
      <button type="button" class="close" onclick={hide}>Hide</button>
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

    {#if browsers.unavailable}
      <div class="warn" role="note" data-testid="browser-unavailable">
        <p>
          The agent browser cannot run here. Ovra uses a Chromium or Google Chrome installed on this computer
          (<code>chromium</code>, <code>chromium-browser</code> or <code>google-chrome</code> on <code>PATH</code>) and
          never downloads one. For now it also needs the Linux sandbox, which keeps the browser behind the session's
          network rules.
        </p>
        <p class="muted">The core said: {browsers.unavailable}</p>
        <button type="button" onclick={() => (browsers.unavailable = null)}>Try again</button>
      </div>
    {:else if !project}
      <p class="muted">Open a project to use the agent browser.</p>
    {:else if !session}
      <p class="muted">Start an agent session first: each session has its own browser.</p>
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
        <p class="muted">No pages open. The agent opens pages with its browser tools, or open one above.</p>
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
    border-top: 1px solid #2a2a31;
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
    font-size: 13px;
  }
  .close {
    margin-left: auto;
  }
  label,
  .muted {
    color: #b4b4c0;
    font-size: 12px;
  }
  .address input {
    flex: 1;
    min-width: 120px;
  }
  .ask {
    padding: 4px 8px;
    background: #2b2616;
    border: 1px solid #6b5a22;
    border-radius: 3px;
  }
  .warn {
    color: #ffd36b;
  }
  .warn p {
    margin: 0 0 6px;
  }
  .error {
    margin: 0;
    color: #ff8f8f;
  }
  .tabs {
    display: flex;
    gap: 2px;
    overflow-x: auto;
    border-bottom: 1px solid #2a2a31;
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
  .tab [aria-selected="true"] {
    border-bottom: 2px solid #4a6bb0;
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
    padding: 2px 8px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
  }
  input {
    background: #121216;
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    cursor: default;
    opacity: 0.6;
  }
  button:focus-visible,
  input:focus-visible,
  select:focus-visible,
  h2:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
</style>
