<script lang="ts">
// Agent sessions of the open project: start one (one agent here, or a task
// planned and split among workers), pick one, read its transcript, reply and
// stop. Ctrl+Enter (Cmd+Enter) sends from a text field.
import { untrack } from "svelte";
import { browserPicks, pickLabel } from "../browser/picks.svelte";
import { registerCommand } from "../keys/commands";
import { formatShortcut, isApple } from "../keys/shortcut";
import type { FileChange, SessionId } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";
import { sandboxBadge, sandboxGaps, sandboxLine, sandboxState } from "./sandbox";
import { agentSessions, openReviews, usageText } from "./sessions.svelte";
import AccountPicker from "./setup/AccountPicker.svelte";
import { claudeAccounts } from "./setup/accounts.svelte";
import ClaudeSetup from "./setup/ClaudeSetup.svelte";
import ProjectTrust from "./setup/ProjectTrust.svelte";
import { paidText } from "./setup/texts";
import { isBusy, isEnded } from "./status";
import Transcript from "./Transcript.svelte";

const { transport, workspace } = useOvra();
const sessions = agentSessions(transport);
const picks = browserPicks(transport);
const accounts = claudeAccounts(transport);
const modEnter = formatShortcut("Mod+Enter", isApple());

type Mode = "here" | "split";
const MODES: { id: Mode; label: string; hint: string }[] = [
  {
    id: "here",
    label: "One agent, here",
    hint: "Claude works directly in this folder. You approve risky steps as it goes.",
  },
  {
    id: "split",
    label: "Plan and split",
    hint: "Claude plans the work and gives parts to up to 3 agents, each on its own copy of the project. You review their changes before anything is merged.",
  },
];

let selected = $state<SessionId | null>(null);
let prompt = $state("");
let reply = $state("");
let busy = $state(false);
let error = $state<string | null>(null);
/** Why the last start failed, shown next to the form; `setup` adds a pointer
 * to the Claude setup card above. */
let startError = $state<{ text: string; setup: boolean } | null>(null);
/** Started from the form and not yet past starting: its failure is a start error. */
let launched = $state<SessionId | null>(null);
/** How the next task runs; the last choice stays for this window. */
let mode = $state<Mode>("here");
let promptField = $state<HTMLTextAreaElement>();
/** The account the picker shows; null while there is only one. */
let account = $state<string | null>(null);

const project = $derived(workspace.project);
const list = $derived(project ? sessions.grouped(project.id) : []);
const current = $derived(list.find((s) => s.id === selected) ?? null);
const messages = $derived(current ? sessions.transcripts.get(current.id) : undefined);
/** Elements picked in the session's browser, sent with the next reply. */
const chips = $derived(current ? picks.of(current.id) : []);
const sandbox = $derived(current ? sandboxLine(sandboxState(current), isEnded(current.status)) : null);
const gaps = $derived(current ? sandboxGaps(sandboxState(current), isEnded(current.status)) : null);
const modeHint = $derived(MODES.find((m) => m.id === mode)?.hint);
/** A direct session's own files once its turn is over; a worker's live in its worktree, so Reviews shows them. */
const changed = $derived(
  current && current.mode === "direct" && sessions.roles.get(current.id)?.kind !== "worker" && !isBusy(current.status)
    ? sessions.changes(current.id)
    : [],
);
/** A task's workers whose changes wait in Reviews. */
const waiting = $derived(
  current && sessions.roles.get(current.id)?.kind === "coordinator" ? sessions.waitingInTask(current.id) : 0,
);

$effect(() => {
  const id = project?.id;
  if (!id) return;
  untrack(() => {
    selected = null;
    error = null;
    startError = null;
    launched = null;
    sessions.load(id).catch((err: unknown) => (error = `Could not list sessions: ${text(err)}`));
  });
});

// Pick the newest session when none is chosen.
$effect(() => {
  if (!current && list.length > 0) selected = list[list.length - 1]?.id ?? null;
});

// A session that fails before it gets going (claude missing, say) failed to start.
$effect(() => {
  const status = list.find((s) => s.id === launched)?.status;
  if (!status || status.state === "starting") return;
  if (status.state === "failed") startError = { text: `Claude could not start: ${status.message}`, setup: true };
  launched = null;
});

// Sessions that existed before this window: fetch their transcript once.
$effect(() => {
  const id = current?.id;
  if (id && !messages) untrack(() => sessions.loadTranscript(id).catch(() => {}));
});

$effect(() => {
  const off = [
    registerCommand({ id: "agents.new_session", title: "New task for Claude", run: () => promptField?.focus() }),
    registerCommand({
      id: "agents.interrupt",
      title: "Interrupt the agent's turn",
      run: () => {
        if (current && busyAgent) void interrupt(current.id);
      },
    }),
    registerCommand({
      id: "agents.stop",
      title: "Stop the agent",
      run: () => {
        if (current && !isEnded(current.status)) void stop(current.id);
      },
    }),
  ];
  return () => {
    for (const unregister of off) unregister();
  };
});

/** An added file has no diff to show yet (git does not track it): it opens instead. */
function showChange(file: FileChange): void {
  const id = project?.id;
  if (!id) return;
  if (file.kind === "added") workspace.openFile(file.path);
  else workspace.gitDiff = { project: id, path: file.path, staged: false };
}

function changeText(file: FileChange): string {
  if (file.kind === "added") return "new";
  if (file.kind === "deleted") return "deleted";
  return `+${file.additions} -${file.deletions}`;
}

function workerName(id: SessionId): string | undefined {
  const role = sessions.roles.get(id);
  return role?.kind === "worker" ? role.name : undefined;
}

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

async function start(): Promise<void> {
  const id = project?.id;
  const body = prompt.trim();
  if (!id || !body || busy) return;
  busy = true;
  startError = null;
  try {
    // A task's coordinator and workers use the project's account: make it
    // the picked one before the task starts.
    if (account && account !== accounts.projectAccount) await accounts.setProject(id, account);
    selected = mode === "here" ? (await sessions.start(id, body, account)).id : await sessions.startTask(id, body);
    launched = selected;
    prompt = "";
  } catch (err) {
    // A conflict (another agent already works here) is not a setup problem.
    const setup = !(err instanceof CoreError && err.error.code === "conflict");
    startError = { text: `Could not start: ${text(err)}`, setup };
  } finally {
    busy = false;
  }
}

function send(): Promise<void> {
  const id = current?.id;
  const body = reply.trim();
  if (!id || !body) return Promise.resolve();
  return run(async () => {
    // The session's next message carries the picks.
    await picks.attach(id);
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
  <!-- Until the first task the column reads as the first run: this line, Claude's
       setup, the project's settings, then the task field. -->
  {#if !project}
    <p class="intro">Open a project to start an agent.</p>
  {:else if list.length === 0}
    <p class="intro" data-testid="agents-empty">Give Claude a task. It asks before risky steps.</p>
  {/if}
  <ClaudeSetup />
  {#if project}
    <ProjectTrust />
    <form
      class="new"
      onsubmit={(event) => {
        event.preventDefault();
        void start();
      }}
    >
      <label for="agent-prompt" class="ask">What should Claude do?</label>
      <textarea
        id="agent-prompt"
        rows="3"
        placeholder="e.g. Fix the failing login test"
        bind:this={promptField}
        bind:value={prompt}
        aria-describedby="agent-mode-hint"
        onkeydown={(event) => submitOnModEnter(event, start)}
      ></textarea>
      <fieldset class="modes">
        <legend class="visually-hidden">How Claude works on it</legend>
        {#each MODES as option (option.id)}
          <label class="mode">
            <input type="radio" name="agent-mode" value={option.id} bind:group={mode} aria-describedby="agent-mode-hint" />
            {option.label}
          </label>
        {/each}
      </fieldset>
      <p class="hint" id="agent-mode-hint">{modeHint}</p>
      <div class="row">
        <span class="hint">{modEnter} starts.</span>
        <AccountPicker project={project.id} bind:account />
        <button type="submit" class="primary" disabled={busy || !prompt.trim()}>Start</button>
      </div>
      {#if startError}
        <p class="error" role="alert" data-testid="agent-start-error">
          {startError.text}
          {#if startError.setup}
            <span class="fix">If Claude is not set up yet, the Claude section above says what to do; then press Start again.</span>
          {/if}
        </p>
      {/if}
    </form>
    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}
    {#if list.length > 0}
      <h3 id="sessions-heading">Tasks</h3>
      <ul class="sessions" aria-labelledby="sessions-heading">
        {#each list as session (session.id)}
          {@const role = sessions.roles.get(session.id)}
          {@const badge = sandboxBadge(sandboxState(session))}
          <li>
            <button
              type="button"
              class:worker={role?.kind === "worker"}
              aria-current={session.id === current?.id ? "true" : undefined}
              title={role?.kind === "worker"
                ? `Branch ${role.branch}${role.kept ? `. Its folder was kept: ${role.kept}` : ""}`
                : undefined}
              onclick={() => (selected = session.id)}
            >
              <span class="title">{sessions.name(session.id)}</span>
              {#if badge}
                <span class="badge" class:unsandboxed={badge.warning} data-testid="agent-sandbox-badge">{badge.text}</span>
              {/if}
              <span class="state" data-state={session.status.state}>{sessions.status(session.id)}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
    {#if current}
      <div class="detail" data-testid="agent-session">
        <div class="row">
          <h3>{sessions.name(current.id)}</h3>
          <div class="buttons">
            {#if busyAgent}
              <button type="button" disabled={busy} onclick={() => interrupt(current.id)}>Interrupt</button>
            {/if}
            {#if !isEnded(current.status)}
              <button type="button" disabled={busy} onclick={() => stop(current.id)}>Stop</button>
            {/if}
          </div>
        </div>
        <p class="status" role="status">{sessions.status(current.id)}</p>
        {#if current.usage.input_tokens + current.usage.output_tokens > 0}
          <p class="status" data-testid="agent-usage">Total: {usageText(current.usage, current.billing)}</p>
        {/if}
        {#if current.billing}
          <!-- An API key bills at API prices, often not what a plan user expects. -->
          <p class={current.billing.kind === "api_key" ? "status billed-key" : "status"} data-testid="agent-billing">
            This agent{accounts.list.length > 1 ? ` (${accounts.name(current.account)} account)` : ""}: {paidText(
              current.billing,
            )}
          </p>
        {/if}
        {#if sandbox}
          <p class={sandbox.warning ? "status unsandboxed" : "status"} data-testid="agent-sandbox">{sandbox.text}</p>
        {/if}
        {#if gaps}
          <p class="status" data-testid="agent-sandbox-gaps">{gaps}</p>
        {/if}
        <Transcript messages={messages ?? []} billing={current.billing} {workerName} />
        {#if changed.length > 0}
          <!-- One agent here changes the folder itself: what it changed, each
               file's diff one press away, and where to keep or undo it. -->
          <section class="outcome" aria-labelledby="agent-changes-heading" data-testid="agent-changes">
            <h4 id="agent-changes-heading">Changed files ({changed.length})</h4>
            <ul aria-labelledby="agent-changes-heading">
              {#each changed as file (file.path)}
                <li>
                  <button type="button" class="file" onclick={() => showChange(file)}>{file.path}</button>
                  <span class="hint">{changeText(file)}</span>
                </li>
              {/each}
            </ul>
            <div class="row">
              <span class="hint">Already in your folder. Keep or undo them in Reviews.</span>
              <button type="button" onclick={openReviews}>Review</button>
            </div>
          </section>
        {:else if waiting > 0}
          <div class="outcome row" data-testid="agent-reviews">
            <span>{waiting === 1 ? "1 change waits" : `${waiting} changes wait`} for your review.</span>
            <button type="button" onclick={openReviews}>Review</button>
          </div>
        {/if}
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
            {#if chips.length > 0}
              <ul class="chips" aria-label="Picked elements, sent with the reply">
                {#each chips as { pick } (pick.pick_id)}
                  <li class="chip" data-testid="pick-chip" title={pick.note || pick.selector}>
                    <span>{pickLabel(pick)}</span>
                    <button
                      type="button"
                      aria-label={`Remove ${pickLabel(pick)}`}
                      onclick={() => picks.remove(current.id, pick.pick_id)}>×</button
                    >
                  </li>
                {/each}
              </ul>
            {/if}
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
              <span class="hint">Claude picks up where it stopped. {modEnter} resumes.</span>
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
  .intro,
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
  .intro,
  .status,
  label {
    color: #b4b4c0;
    font-size: 12px;
  }
  /* The column's main action: the setup cards above stay quieter. */
  .new {
    margin-top: 4px;
  }
  label.ask {
    color: #e6e6e6;
    font-size: 13px;
    font-weight: 600;
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
    /* In the panel's flex column an overflowing box would shrink to nothing. */
    flex: none;
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
  .sessions button.worker {
    /* A task's workers sit under its coordinator. */
    padding-left: 22px;
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
  .billed-key,
  .unsandboxed {
    color: #ffd36b;
  }
  .badge {
    flex: none;
    margin-left: auto;
    font-size: 11px;
    color: #b4b4c0;
  }
  .badge.unsandboxed {
    color: #ffd36b;
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
  .outcome {
    margin: 8px 12px 0;
    padding: 6px 8px;
    font-size: 12px;
    background: #1b1b21;
    border: 1px solid #4a4a57;
    border-radius: 3px;
  }
  .outcome h4 {
    margin: 0 0 4px;
    font-size: 12px;
  }
  .outcome ul {
    list-style: none;
    margin: 0 0 4px;
    padding: 0;
  }
  .outcome li {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .outcome button.file {
    padding: 1px 4px;
    color: #9db8ff;
    background: transparent;
    border-color: transparent;
    text-decoration: underline;
    overflow-wrap: anywhere;
    text-align: left;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .chip {
    display: flex;
    align-items: center;
    gap: 4px;
    max-width: 100%;
    padding: 0 0 0 8px;
    font-size: 12px;
    background: #1f2b1f;
    border: 1px solid #3f6b3f;
    border-radius: 10px;
  }
  .chip span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip button {
    padding: 0 6px;
    border: none;
    background: transparent;
  }
  .error {
    color: #ff8f8f;
    font-size: 12px;
    margin: 0;
  }
  .fix {
    display: block;
    color: #b4b4c0;
  }
  .new .hint {
    margin: 0;
  }
  .modes {
    display: flex;
    margin: 4px 0 0;
    padding: 0;
    border: none;
  }
  .mode {
    flex: 1;
    position: relative;
    padding: 4px 8px;
    text-align: center;
    color: #e6e6e6;
    background: #1b1b21;
    border: 1px solid #4a4a57;
    cursor: pointer;
  }
  .mode:first-of-type {
    border-radius: 3px 0 0 3px;
  }
  .mode:last-of-type {
    border-left: none;
    border-radius: 0 3px 3px 0;
  }
  .mode:has(input:checked) {
    background: #2f3a5c;
    border-color: #7aa2ff;
  }
  .mode:has(input:focus-visible) {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  /* The native radio keeps arrow keys and screen readers; the label is the button. */
  .mode input {
    position: absolute;
    opacity: 0;
    inset: 0;
    margin: 0;
    cursor: pointer;
  }
  button.primary:not(:disabled) {
    background: #3b5bdb;
    border-color: #5c7cfa;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  @media (forced-colors: active) {
    .sessions button[aria-current="true"],
    .mode:has(input:checked) {
      border: 2px solid Highlight;
    }
  }
</style>
