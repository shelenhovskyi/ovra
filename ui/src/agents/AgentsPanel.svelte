<script lang="ts">
// Agent sessions of the open project: start one, pick one, read its
// transcript, reply and stop. Ctrl+Enter (Cmd+Enter) sends from a text field.
import { untrack } from "svelte";
import { registerCommand } from "../keys/commands";
import { formatShortcut, isApple } from "../keys/shortcut";
import type { SessionId } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";
import { agentSessions, sessionName, usageText } from "./sessions.svelte";
import ClaudeSetup from "./setup/ClaudeSetup.svelte";
import ProjectTrust from "./setup/ProjectTrust.svelte";
import { isEnded, statusLabel } from "./status";
import Transcript from "./Transcript.svelte";

const { transport, workspace } = useOvra();
const sessions = agentSessions(transport);
const modEnter = formatShortcut("Mod+Enter", isApple());

let selected = $state<SessionId | null>(null);
let prompt = $state("");
let reply = $state("");
let busy = $state(false);
let error = $state<string | null>(null);
let promptField = $state<HTMLTextAreaElement>();

const project = $derived(workspace.project);
const list = $derived(project ? sessions.inProject(project.id) : []);
const current = $derived(list.find((s) => s.id === selected) ?? null);
const messages = $derived(current ? sessions.transcripts.get(current.id) : undefined);

$effect(() => {
  const id = project?.id;
  if (!id) return;
  untrack(() => {
    selected = null;
    error = null;
    sessions.load(id).catch((err: unknown) => (error = `Could not list sessions: ${text(err)}`));
  });
});

// Pick the newest session when none is chosen.
$effect(() => {
  if (!current && list.length > 0) selected = list[list.length - 1]?.id ?? null;
});

// Sessions that existed before this window: fetch their transcript once.
$effect(() => {
  const id = current?.id;
  if (id && !messages) untrack(() => sessions.loadTranscript(id).catch(() => {}));
});

$effect(() => {
  const off = [
    registerCommand({ id: "agents.new_session", title: "New agent session", run: () => promptField?.focus() }),
    registerCommand({
      id: "agents.interrupt",
      title: "Interrupt the agent's turn",
      run: () => {
        if (current && busyAgent) void interrupt(current.id);
      },
    }),
    registerCommand({
      id: "agents.stop",
      title: "Stop the agent session",
      run: () => {
        if (current && !isEnded(current.status)) void stop(current.id);
      },
    }),
  ];
  return () => {
    for (const unregister of off) unregister();
  };
});

function text(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}

async function run(action: () => Promise<void>): Promise<void> {
  if (busy) return;
  busy = true;
  error = null;
  try {
    await action();
  } catch (err) {
    error = text(err);
  } finally {
    busy = false;
  }
}

function start(): Promise<void> {
  const id = project?.id;
  const body = prompt.trim();
  if (!id || !body) return Promise.resolve();
  return run(async () => {
    const info = await sessions.start(id, body);
    selected = info.id;
    prompt = "";
  });
}

function send(): Promise<void> {
  const id = current?.id;
  const body = reply.trim();
  if (!id || !body) return Promise.resolve();
  return run(async () => {
    await sessions.send(id, body);
    reply = "";
  });
}

function resume(): Promise<void> {
  const id = current?.id;
  const body = reply.trim();
  if (!id || !body) return Promise.resolve();
  return run(async () => {
    await sessions.resume(id, body);
    reply = "";
  });
}

function interrupt(id: SessionId): Promise<void> {
  return run(() => sessions.interrupt(id));
}

function stop(id: SessionId): Promise<void> {
  return run(() => sessions.stop(id));
}

function submitOnModEnter(event: KeyboardEvent, submit: () => Promise<void>): void {
  if (event.code === "Enter" && (event.ctrlKey || event.metaKey) && !event.isComposing) {
    event.preventDefault();
    void submit();
  }
}

/** Working or waiting for an approval: a turn is in progress. */
const busyAgent = $derived(
  current !== null && (current.status.state === "working" || current.status.state === "waiting_for_approval"),
);
const canReply = $derived(current !== null && current.status.state === "idle");
</script>

<section class="agents" aria-labelledby="agents-heading" data-testid="agents-panel">
  <h2 id="agents-heading">Agents</h2>
  <ClaudeSetup />
  {#if !project}
    <p class="empty">Open a project to start an agent.</p>
  {:else}
    <ProjectTrust />
    <form
      class="new"
      onsubmit={(event) => {
        event.preventDefault();
        void start();
      }}
    >
      <label for="agent-prompt">Task for a new Claude session</label>
      <textarea
        id="agent-prompt"
        rows="3"
        bind:this={promptField}
        bind:value={prompt}
        aria-describedby="agent-prompt-hint"
        onkeydown={(event) => submitOnModEnter(event, start)}
      ></textarea>
      <div class="row">
        <span class="hint" id="agent-prompt-hint">Works in the project folder. {modEnter} starts.</span>
        <button type="submit" disabled={busy || !prompt.trim()}>Start</button>
      </div>
    </form>
    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}
    {#if list.length > 0}
      <h3 id="sessions-heading">Sessions</h3>
      <ul class="sessions" aria-labelledby="sessions-heading">
        {#each list as session (session.id)}
          <li>
            <button
              type="button"
              aria-current={session.id === current?.id ? "true" : undefined}
              onclick={() => (selected = session.id)}
            >
              <span class="title">{sessionName(session, session.id)}</span>
              <span class="state" data-state={session.status.state}>{statusLabel(session.status)}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
    {#if current}
      <div class="detail" data-testid="agent-session">
        <div class="row">
          <h3>{sessionName(current, current.id)}</h3>
          <div class="buttons">
            {#if busyAgent}
              <button type="button" disabled={busy} onclick={() => interrupt(current.id)}>Interrupt</button>
            {/if}
            {#if !isEnded(current.status)}
              <button type="button" disabled={busy} onclick={() => stop(current.id)}>Stop</button>
            {/if}
          </div>
        </div>
        <p class="status" role="status">{statusLabel(current.status)}</p>
        {#if current.usage.input_tokens + current.usage.output_tokens > 0}
          <p class="status" data-testid="agent-usage">Total: {usageText(current.usage)}</p>
        {/if}
        <Transcript messages={messages ?? []} />
        {#if !isEnded(current.status)}
          <form
            class="reply"
            onsubmit={(event) => {
              event.preventDefault();
              void send();
            }}
          >
            <label for="agent-reply">Reply to Claude</label>
            <textarea
              id="agent-reply"
              rows="2"
              bind:value={reply}
              onkeydown={(event) => submitOnModEnter(event, send)}
            ></textarea>
            <div class="row">
              <span class="hint">{canReply ? `${modEnter} sends.` : "Claude is busy; reply when it waits for you."}</span>
              <button type="submit" disabled={busy || !canReply || !reply.trim()}>Send</button>
            </div>
          </form>
        {:else if current.resumable}
          <form
            class="reply"
            onsubmit={(event) => {
              event.preventDefault();
              void resume();
            }}
          >
            <label for="agent-resume">Resume with a message</label>
            <textarea
              id="agent-resume"
              rows="2"
              bind:value={reply}
              onkeydown={(event) => submitOnModEnter(event, resume)}
            ></textarea>
            <div class="row">
              <span class="hint">Continues this session where it ended. {modEnter} resumes.</span>
              <button type="submit" disabled={busy || !reply.trim()}>Resume</button>
            </div>
          </form>
        {/if}
      </div>
    {/if}
  {/if}
</section>

<style>
  .agents {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    padding: 8px 0 0;
    /* Setup and trust cards above must not squeeze the transcript: scroll instead. */
    overflow-y: auto;
  }
  h2,
  h3 {
    margin: 0 12px 6px;
    font-size: 13px;
  }
  .detail h3 {
    margin: 0;
    overflow-wrap: anywhere;
  }
  form,
  .detail > .row,
  .status,
  .empty,
  .error {
    margin: 0 12px 8px;
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .buttons {
    display: flex;
    gap: 6px;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .hint,
  .empty,
  .status,
  label {
    color: #b4b4c0;
    font-size: 12px;
  }
  textarea {
    font: inherit;
    color: inherit;
    background: #121216;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    resize: vertical;
  }
  button {
    font: inherit;
    padding: 3px 10px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.55;
    cursor: default;
  }
  button:focus-visible,
  textarea:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  .sessions {
    list-style: none;
    margin: 0 0 8px;
    padding: 0 12px;
    max-height: 30vh;
    overflow: auto;
  }
  .sessions button {
    width: 100%;
    display: flex;
    justify-content: space-between;
    gap: 8px;
    text-align: left;
    background: transparent;
    border-color: transparent;
  }
  .sessions button[aria-current="true"] {
    background: #2a2a33;
    border-color: #7aa2ff;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .state {
    flex: none;
    font-size: 12px;
    color: #b4b4c0;
  }
  .state[data-state="waiting_for_approval"] {
    color: #ffd36b;
  }
  .state[data-state="failed"] {
    color: #ff8f8f;
  }
  .detail {
    display: flex;
    flex-direction: column;
    flex: none;
    border-top: 1px solid #2a2a31;
    padding-top: 8px;
  }
  .reply {
    margin-top: 8px;
  }
  .error {
    color: #ff8f8f;
  }
  @media (forced-colors: active) {
    .sessions button[aria-current="true"] {
      border: 2px solid Highlight;
    }
  }
</style>
