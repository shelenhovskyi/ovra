<script lang="ts">
// Per-project trust. Until the user trusts a project, Claude runs with the
// user's own settings only; trusting it also loads the project's .claude/
// settings, hooks, agents, commands and skills. A project without settings
// of its own shows no card: the `projects.review_trust` command opens it
// (the language servers' "Turn on" does). With settings, the card leads
// with one plain sentence and the warning lines (short, counted, never
// hidden); "Review and trust…" opens the full lists, and Trust sits at their
// bottom, so nobody trusts what they did not see listed. Once trusted, the
// card folds to one line with the warning count; its toggle stays in the
// heading row, open or not. Settings that changed since (a git pull, say)
// do not load until the user trusts them again.
import { tick, untrack } from "svelte";
import { registerCommand } from "../../keys/commands";
import type { ProjectConfigReview } from "../../lib/protocol";
import { CoreError } from "../../lib/transport";
import { languageServers } from "../../lsp/servers.svelte";
import { useOvra } from "../../shell/context";
import Icon from "../../shell/Icon.svelte";
import {
  changeSummary,
  changes,
  groups,
  KIND_LABELS,
  type Row,
  runsCode,
  TRUST_ARM_MS,
  touchesConfig,
  warnings,
  weight,
} from "./texts";
import { trustWaiting } from "./trust-state.svelte";

let {
  line = false,
  onreview,
}: {
  /** One line for an open task: the settings wait for a decision, with Review. */
  line?: boolean;
  /** Review on the line: the full card is in the task list. */
  onreview?: () => void;
} = $props();

/** Entries shown per file before "+N more". */
const SHOWN = 5;

const { transport, workspace } = useOvra();
const lsp = languageServers(transport);
let section = $state<HTMLElement>();

let review = $state<ProjectConfigReview | null>(null);
/** The card is open (a trusted one folds to a line), or the "no settings" card is asked for. */
let details = $state(false);
/** The full lists are open. */
let lists = $state(false);
let more = $state<Record<string, boolean>>({});
let busy = $state(false);
let error = $state<string | null>(null);
// Answers can come back out of order: only the latest request's counts.
let latest = 0;
// The trust buttons count a press only TRUST_ARM_MS after what the card shows last changed.
let shownAt = $state(Date.now());
let now = $state(Date.now());
const armed = $derived(now >= shownAt + TRUST_ARM_MS);
const EARLY = "Ready in a moment — read the card first.";
/** A press came before the buttons were ready. */
let early = $state(false);

const project = $derived(workspace.project);
const changed = $derived(
  project?.trusted === true && review !== null && review.trusted?.fingerprint !== review.current.fingerprint,
);
const diff = $derived(changed && review?.trusted ? changes(review.trusted, review.current) : []);
const items = $derived(review?.current.items ?? []);
// Unknown until the settings were read: a failed read is not "no settings".
const empty = $derived(review !== null && !changed && items.length === 0);
// What the user is asked about: what was added when the settings changed, all of it otherwise.
const risk = $derived(
  changed
    ? warnings(
        diff.map((c) => ({ path: c.path, kind: c.kind, entries: c.added })),
        "changed",
      )
    : warnings(items, project?.trusted ? "trusted" : "untrusted"),
);
const counts = $derived(
  changeSummary(diff) ||
    "The list is the same, but files under .claude/ changed (a skill, an agent, a command or a setting not fully shown, for example). Open them to see what changed.",
);
const shownGroups = $derived(
  groups(
    changed
      ? diff.map((c) => ({
          path: c.path,
          kind: c.kind,
          rows: [
            ...c.added.map((text): Row => ({ text, sign: "added" })),
            ...c.removed.map((text): Row => ({ text, sign: "removed" })),
          ],
        }))
      : items.map((item) => ({ path: item.path, kind: item.kind, rows: item.entries.map((text) => ({ text })) })),
  ),
);
const heading = $derived(
  changed
    ? "Project settings changed"
    : project?.trusted
      ? empty
        ? "Project trusted · no Claude settings of its own"
        : "Project's own settings are on"
      : empty
        ? "No Claude settings in this project"
        : "This project has its own Claude settings",
);
const lead = $derived(
  changed
    ? "Claude runs without them until you trust the changes."
    : project?.trusted
      ? "Claude uses this project's own commands, hooks and skills. If they change, Ovra asks again."
      : "Trusting lets Claude use this project's own commands, hooks and skills, and turns on code help (language servers). Trust only a project whose authors you trust; Claude works without it.",
);
const sandbox = $derived(lsp.status?.sandbox);
/** Trusted and unchanged: nothing to decide, so one quiet line until Details opens. */
const settled = $derived(project?.trusted === true && review !== null && !changed && !empty && !details);
/** A decision is pending: the lists lead to a Trust button. */
const deciding = $derived(changed || project?.trusted !== true);
/**
 * A decision waits but nobody asked to see it: no card, only the Agents
 * header's "things to check" count (trust-state). Its menu and the command
 * open the card; nothing loads until Trust at the bottom of its lists.
 */
const folded = $derived(!line && deciding && !details && review !== null && !empty);
const warningCount = $derived([risk.broad, risk.code, risk.env, risk.unreadable].filter(Boolean).length);

// The header's menu sees what waits here, and opens the card.
$effect(() => {
  if (line) return;
  trustWaiting.waiting = !!project && review !== null && deciding && !empty;
  trustWaiting.changed = changed;
  trustWaiting.warnings = warningCount;
});
$effect(() => {
  if (line) return;
  trustWaiting.open = () => void reveal(false);
  return () => {
    trustWaiting.waiting = false;
    trustWaiting.open = null;
  };
});

/** Opens the card (and its lists, when asked), scrolled into view, focus on its heading. */
async function reveal(withLists: boolean): Promise<void> {
  details = true;
  if (withLists) lists = true;
  await tick();
  section?.scrollIntoView?.({ block: "nearest" });
  // Never a Trust button: the Enter or press that opened it must not trust.
  section?.querySelector<HTMLElement>("#trust-heading")?.focus();
  rearm();
}

// The language servers' footer links here: trusting is what starts them.
$effect(() => {
  if (line) return;
  return registerCommand({
    id: "projects.review_trust",
    title: "Review and trust this project",
    run: () => reveal(true),
  });
});

/** Opens or closes the lists; opened, they are scrolled into view. */
async function toggleLists(): Promise<void> {
  lists = !lists;
  if (!lists) return;
  await tick();
  section?.querySelector("#trust-details")?.scrollIntoView?.({ block: "start", behavior: "smooth" });
}

function rearm(): void {
  shownAt = Date.now();
  now = shownAt;
  early = false;
}

// Whatever changes what the card shows (or where it puts the buttons) arms them again.
$effect(() => {
  void [project?.id, project?.trusted, review?.current.fingerprint, changed, empty, details, lists];
  untrack(rearm);
});

$effect(() => {
  if (armed) return;
  const timer = setTimeout(() => (now = Date.now()), Math.max(0, shownAt + TRUST_ARM_MS - Date.now()));
  return () => clearTimeout(timer);
});

/**
 * Trusts on a deliberate press only: not before the card is armed (an
 * early press says why instead), not from a held key.
 */
function trustPress(): void {
  if (armed) void trust(true);
  else early = true;
}

function noRepeat(event: KeyboardEvent): void {
  // A held Enter repeats clicks on a focused button.
  if (event.repeat) event.preventDefault();
}

$effect(() => {
  const id = project?.id;
  untrack(() => {
    details = false;
    lists = false;
    more = {};
    review = null;
    error = null;
    if (id) void load(id);
  });
});

// Check again when changed settings matter (a session starts) and when they
// may have changed: a review was decided, or files under .claude/ changed.
// The summary is always on screen, so untrusted projects are re-read too.
$effect(() =>
  transport.onEvent((event) => {
    const id = project?.id;
    const matters =
      (event.type === "agents.status" && event.data.status.state === "starting") ||
      event.type === "review.changed" ||
      (event.type === "fs.changed" && touchesConfig(event.data.paths));
    if (matters && id && event.data.project === id) void load(id);
  }),
);

function text(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}

async function load(id: string): Promise<void> {
  const request = ++latest;
  try {
    const answer = await transport.call({ method: "agents.project_config", params: { project: id } });
    if (request === latest && workspace.project?.id === id) review = answer.data;
  } catch (err) {
    if (request === latest) error = text(err);
  }
}

function trust(trusted: boolean): Promise<void> {
  const id = project?.id;
  // The settings the card shows: the core refuses them if they changed meanwhile.
  const fingerprint = review?.current.fingerprint;
  if (!id || busy || (trusted && !fingerprint)) return Promise.resolve();
  busy = true;
  error = null;
  return (async () => {
    try {
      const updated = await transport.call({
        method: "projects.trust",
        params: trusted ? { id, trusted, fingerprint } : { id, trusted },
      });
      if (workspace.project?.id === id) workspace.project = updated.data;
      details = false;
      lists = false;
    } catch (err) {
      error = text(err);
    } finally {
      // After a conflict this shows what changed meanwhile.
      await load(id);
      busy = false;
    }
    // The pressed button went with the details: focus stays on the card, not the page.
    await tick();
    focusHeading();
  })();
}

function focusHeading(): void {
  section?.querySelector<HTMLElement>("#trust-heading")?.focus();
}

/** Escape folds the details, as it closes other panels' extras; focus stays on the card. */
async function onkeydown(event: KeyboardEvent): Promise<void> {
  if (event.code !== "Escape" || !details) return;
  event.stopPropagation();
  details = false;
  lists = false;
  await tick();
  focusHeading();
}
</script>

{#snippet warningChip()}
  {#if warningCount > 0}
    {@const words = warningCount === 1 ? " warning" : " warnings"}
    <span class="chip" title={`${warningCount}${words}: see Details`} data-testid="trust-warning-count"
      ><span aria-hidden="true">⚠</span> {warningCount}<span class="visually-hidden">{words}</span></span
    >
  {/if}
{/snippet}

{#if line}
  <!-- While a task is open the card is in the list: one line says the decision still waits. -->
  {#if project && review && deciding && !empty}
    <p class="trust-line" data-testid="project-trust-line">
      <Icon name="shield-alert" small /><span
        >Project settings: {changed ? "changed, not trusted" : "not trusted"}, so Claude runs without them</span
      >
      <button type="button" class="link" onclick={onreview}>Review</button>
    </p>
  {/if}
  <!-- No settings of its own: nothing to trust here until someone asks (the command). -->
{:else if project && (review || error) && !(empty && !details) && !folded}
  <!-- Escape from any of its controls folds the details. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <section
    class="trust"
    class:changed
    class:quiet={empty || settled}
    aria-labelledby="trust-heading"
    data-testid="project-trust"
    bind:this={section}
    {onkeydown}
  >
    {#if !review}
      <h3 id="trust-heading" tabindex="-1">Project settings</h3>
    {:else if empty}
      <div class="row">
        <h3 id="trust-heading" tabindex="-1">{heading}</h3>
        {#if project.trusted}
          <button type="button" class="link" disabled={busy} onclick={() => trust(false)}>Stop trusting</button>
        {:else}
          <!-- Nothing of the project's own runs with Claude: a quiet button, not the panel's primary. -->
          <button
            type="button"
            class="link edge"
            disabled={busy}
            aria-disabled={!armed}
            title={armed ? undefined : EARLY}
            onkeydown={noRepeat}
            onclick={trustPress}>Trust project</button
          >
        {/if}
        <button type="button" class="link" onclick={() => (details = false)}>Close</button>
      </div>
      <!-- Says why an early press did nothing; goes once the buttons are ready. -->
      <p class="wait" aria-live="polite" data-testid="trust-wait">{early && !armed ? EARLY : ""}</p>
      {#if !project.trusted}
        <p data-testid="trust-language-servers">
          Trusting it also starts code help (language servers), which runs the project's code{sandbox
            ? " in the sandbox"
            : " without a sandbox on this computer"}.
        </p>
      {/if}
    {:else}
      <div class="row">
        <h3 id="trust-heading" tabindex="-1">{heading}</h3>
        {#if project.trusted && !changed}
          <!-- Open or folded, the toggle stays here, where the eye left it. -->
          <button
            type="button"
            class="link toggle"
            aria-expanded={!settled}
            aria-controls="trust-body"
            onclick={() => (details = !details)}
          >
            {#if settled}{@render warningChip()}{/if}
            {settled ? "Details" : "Hide"}
          </button>
        {:else}
          <!-- Opened from its line: Hide folds it back; the decision still waits there. -->
          <button
            type="button"
            class="link toggle"
            aria-expanded="true"
            aria-controls="trust-body"
            onclick={() => {
              details = false;
              lists = false;
            }}>Hide</button
          >
        {/if}
      </div>
      {#if !settled}
        <div id="trust-body" class="body">
          <p class="detail">{lead}</p>
          {#if changed}
            <!-- Only file contents changed (a skill's script, say): nothing below names it, so it warns. -->
            <p class:warning={diff.length === 0} data-testid="trust-summary">
              {diff.length === 0 ? "⚠ " : ""}{counts}
            </p>
          {/if}
          <!-- "⚠" marks warnings by shape, not only by colour. -->
          {#if risk.broad}
            <p class="warning" role="note" data-testid="broad-rules">⚠ {risk.broad}</p>
          {/if}
          {#if risk.code}
            <p class="warning" role="note" data-testid="trust-runs-code">⚠ {risk.code}</p>
          {/if}
          {#if risk.env}
            <p class="warning" role="note" data-testid="trust-risky-env">⚠ {risk.env}</p>
          {/if}
          {#if risk.unreadable}
            <p class="warning" role="note" data-testid="trust-unreadable">⚠ {risk.unreadable}</p>
          {/if}
          {#if !project.trusted && !sandbox}
            <p class="warning" role="note" data-testid="trust-no-sandbox">
              ⚠ Language servers will run this project's code without a sandbox on this computer.
            </p>
          {/if}
          <div class="row">
            <button
              type="button"
              class="link disclose"
              aria-expanded={lists}
              aria-controls="trust-details"
              onclick={toggleLists}
              ><span class="chevron"><Icon name={lists ? "chevron" : "chevron-right"} small /></span>{project.trusted &&
              !changed
                ? "What's included"
                : "Review and trust…"}</button
            >
            {#if project.trusted}
              <button type="button" disabled={busy} onclick={() => trust(false)}>Stop trusting</button>
            {/if}
          </div>
          {#if lists}
            <div id="trust-details" class="details" data-testid="trust-details">
              {#if changed}
                <p class="detail">Lines with + load once you trust them; lines with − no longer load.</p>
              {/if}
              {#each shownGroups as group (group.kind)}
                <!-- Deciding: every group open, so Trust comes after all of it. -->
                <details class="group" data-kind={group.kind} open={deciding || weight(group.kind) > 0}>
                  <summary>
                    {KIND_LABELS[group.kind]} <span class="count">{group.count}</span>
                    {#if runsCode(group.kind)}<strong>· runs code</strong>{/if}
                  </summary>
                  {#each group.files as file (file.path)}
                    {@const key = `${group.kind}\u0000${file.path}`}
                    {@const rows = more[key] ? file.rows : file.rows.slice(0, SHOWN)}
                    <div class="file">
                      <span class="path">{file.path}</span>
                      <ul>
                        {#each rows as row, n (n)}
                          <li class={row.sign}>
                            {#if row.sign}<span class="sign" aria-label={row.sign === "added" ? "Added" : "Removed"}
                                >{row.sign === "added" ? "+" : "−"}</span
                              >{/if}
                            <code>{row.text}</code>
                          </li>
                        {/each}
                      </ul>
                      {#if rows.length < file.rows.length}
                        <button type="button" class="link" onclick={() => (more[key] = true)}
                          >+{file.rows.length - rows.length} more</button
                        >
                      {/if}
                    </div>
                  {/each}
                </details>
              {/each}
              <p class="detail note" data-testid="trust-language-servers">
                {project.trusted ? "Ovra starts" : "Trusting also lets Ovra start"} code help (language servers) for
                the files you edit. It runs the project's code, such as build scripts,{#if sandbox}{" "}in the <span
                    title={sandbox}>sandbox</span
                  >, kept to the project and without network{:else}{" "}without a sandbox on this computer{/if}. Stop trusting stops it.
                Changes apply to sessions started afterwards.
              </p>
              {#if changed || !project.trusted}
                <!-- Trust at the bottom of the lists: what it loads was in front of the person first. -->
                <div class="row end">
                  <button
                    type="button"
                    class="primary"
                    disabled={busy}
                    aria-disabled={!armed}
                    title={armed ? undefined : EARLY}
                    onkeydown={noRepeat}
                    onclick={trustPress}>{changed ? "Trust the new settings" : "Trust project"}</button
                  >
                </div>
                <!-- Says why an early press did nothing; goes once the buttons are ready. -->
                <p class="wait" aria-live="polite" data-testid="trust-wait">{early && !armed ? EARLY : ""}</p>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    {/if}
    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}
  </section>
{/if}

<style>
  .trust {
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
    background: var(--s2);
    border-radius: var(--radius);
  }
  /* Nothing to decide: one quiet line, no card. */
  .trust.quiet {
    padding: 4px 12px;
    background: none;
    color: var(--text-2);
  }
  h3 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    line-height: 24px;
  }
  .quiet h3 {
    font-size: 13px;
    font-weight: 500;
  }
  .changed h3 {
    color: var(--warn);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .row.end {
    justify-content: flex-end;
    margin-top: 4px;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  p,
  ul {
    margin: 0;
  }
  .detail,
  .path {
    color: var(--text-2);
  }
  /* Security facts stay visible, set apart by colour and the "⚠" shape. */
  .warning,
  strong {
    color: var(--warn);
    font-weight: 500;
  }
  h3:focus {
    outline: none;
  }
  h3:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .wait {
    margin: 0;
    font-size: 13px;
    color: var(--warn);
  }
  strong {
    margin-left: 4px;
  }
  /* No scroll box of its own: only the panel scrolls, so nothing passes by unseen. */
  .details {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  summary {
    cursor: pointer;
  }
  .file {
    padding-left: 14px;
    display: flex;
    flex-direction: column;
  }
  .file ul {
    list-style: none;
    padding-left: 8px;
  }
  code {
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  button {
    height: 28px;
    padding: 0 10px;
    font: inherit;
    color: var(--text-2);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  button:not(.primary):hover:not(:disabled) {
    color: var(--text);
    background: var(--s3);
  }
  button.primary {
    font-weight: 500;
    color: var(--accent-ink);
    background: var(--accent);
  }
  button.link {
    align-self: flex-start;
    padding: 0 4px;
  }
  .quiet button.link,
  button.toggle {
    flex: none;
    white-space: nowrap;
  }
  /* The chip and its label share one button, so they line up and click as one. */
  button.toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 6px;
  }
  button.disclose {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 0 6px 0 0;
  }
  .chevron {
    display: inline-flex;
  }
  .count {
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  .note {
    font-size: 12px;
  }
  /* Quiet, with an edge so it can be found. */
  .quiet button.link.edge {
    height: 24px;
    padding: 0 8px;
    box-shadow: inset 0 0 0 1px var(--s4);
  }
  button:disabled,
  button[aria-disabled="true"] {
    opacity: 0.5;
    cursor: default;
  }
  .error {
    color: var(--danger);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    height: 20px;
    padding: 0 6px;
    font-size: 12px;
    font-weight: 500;
    color: var(--warn);
    background: var(--warn-soft);
    border-radius: var(--radius-sm);
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .added .sign {
    color: var(--ok);
  }
  .removed .sign {
    color: var(--danger);
  }
  .removed code {
    text-decoration: line-through;
  }
  @media (forced-colors: active) {
    .trust.changed {
      border: 2px solid Highlight;
    }
  }
  .trust-line {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: 12px;
    color: var(--text-2);
  }
  .trust-line span {
    min-width: 0;
  }
</style>

