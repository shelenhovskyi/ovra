<script lang="ts">
// The next task for the agents, given without leaving Focus. It starts the
// same way as the Agents panel's form, with the project's account and the
// way of working last chosen in this project; the IDE keeps the rest
// (context chips, accounts, setup).
import { agentSessions } from "../agents/sessions.svelte";
import { rememberedMode, rememberMode, type TaskMode } from "../agents/task-mode";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";

const MODES: { id: TaskMode; label: string }[] = [
  { id: "here", label: "One agent, here" },
  { id: "split", label: "Plan and split" },
  { id: "research", label: "Research" },
];

const { transport, workspace } = useOvra();
const sessions = agentSessions(transport);

let text = $state("");
let busy = $state(false);
let error = $state<string | null>(null);
let mode = $state<TaskMode>("here");

$effect(() => {
  const project = workspace.project?.id;
  if (project) mode = rememberedMode(project);
});

async function start(): Promise<void> {
  const project = workspace.project?.id;
  const body = text.trim();
  if (!project || !body || busy) return;
  busy = true;
  error = null;
  try {
    if (mode === "split") await sessions.startTask(project, body);
    else await sessions.start(project, body, null, mode === "research" ? "research" : "direct");
    rememberMode(project, mode);
    text = "";
  } catch (e) {
    const why = e instanceof CoreError ? e.error.message : e instanceof Error ? e.message : String(e);
    error = `Could not start: ${why}`;
  } finally {
    busy = false;
  }
}

/** Enter starts, Shift+Enter starts a new line. */
function onkeydown(event: KeyboardEvent): void {
  if (event.code !== "Enter" && event.code !== "NumpadEnter") return;
  if (event.shiftKey || event.isComposing || event.keyCode === 229) return;
  event.preventDefault();
  void start();
}
</script>

<form
  class="compose"
  data-testid="focus-compose"
  onsubmit={(event) => {
    event.preventDefault();
    void start();
  }}
>
  <label class="visually-hidden" for="focus-task">Next task</label>
  <textarea id="focus-task" rows="2" placeholder="Give the agents a task" bind:value={text} {onkeydown}></textarea>
  <div class="acts">
    <label class="visually-hidden" for="focus-mode">How</label>
    <select id="focus-mode" bind:value={mode} data-testid="focus-mode">
      {#each MODES as option (option.id)}<option value={option.id}>{option.label}</option>{/each}
    </select>
    <button type="submit" class="primary" disabled={busy || !text.trim()} data-testid="focus-start">Start</button>
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</form>

<style>
  .compose {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border-radius: var(--r-lg);
    background: var(--s2);
  }
  textarea {
    box-sizing: border-box;
    width: 100%;
    padding: 6px 10px;
    color: var(--text);
    background: var(--s1);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    resize: vertical;
    font: inherit;
  }
  .acts {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  select,
  button {
    min-height: var(--control-h);
    padding: 0 12px;
    color: var(--text);
    background: var(--s3);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    font: inherit;
  }
  .primary:not(:disabled) {
    color: var(--accent-ink);
    background: var(--accent);
    font-weight: 500;
  }
  .error {
    margin: 0;
    color: var(--danger);
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
