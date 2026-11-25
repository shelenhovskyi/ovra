<script lang="ts">
// Per-project trust. Until the user trusts a project, Claude runs with the
// user's own settings only; trusting it also loads the project's .claude/
// settings, hooks, agents, commands and skills. The card leads with counts
// and the risky parts (rules that skip approvals, what runs code) in one or
// two lines; the full lists sit behind "Show details". Settings that changed
// since (a git pull, say) do not load until the user trusts them again.
import { tick, untrack } from "svelte";
import { registerCommand } from "../../keys/commands";
import type { ProjectConfigReview } from "../../lib/protocol";
import { CoreError } from "../../lib/transport";
import { languageServers } from "../../lsp/servers.svelte";
import { useOvra } from "../../shell/context";
import {
  changeSummary,
  changes,
  groups,
  KIND_LABELS,
  type Row,
  runsCode,
  summary,
  touchesConfig,
  warnings,
  weight,
} from "./texts";

/** Entries shown per file before "+N more". */
const SHOWN = 5;

const { transport, workspace } = useOvra();
const lsp = languageServers(transport);
let section = $state<HTMLElement>();

let review = $state<ProjectConfigReview | null>(null);
let details = $state(false);
let more = $state<Record<string, boolean>>({});
let busy = $state(false);
let error = $state<string | null>(null);
// Answers can come back out of order: only the latest request's counts.
let latest = 0;

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
        diff.map((c) => ({ kind: c.kind, entries: c.added })),
        true,
      )
    : warnings(items),
);
const counts = $derived(
  changed
    ? changeSummary(diff) ||
        "The list is the same, but file contents changed (a hook script, a skill or an environment value, for example)."
    : summary(items),
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
        : "Project settings are on"
      : empty
        ? "No Claude settings in this project"
        : "This project has its own Claude settings",
);
const lead = $derived(
  changed
    ? "Claude runs without them until you trust the changes."
    : project?.trusted
      ? "Claude uses this project's own commands, hooks and skills. If they change, Ovra asks again."
      : "Trusting lets Claude use this project's own commands, hooks and skills, and starts language servers.",
);
const sandbox = $derived(lsp.status?.sandbox);

// The language servers' footer links here: trusting is what starts them.
$effect(() =>
  registerCommand({
    id: "projects.review_trust",
    title: "Review and trust this project",
    run: async () => {
      details = true;
      await tick();
      section?.scrollIntoView({ block: "nearest" });
      (section?.querySelector<HTMLElement>("button.primary") ?? section?.querySelector<HTMLElement>("button"))?.focus();
    },
  }),
);

$effect(() => {
  const id = project?.id;
  untrack(() => {
    details = false;
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
    } catch (err) {
      error = text(err);
    } finally {
      // After a conflict this shows what changed meanwhile.
      await load(id);
      busy = false;
    }
  })();
}
</script>

{#if project && (review || error)}
  <section
    class="trust"
    class:changed
    class:quiet={empty}
    aria-labelledby="trust-heading"
    data-testid="project-trust"
    bind:this={section}
  >
    {#if !review}
      <h3 id="trust-heading">Project settings</h3>
    {:else if empty}
      <div class="row">
        <h3 id="trust-heading">{heading}</h3>
        <button type="button" class="link" class:primary={!project.trusted} disabled={busy} onclick={() => trust(!project.trusted)}>
          {project.trusted ? "Stop trusting" : "Trust project"}
        </button>
      </div>
      {#if !project.trusted}
        <p data-testid="trust-language-servers">
          Trusting it starts language servers, which run the project's code{sandbox
            ? " in the sandbox"
            : " without a sandbox on this computer"}.
        </p>
      {/if}
    {:else}
      <h3 id="trust-heading">{heading}</h3>
      <p class="detail">{lead}</p>
      <p data-testid="trust-summary">{counts}</p>
      {#if risk.broad}
        <p class="warning" role="note" data-testid="broad-rules">{risk.broad}</p>
      {/if}
      {#if risk.code}
        <p class="warning" role="note" data-testid="trust-runs-code">{risk.code}</p>
      {/if}
      {#if risk.unreadable}
        <p class="warning" role="note" data-testid="trust-unreadable">{risk.unreadable}</p>
      {/if}
      {#if !project.trusted && !sandbox}
        <p class="warning" role="note" data-testid="trust-no-sandbox">
          Language servers will run this project's code without a sandbox on this computer.
        </p>
      {/if}
      <div class="row">
        <button
          type="button"
          class="link"
          aria-expanded={details}
          aria-controls="trust-details"
          onclick={() => (details = !details)}>{details ? "Hide details" : "Show details"}</button
        >
        <span class="actions">
          {#if project.trusted}
            <button type="button" disabled={busy} onclick={() => trust(false)}>Stop trusting</button>
          {/if}
          {#if changed}
            <button type="button" class="primary" disabled={busy} onclick={() => trust(true)}>Trust the new settings</button>
          {:else if !project.trusted}
            <button type="button" class="primary" disabled={busy} onclick={() => trust(true)}>Trust project</button>
          {/if}
        </span>
      </div>
      {#if details}
        <div id="trust-details" class="details" data-testid="trust-details">
          <p class="detail" data-testid="trust-language-servers">
            {project.trusted ? "Ovra starts" : "Trusting also lets Ovra start"} language servers for the files you edit.
            They run the project's code (build scripts, for example){sandbox
              ? ` in the sandbox (${sandbox}), which keeps them to the project and their cache, without network`
              : ", without a sandbox on this computer"}. Stop trusting stops them.
          </p>
          <p class="detail">
            {changed ? "Lines with + load once you trust them; lines with − no longer load." : ""}
            Changes apply to sessions started afterwards.
          </p>
          {#each shownGroups as group (group.kind)}
            <details class="group" data-kind={group.kind} open={weight(group.kind) > 0}>
              <summary>
                {KIND_LABELS[group.kind]}
                {#if runsCode(group.kind)}<strong>runs code</strong>{/if}
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
    margin: 0 12px 8px;
    padding: 6px 8px;
    border: 1px solid #2a2a31;
    border-radius: 3px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
  }
  .trust.quiet {
    border-color: transparent;
    padding-block: 2px;
    color: #9a9aa8;
  }
  h3 {
    margin: 0;
    font-size: 12px;
  }
  .quiet h3 {
    font-weight: normal;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  p,
  ul {
    margin: 0;
  }
  .detail,
  .path {
    color: #b4b4c0;
  }
  .warning,
  strong {
    color: #ffd36b;
  }
  strong {
    margin-left: 4px;
    font-weight: 600;
  }
  .details {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 40vh;
    overflow: auto;
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
    font-family: ui-monospace, monospace;
    font-size: 11px;
    overflow-wrap: anywhere;
  }
  button {
    font: inherit;
    padding: 2px 8px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    cursor: pointer;
  }
  button.primary {
    border-color: #7aa2ff;
  }
  button.link {
    align-self: flex-start;
    padding: 0;
    background: none;
    border: none;
    color: #9db8ff;
    text-decoration: underline;
  }
  button:disabled {
    opacity: 0.55;
    cursor: default;
  }
  button:focus-visible,
  summary:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  .error {
    color: #ff8f8f;
  }
  .trust.changed {
    border-color: #ffd36b;
  }
  .added .sign {
    color: #8fd18f;
  }
  .removed .sign {
    color: #ff8f8f;
  }
  .removed code {
    text-decoration: line-through;
  }
  @media (forced-colors: active) {
    .trust.changed {
      border: 2px solid Highlight;
    }
  }
</style>
