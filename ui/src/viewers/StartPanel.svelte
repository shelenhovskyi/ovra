<script lang="ts">
// The main area when a project is open but no file is: the next steps, as
// buttons that run the same commands as the palette and their shortcuts.
import { type Command, listCommands } from "../keys/commands";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";

const ACTIONS = [
  { command: "agents.new_session", label: "Give Claude a task" },
  { command: "palette.go_to_file", label: "Find a file" },
  { command: "search.show", label: "Search in files" },
  { command: "terminal.new", label: "Open a terminal" },
] as const;

/** Commands are registered by their panels, which are mounted by the time a project is open. */
let commands = $state<Command[]>([]);
$effect(() => {
  commands = listCommands();
});

const actions = $derived(
  ACTIONS.flatMap(({ command, label }) => {
    const found = commands.find((c) => c.id === command);
    if (!found) return [];
    const keys = found.keys ? formatShortcut(liveKeys(found.keys, inBrowserTab()), isApple()) : null;
    return [{ label, keys, run: found.run }];
  }),
);
</script>

<section class="start" data-testid="start" aria-labelledby="start-title">
  <h2 id="start-title">No file open</h2>
  {#if actions.length === 0}
    <p>Open a file from the tree on the left.</p>
  {:else}
    <p>Open a file from the tree on the left, or:</p>
    <ul>
      {#each actions as action (action.label)}
        <li>
          <button type="button" onclick={() => void action.run()}>
            <span class="label">{action.label}</span>
            {#if action.keys}<kbd>{action.keys}</kbd>{/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .start {
    max-width: 340px;
    margin: 18vh auto 0;
    padding: 0 16px;
    color: var(--text-2);
  }
  h2 {
    margin: 0 0 6px;
    font-size: 16px;
    font-weight: 600;
    color: var(--text);
  }
  p {
    margin: 0 0 12px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  button {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 6px 10px;
    font: inherit;
    color: var(--text);
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  button:hover {
    background: var(--s3);
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  kbd {
    font: 12px var(--mono);
    color: var(--text-2);
  }
</style>
