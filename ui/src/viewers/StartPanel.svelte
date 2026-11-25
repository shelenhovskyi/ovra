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
    max-width: 320px;
    margin: 48px auto 0;
    padding: 0 16px;
    color: #b4b4c0;
  }
  h2 {
    margin: 0 0 4px;
    font-size: 14px;
    font-weight: 600;
    color: #e6e6e6;
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
    color: #e6e6e6;
    text-align: left;
    background: #1e1e24;
    border: 1px solid #2a2a31;
    border-radius: 4px;
    cursor: pointer;
  }
  button:hover {
    background: #26262e;
    border-color: #3a3a44;
  }
  button:focus-visible {
    outline: 2px solid #4a6bb0;
    outline-offset: 1px;
  }
  kbd {
    font: inherit;
    font-size: 12px;
    color: #9a9aa6;
  }
</style>
