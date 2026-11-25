<script lang="ts">
// The main area when a project is open but no file is: the project's name
// and the next steps, as buttons that run the same commands as the palette
// and their shortcuts.
import { type Command, listCommands } from "../keys/commands";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import { useOvra } from "../shell/context";
import EmptyState from "../shell/EmptyState.svelte";

const ACTIONS = [
  { command: "agents.new_session", label: "New task" },
  { command: "palette.go_to_file", label: "Find a file" },
  { command: "search.show", label: "Search" },
  { command: "terminal.toggle", label: "Terminal" },
] as const;

const { workspace } = useOvra();

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
    return [{ label, keys, run: () => void found.run() }];
  }),
);
</script>

<EmptyState
  testid="start"
  centred
  headline={workspace.project?.name ?? "No file open"}
  line="Open a file, or give Claude a task."
  {actions}
/>
