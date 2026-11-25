<script lang="ts">
// The audit log viewer: a header button (and the "Audit log" command) opens a
// dialog with the open project's entries, newest first, updated live. It
// opens on the newest page; "Load older" at the end of the list reads the page
// before. It only reads: the core offers no way to edit or remove an entry.
// Escape closes.
import { tick, untrack } from "svelte";
import { agentSessions } from "../agents/sessions.svelte";
import { registerCommand } from "../keys/commands";
import { useOvra } from "../shell/context";
import { describe, FILTERS, type Filter, shows, when } from "./describe";
import { AuditTrail } from "./trail.svelte";

/** Whether to show the header button; the shell opens this from its "More" menu instead. */
let { trigger = true }: { trigger?: boolean } = $props();
const { transport, workspace } = useOvra();
const trail = new AuditTrail(transport);
const sessions = agentSessions(transport);

let opened = $state(false);
let filter = $state<Filter>("all");
let refusedOnly = $state(false);
let dialog = $state<HTMLElement>();
/** Read out when older entries arrive. */
let olderNote = $state("");
let returnTo: HTMLElement | null = null;

const shown = $derived(
  trail.entries
    .filter((entry) => shows(filter, refusedOnly, entry))
    .reverse()
    .map((entry) => ({ entry, ...describe(entry) })),
);

$effect(() => () => trail.dispose());
$effect(() => registerCommand({ id: "audit.toggle", title: "Audit log", run: toggle }));

// Read the log when the dialog opens, and again for another project.
$effect(() => {
  const project = workspace.project?.id ?? null;
  // Only these two: reading the log must not run it again.
  if (opened) untrack(() => void trail.show(project));
});

async function show(): Promise<void> {
  const active = document.activeElement;
  returnTo = active instanceof HTMLElement ? active : null;
  opened = true;
  await tick();
  dialog?.querySelector<HTMLElement>("h2")?.focus();
}

function close(): void {
  opened = false;
  if (returnTo?.isConnected) returnTo.focus();
  returnTo = null;
}

function toggle(): void {
  if (opened) close();
  else void show();
}

function onDialogKeydown(event: KeyboardEvent): void {
  // Shortcuts of the panels behind the modal must not act.
  event.stopPropagation();
  if (event.code === "Escape") {
    event.preventDefault();
    close();
    return;
  }
  if (event.code !== "Tab") return;
  // Keep Tab inside the dialog (aria-modal).
  const focusable = [...(dialog?.querySelectorAll<HTMLElement>("button, input, select, [tabindex]") ?? [])].filter(
    (el) => el.tabIndex >= 0 && !el.matches(":disabled"),
  );
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (!first || !last) return;
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

async function loadOlder(): Promise<void> {
  const added = await trail.loadOlder();
  if (added > 0) olderNote = `${added} older ${added === 1 ? "entry" : "entries"} loaded.`;
  await tick();
  // At the start of the log the button goes; focus must stay in the dialog.
  if (opened && !dialog?.contains(document.activeElement)) dialog?.querySelector<HTMLElement>("h2")?.focus();
}

function who(session: string | null): string {
  return session ? sessions.name(session) : "Project";
}
</script>

{#if trigger}
  <button type="button" class="open-audit" data-testid="open-audit" aria-haspopup="dialog" onclick={() => void show()}>
    Audit log
  </button>
{/if}

{#if opened}
  <div class="backdrop">
    <div
      class="audit"
      role="dialog"
      aria-modal="true"
      aria-labelledby="audit-title"
      tabindex="-1"
      data-testid="audit"
      bind:this={dialog}
      onkeydown={onDialogKeydown}
    >
      <header>
        <h2 id="audit-title" tabindex="-1">Audit log</h2>
        <span class="muted">{workspace.project?.name ?? "No project"}</span>
        <label>
          Show
          <select bind:value={filter} data-testid="audit-filter">
            {#each Object.entries(FILTERS) as [value, { label }] (value)}
              <option {value}>{label}</option>
            {/each}
          </select>
        </label>
        <label><input type="checkbox" bind:checked={refusedOnly} data-testid="audit-refused" /> Refused only</label>
        <button type="button" class="close" aria-keyshortcuts="Escape" onclick={close}>Close</button>
      </header>
      <p class="muted note">
        What agents did and who allowed it: session starts, tool decisions, trust, policy and review decisions. Entries
        are only ever added. Inputs keep no file contents, and token-like text is removed.
      </p>
      {#if trail.error}
        <p class="error" role="alert">The audit log could not be read: {trail.error}</p>
      {/if}
      {#if !workspace.project}
        <p class="muted note">Open a project to see its audit log.</p>
      {:else if shown.length === 0}
        <p class="muted note" data-testid="audit-empty">
          {trail.loading ? "Loading the audit log…" : trail.entries.length === 0 ? "Nothing recorded yet." : "No entries match."}
        </p>
      {:else}
        <ol class="entries" aria-label="Audit entries, newest first">
          {#each shown as row (row.entry.seq)}
            <li class={row.tone} data-testid="audit-entry">
              <div class="line">
                <time datetime={new Date(row.entry.at_ms).toISOString()}>{when(row.entry.at_ms)}</time>
                <span class="who">{who(row.entry.session)}</span>
                <strong>{row.title}</strong>
              </div>
              <p class="detail">{row.detail}</p>
              {#if row.input}
                <pre>{row.input}</pre>
              {/if}
            </li>
          {/each}
        </ol>
      {/if}
      {#if workspace.project && trail.older}
        <p class="more">
          <span class="muted">Older entries are not loaded yet.</span>
          <button
            type="button"
            data-testid="audit-older"
            aria-busy={trail.loadingOlder}
            onclick={() => void loadOlder()}
          >
            {trail.loadingOlder ? "Loading…" : "Load older"}
          </button>
        </p>
      {/if}
      <p class="visually-hidden" role="status" data-testid="audit-older-note">{olderNote}</p>
    </div>
  </div>
{/if}

<style>
  button,
  select {
    font: inherit;
    padding: 2px 10px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    cursor: pointer;
  }
  button:focus-visible,
  select:focus-visible,
  input:focus-visible,
  h2:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 90;
    display: flex;
    padding: 24px;
    background: rgb(0 0 0 / 55%);
  }
  .audit {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: #16161a;
    border: 1px solid #3a3a44;
    border-radius: 6px;
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-bottom: 1px solid #2a2a31;
  }
  button[aria-busy="true"] {
    cursor: progress;
    opacity: 0.6;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .close {
    margin-left: auto;
  }
  h2 {
    margin: 0;
    font-size: 14px;
  }
  .muted {
    color: #b4b4c0;
    font-size: 12px;
  }
  .note {
    margin: 6px 12px;
  }
  .error {
    color: #ff8f8f;
    margin: 6px 12px;
  }
  .entries {
    flex: 1;
    overflow: auto;
    list-style: none;
    margin: 0;
    padding: 4px 12px 12px;
  }
  li {
    padding: 6px 8px;
    border-left: 3px solid #3a3a44;
    border-bottom: 1px solid #24242b;
  }
  .more {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0;
    padding: 6px 12px;
    border-top: 1px solid #2a2a31;
  }
  li.refused {
    border-left-color: #ff8f8f;
  }
  li.warn {
    border-left-color: #ffd36b;
  }
  .line {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    align-items: baseline;
  }
  time,
  .who {
    color: #b4b4c0;
    font-size: 12px;
  }
  .detail {
    margin: 2px 0 0;
    color: #c8c8d2;
  }
  pre {
    margin: 4px 0 0;
    max-height: 8em;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-size: 12px;
    color: #d8d8e0;
    background: #1e1e24;
    padding: 4px 6px;
    border-radius: 3px;
  }
</style>
