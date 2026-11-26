<script lang="ts">
// Settings > Setup check: everything Ovra needs on this computer in one
// list (`setup.check`), each row with what was found and, for a problem,
// one way to fix it. Checks again on request; nothing here changes anything.
import { untrack } from "svelte";
import type { SetupReport } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";
import Icon from "../shell/Icon.svelte";
import type { IconName } from "../shell/icons";
import { openSettings } from "./commands";
import { type CheckLevel, checkRows, summary } from "./setup-check";

const { transport, workspace } = useOvra();

let report = $state<SetupReport | null>(null);
let error = $state<string | null>(null);
let busy = $state(false);

const rows = $derived(report ? checkRows(report) : []);
const project = $derived(workspace.project?.id ?? null);

const ICON: Record<CheckLevel, IconName> = { ok: "check", warning: "warning", problem: "error", skipped: "info" };
const LABEL: Record<CheckLevel, string> = { ok: "OK", warning: "Warning", problem: "Problem", skipped: "Not checked" };

$effect(() => {
  void project;
  untrack(() => void check());
});

function text(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}

async function check(): Promise<void> {
  if (busy) return;
  busy = true;
  try {
    report = (await transport.call({ method: "setup.check", params: { project } })).data;
    error = null;
  } catch (err) {
    error = text(err);
  } finally {
    busy = false;
  }
}

function open(url: string): void {
  transport.call({ method: "open.url", params: { url } }).catch((err: unknown) => (error = text(err)));
}
</script>

<div class="setup" data-testid="setup-check" aria-busy={busy}>
  <div class="head">
    <p class="summary" role="status">
      {#if report}{summary(rows)}{:else if !error}Checking…{/if}
    </p>
    <button type="button" disabled={busy} onclick={() => void check()}>Check again</button>
  </div>
  {#if error}<p class="error" role="alert">Could not check: {error}</p>{/if}
  {#if report}
    <ul aria-label="Checks">
      {#each rows as row (row.id)}
        <li class={row.level} data-testid={`setup-${row.id}`}>
          <span class="mark" role="img" aria-label={LABEL[row.level]}><Icon name={ICON[row.level]} small /></span>
          <div class="body">
            <p class="name">{row.name}</p>
            <p class="title">{row.title}</p>
            {#if row.fix}<p class="fix">{row.fix}</p>{/if}
            {#if row.commands.length > 0}
              <ul class="commands" aria-label={`Commands for ${row.name}`}>
                {#each row.commands as command (command.command)}
                  <li>
                    {#if command.label}<span>{command.label}</span>{/if}
                    <code>{command.command}</code>
                  </li>
                {/each}
              </ul>
            {/if}
            {#if row.level !== "ok" && row.level !== "skipped" && (row.link || row.pane)}
              <div class="actions">
                {#if row.link}
                  {@const link = row.link}
                  <button type="button" class="link" onclick={() => open(link.url)}>{link.label}</button>
                {/if}
                {#if row.pane}
                  {@const pane = row.pane}
                  <button type="button" class="link" onclick={() => openSettings(pane)}>Open Agents settings</button>
                {/if}
              </div>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .setup {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    max-width: 560px;
  }
  p {
    margin: 0;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
  }
  .summary {
    font-weight: 600;
  }
  .error {
    color: var(--danger);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    gap: var(--sp-2);
    padding: var(--sp-2) 0;
  }
  li + li {
    border-top: 1px solid var(--separator);
  }
  .mark {
    display: inline-flex;
    flex: none;
    padding-top: 1px;
  }
  .ok .mark {
    color: var(--ok);
  }
  .warning .mark {
    color: var(--warn);
  }
  .problem .mark {
    color: var(--danger);
  }
  .skipped .mark {
    color: var(--text-2);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    min-width: 0;
  }
  .name {
    font: var(--t-small);
    color: var(--text-2);
  }
  .title {
    overflow-wrap: anywhere;
  }
  .fix {
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .commands li {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
    padding: 0;
    border: 0;
  }
  .commands span {
    font: var(--t-small);
    color: var(--text-2);
  }
  code {
    font: 13px / 18px var(--mono);
    overflow-wrap: anywhere;
    user-select: all;
  }
  .actions {
    display: flex;
    gap: var(--sp-3);
  }
  button {
    height: var(--ctl);
    padding: 0 var(--sp-3);
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
    cursor: default;
  }
  button:hover {
    background: var(--s4);
  }
  button.link {
    height: auto;
    padding: 0;
    color: var(--accent);
    background: transparent;
  }
  button.link:hover {
    text-decoration: underline;
    background: transparent;
  }
  @media (forced-colors: active) {
    button {
      border: 1px solid ButtonText;
    }
  }
</style>
