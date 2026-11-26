<script lang="ts">
// The Problems tab of the bottom dock, next to the terminals: every
// diagnostic the open project's language servers published (problems.svelte.ts),
// grouped by file, errors first. "Show problems" (Mod+Shift+M) shows it and
// goes into it, and hides it from inside; Escape inside hides it too. The
// list is one Tab stop: arrows move, Enter or a click opens the spot. A
// row's Ask agent (on hover, and on the chosen row) hands it to a new task.
import { tick } from "svelte";
import { registerCommand } from "../keys/commands";
import { useOvra } from "../shell/context";
import Icon from "../shell/Icon.svelte";
import { askAboutProblem } from "./agent-context";
import { type Problem, problems, problemsDock } from "./lsp/problems.svelte";

const { workspace } = useOvra();

const files = $derived(problems.files(workspace.project?.id));
/** Every problem in list order, with its file. */
const rows = $derived(files.flatMap((file) => file.problems.map((problem) => ({ path: file.path, problem }))));
let active = $state(0);
const current = $derived(Math.min(active, Math.max(rows.length - 1, 0)));

let panel = $state<HTMLElement>();
let returnTo: HTMLElement | null = null;

$effect(() =>
  registerCommand({
    id: "problems.toggle",
    title: "Show problems",
    keys: "Mod+Shift+M",
    category: "View",
    keywords: ["diagnostics", "errors", "warnings"],
    run: toggle,
  }),
);

/** Shows the tab and moves focus into its list (its heading while it is empty). */
async function show(): Promise<void> {
  const focused = document.activeElement;
  if (!panel?.contains(focused)) returnTo = focused instanceof HTMLElement ? focused : null;
  problemsDock.shown = true;
  await tick();
  (panel?.querySelector<HTMLElement>('[role="option"][tabindex="0"]') ?? panel?.querySelector("h2"))?.focus();
}

function hide(): void {
  const inside = !!panel?.contains(document.activeElement);
  problemsDock.shown = false;
  if (inside && returnTo?.isConnected) returnTo.focus();
  returnTo = null;
}

function toggle(): void {
  if (problemsDock.shown && panel?.contains(document.activeElement)) hide();
  else void show();
}

function open(index: number): void {
  const row = rows[index];
  if (!row) return;
  active = index;
  workspace.openFile(row.path, "edit", row.problem.spot);
}

async function move(index: number): Promise<void> {
  active = Math.min(Math.max(index, 0), rows.length - 1);
  await tick();
  panel?.querySelector<HTMLElement>('[role="option"][tabindex="0"]')?.focus();
}

function onListKey(event: KeyboardEvent): void {
  const step: Record<string, () => number> = {
    ArrowDown: () => current + 1,
    ArrowUp: () => current - 1,
    Home: () => 0,
    End: () => rows.length - 1,
  };
  if (event.code in step) {
    event.preventDefault();
    void move(step[event.code]?.() ?? current);
  } else if (event.code === "Enter" || event.code === "NumpadEnter") {
    event.preventDefault();
    open(current);
  }
}

function onkeydown(event: KeyboardEvent): void {
  if (event.code !== "Escape" || event.defaultPrevented) return;
  event.preventDefault();
  hide();
}

const NAMES: Record<Problem["severity"], string> = { error: "Error", warning: "Warning", info: "Info" };

/** Where each file's rows start in `rows`. */
const starts = $derived(files.map((_, i) => files.slice(0, i).reduce((n, f) => n + f.problems.length, 0)));
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section
  class="problems-dock"
  aria-labelledby="problems-dock-title"
  hidden={!problemsDock.shown}
  data-testid="problems"
  bind:this={panel}
  {onkeydown}
>
  <div class="bar">
    <h2 id="problems-dock-title" tabindex="-1">Problems</h2>
    <button type="button" class="hide" aria-label="Hide problems" title="Hide problems (Escape)" onclick={hide}
      ><Icon name="chevron" small /></button
    >
  </div>
  <div class="body">
    {#if rows.length === 0}
      <p class="empty">No problems.</p>
    {:else}
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div role="listbox" aria-label="Problems" tabindex="-1" onkeydown={onListKey}>
        {#each files as file, f (file.path)}
          <div role="group" aria-label={file.path}>
            <div class="file" aria-hidden="true">{file.path}</div>
            {#each file.problems as problem, i (i)}
              {@const index = (starts[f] ?? 0) + i}
              <!-- The list's keys (onListKey) open the row from the keyboard. -->
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <div
                role="option"
                class="row {problem.severity}"
                tabindex={index === current ? 0 : -1}
                aria-selected={index === current}
                title={problem.message}
                data-testid="problem"
                onclick={() => open(index)}
              >
                <span class="sev" role="img" aria-label={NAMES[problem.severity]}><Icon name={problem.severity} small /></span>
                <span class="message">{problem.message}</span>
                <span class="where">{problem.line}:{problem.column}</span>
                <!-- Pointer only, like the editor's hover card; the row stays the list's one stop. -->
                <button
                  type="button"
                  class="ask"
                  tabindex="-1"
                  aria-label="Ask agent"
                  title="Ask agent"
                  onclick={(event) => {
                    event.stopPropagation();
                    askAboutProblem(file.path, problem);
                  }}><Icon name="agent" small /></button
                >
              </div>
            {/each}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</section>

<style>
  .problems-dock {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--s2);
  }
  .problems-dock[hidden] {
    display: none;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: 34px;
    padding: 4px 8px 4px 12px;
  }
  h2 {
    margin: 0;
    font: var(--t-headline);
  }
  .hide {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    margin-left: auto;
    padding: 0;
    color: var(--text-2);
    background: none;
    border: 0;
    border-radius: var(--r-sm);
  }
  .hide:hover {
    color: var(--text);
    background: var(--s3);
  }
  .body {
    flex: 1;
    min-height: 0;
    padding: 0 8px 12px;
    overflow: auto;
  }
  .empty {
    margin: 4px;
    color: var(--text-2);
  }
  .file {
    padding: 6px 8px 2px;
    font-family: var(--mono);
    font-size: 13px;
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 26px;
    padding: 0 8px 0 16px;
    border-radius: var(--r-sm);
    cursor: default;
  }
  .row:hover {
    background: var(--s3);
  }
  .row[aria-selected="true"]:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .sev {
    display: grid;
    place-items: center;
    color: var(--text-2);
  }
  .error .sev {
    color: var(--danger);
  }
  .warning .sev {
    color: var(--warn);
  }
  .message {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ask {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    color: var(--text-2);
    background: none;
    border: 0;
    border-radius: var(--r-sm);
    visibility: hidden;
  }
  .row:hover .ask,
  .row[aria-selected="true"] .ask {
    visibility: visible;
  }
  .ask:hover {
    color: var(--text);
    background: var(--s4);
  }
  @media (hover: none) {
    .ask {
      visibility: visible;
    }
  }
  .where {
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }
</style>
