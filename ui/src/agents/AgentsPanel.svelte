<script lang="ts">
// Agent sessions of the open project: a compact list of tasks with a start
// box under it; a row opens its transcript, where the person replies, stops
// and closes it. Ctrl+Enter (Cmd+Enter) sends from a text field.
import { type Snippet, tick, untrack } from "svelte";
import { describeTool } from "../approvals/describe";
import { browserPicks, pickLabel } from "../browser/picks.svelte";
import { registerCommand } from "../keys/commands";
import { formatShortcut, isApple } from "../keys/shortcut";
import type { AccountState, FileChange, SessionId, SessionInfo } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";
import { signedOutError } from "./errors";
import { sandboxBadge, sandboxGaps, sandboxLine, sandboxState } from "./sandbox";
import { agentSessions, openReviews, usageText } from "./sessions.svelte";
import AccountPicker from "./setup/AccountPicker.svelte";
import { signInCommand } from "./setup/account-texts";
import ClaudeSetup from "./setup/ClaudeSetup.svelte";
import { claudeAccounts, DEFAULT_ACCOUNT } from "./setup/claude-accounts.svelte";
import ProjectTrust from "./setup/ProjectTrust.svelte";
import { billedShort, paidText } from "./setup/texts";
import { isBusy, isEnded } from "./status";
import Transcript from "./Transcript.svelte";
import { rememberedMode, rememberMode, type TaskMode } from "./task-mode";

/** `controls`: the shell's buttons for the panel (swap, collapse), drawn at the header's end. */
let { controls }: { controls?: Snippet } = $props();

const { transport, workspace } = useOvra();
const sessions = agentSessions(transport);
const picks = browserPicks(transport);
const accounts = claudeAccounts(transport);
const modEnter = formatShortcut("Mod+Enter", isApple());

/** `hint`: a few words under the options; `more`: the whole of it, on hover. */
const MODES: { id: TaskMode; label: string; hint: string; more: string }[] = [
  { id: "here", label: "One agent, here", hint: "Works in this folder", more: "Asks before risky steps." },
  {
    id: "split",
    label: "Plan and split",
    hint: "Up to 3 agents; you review",
    more: "Each works on its own copy. You review before anything is merged.",
  },
];

/** The session whose transcript is open; null shows the list. */
let selected = $state<SessionId | null>(null);
let prompt = $state("");
let reply = $state("");
let busy = $state(false);
let error = $state<string | null>(null);
/** Why the last start failed; `setup` points to the Claude card. */
let startError = $state<{ text: string; setup: boolean } | null>(null);
/** Started from the form and not yet past starting: its failure is a start error. */
let launched = $state<SessionId | null>(null);
/** What `launched` was asked to do. */
let launchedPrompt = "";
/** How the next task runs; the last choice is kept per project. */
let mode = $state<TaskMode>("here");
let promptField = $state<HTMLTextAreaElement>();
let replyField = $state<HTMLTextAreaElement>();
/** The account the picker shows; null while there is only one. */
let account = $state<string | null>(null);
/** Transcripts asked for to tell a failed turn from a done one. */
const asked = new Set<SessionId>();

const project = $derived(workspace.project);
const list = $derived(project ? sessions.shown(project.id) : []);
const current = $derived(list.find((s) => s.id === selected) ?? null);
const messages = $derived(current ? sessions.transcripts.get(current.id) : undefined);
/** Elements picked in the session's browser, sent with the next reply. */
const chips = $derived(current ? picks.of(current.id) : []);
const sandbox = $derived(current ? sandboxLine(sandboxState(current), isEnded(current.status)) : null);
const gaps = $derived(current ? sandboxGaps(sandboxState(current), isEnded(current.status)) : null);
const modeHint = $derived(MODES.find((m) => m.id === mode)?.hint);
/** The open session cannot go on: its account is signed out. */
const signedOut = $derived(current ? signedOutError(current.status, messages) : null);
/** A signed-in account to start over with, other than the one that failed. */
const fallback = $derived.by((): AccountState | undefined => {
  const ready = accounts.signedIn;
  return ready && ready.account.id !== (current?.account ?? DEFAULT_ACCOUNT) ? ready : undefined;
});
/** The account a new task would start with is known to be signed out. */
const blocked = $derived(accounts.signedOut(account ?? accounts.projectAccount));
const usable = $derived(accounts.signedIn);
/** A direct session's own files once its turn is over; a worker's live in its worktree, so Reviews shows them. */
const changed = $derived(
  current && current.mode === "direct" && sessions.roles.get(current.id)?.kind !== "worker" && !isBusy(current.status)
    ? sessions.changes(current.id)
    : [],
);
/** A task's workers, or a worker itself, whose changes wait in Reviews. */
const waiting = $derived.by(() => {
  const kind = current ? sessions.roles.get(current.id)?.kind : undefined;
  if (!current || !kind) return 0;
  if (kind === "coordinator") return sessions.waitingInTask(current.id);
  return sessions.waitsForReview(current.id) ? 1 : 0;
});
/** The task the person pressed Close on: it asks once before closing it with its workers. */
let confirming = $state<SessionId | null>(null);
const capacity = $derived(project ? sessions.capacity.get(project.id) : undefined);

$effect(() => {
  const id = project?.id;
  if (!id) return;
  untrack(() => {
    selected = null;
    mode = rememberedMode(id);
    error = null;
    startError = null;
    launched = null;
    sessions.load(id).catch((err: unknown) => (error = `Could not list sessions: ${text(err)}`));
  });
});

// A session that fails before it gets going (claude missing, say) failed to
// start: back to the start box, with the prompt to try again.
$effect(() => {
  const failed = list.find((s) => s.id === launched);
  if (!failed || failed.status.state === "starting") return;
  if (failed.status.state === "failed") {
    startError = { text: `Claude could not start: ${failed.status.message}`, setup: true };
    untrack(() => {
      if (selected === failed.id) selected = null;
      if (!prompt) prompt = launchedPrompt;
    });
  }
  launched = null;
});

// Sessions that existed before this window (or a resync): fetch the open
// one's transcript.
$effect(() => {
  const id = current?.id;
  if (id && !messages) untrack(() => sessions.loadTranscript(id).catch(() => {}));
});

// And once those of sessions whose last turn may have failed, so their row says so.
$effect(() => {
  const wanted = list.filter((s) => s.status.state === "idle" || s.status.state === "stopped");
  untrack(() => {
    for (const s of wanted)
      if (!sessions.transcripts.has(s.id) && !asked.has(s.id)) {
        asked.add(s.id);
        sessions.loadTranscript(s.id).catch(() => asked.delete(s.id));
      }
  });
});

$effect(() => {
  const off = [
    registerCommand({ id: "agents.new_session", title: "New task for Claude", run: newTask }),
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

async function newTask(): Promise<void> {
  selected = null;
  await tick();
  promptField?.focus();
}

function open(id: SessionId | null): void {
  selected = id;
  error = null;
  reply = "";
}

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

/** What a working session does now: its newest tool call, in words. */
function step(id: SessionId): string | null {
  const transcript = sessions.transcripts.get(id) ?? [];
  for (let i = transcript.length - 1; i >= 0; i--) {
    const message = transcript[i];
    if (message?.kind === "tool_use") return describeTool(message.name, message.input).action;
    if (message?.kind === "user_text") return null;
  }
  return null;
}

/** The row's call to action, if the person is needed. */
function chip(s: SessionInfo): { text: string; tone: "warn" | "ok" } | null {
  if (s.status.state === "waiting_for_approval") return { text: "Approve", tone: "warn" };
  if (!isBusy(s.status) && (sessions.waitsForReview(s.id) || sessions.waitingInTask(s.id) > 0))
    return { text: "Review", tone: "ok" };
  return null;
}

/** A session that is done or failed can be closed; one in a turn first needs a stop. */
function closable(s: SessionInfo): boolean {
  return !isBusy(s.status) && s.status.state !== "queued";
}

function tone(s: SessionInfo): string {
  const label = sessions.status(s.id);
  if (s.status.state === "failed" || label.startsWith("Failed")) return "failed";
  // A worker stopped with its task after it finished is done.
  if (label === "Done") return "idle";
  return s.status.state;
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

/** Starts `body` with the account `use` (null: the project's) and opens it. */
async function begin(body: string, how: TaskMode, use: string | null): Promise<SessionId> {
  const id = project?.id;
  if (!id) throw new Error("No project is open");
  // A task's coordinator and workers use the project's account: make it
  // the picked one before the task starts.
  if (use && use !== accounts.projectAccount) await accounts.setProject(id, use);
  const started = how === "here" ? (await sessions.start(id, body, use)).id : await sessions.startTask(id, body);
  selected = started;
  await tick();
  replyField?.focus();
  return started;
}

async function start(): Promise<void> {
  const body = prompt.trim();
  if (!project || !body || busy || blocked) return;
  busy = true;
  startError = null;
  try {
    launched = await begin(body, mode, account);
    launchedPrompt = body;
    prompt = "";
  } catch (err) {
    // A conflict (another agent already works here) is not a setup problem.
    const setup = !(err instanceof CoreError && err.error.code === "conflict");
    startError = { text: `Could not start: ${text(err)}`, setup };
  } finally {
    busy = false;
  }
}

/** Starts the signed-out session's task again with a signed-in account and closes the old one. */
function retry(old: SessionInfo, use: AccountState): Promise<void> {
  const first = sessions.transcripts.get(old.id)?.find((m) => m.kind === "user_text" && m.from === "person");
  const body = first?.kind === "user_text" ? first.text : old.title;
  const how: TaskMode = sessions.roles.get(old.id)?.kind === "coordinator" ? "split" : "here";
  return run(async () => {
    await begin(body, how, use.account.id);
    await sessions.close(old.id);
  });
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

/** Closes `id`; a task (a coordinator) asks first, since its workers close with it. */
function close(id: SessionId): Promise<void> {
  if (sessions.roles.get(id)?.kind === "coordinator" && confirming !== id) {
    confirming = id;
    return Promise.resolve();
  }
  confirming = null;
  return run(async () => {
    await sessions.close(id);
    if (selected === id) selected = null;
  });
}

function copy(value: string): void {
  navigator.clipboard?.writeText(value).catch(() => {});
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

{#snippet icon(path: string)}
  <svg class="icon" viewBox="0 0 24 24" aria-hidden="true"><path d={path} /></svg>
{/snippet}

{#snippet confirmClose(id: SessionId)}
  {@const workers = sessions.workers(id).length}
  {@const kept = sessions.waitingInTask(id)}
  <div class="confirm" role="group" aria-label="Close this task?" data-testid="agents-close-confirm">
    <p>
      Close this task{workers > 0 ? ` and its ${workers === 1 ? "worker" : `${workers} workers`}` : ""}?{kept > 0
        ? " Their changes stay in Reviews."
        : ""}
    </p>
    <span class="buttons">
      <button type="button" class="quiet" onclick={() => (confirming = null)}>Cancel</button>
      <button type="button" class="quiet strong" disabled={busy} onclick={() => close(id)}>Close task</button>
    </span>
  </div>
{/snippet}

<section class="agents" aria-labelledby="agents-heading" data-testid="agents-panel">
  <header class="head">
    <h2 id="agents-heading">Agents</h2>
    {#if capacity}
      <span class="capacity" data-testid="agents-capacity" title="Agents working in this project, of how many may">
        {capacity.working} of {capacity.limit}{capacity.queued > 0 ? `, ${capacity.queued} waiting` : ""}
      </span>
      <span class="slots" aria-hidden="true">
        {#each { length: capacity.limit } as _, n (n)}<i class:on={n < capacity.working}></i>{/each}
      </span>
    {/if}
    <span class="end">
      {#if current}
        <button
          type="button"
          class="quiet icon-only"
          title="New task"
          aria-label="New task"
          onclick={newTask}>{@render icon("M12 5v14M5 12h14")}</button
        >
      {/if}
      {@render controls?.()}
    </span>
  </header>

  {#if !project}
    <p class="intro">Open a project to start an agent.</p>
  {:else if current}
    <div class="detail" data-testid="agent-session">
      <div class="bar">
        <button type="button" class="quiet back" onclick={() => open(null)}>{@render icon("M15 18l-6-6 6-6")}Tasks</button>
        <span class="buttons">
          {#if busyAgent}
            <button type="button" class="quiet" disabled={busy} onclick={() => interrupt(current.id)}>Interrupt</button>
          {/if}
          {#if !isEnded(current.status)}
            <button type="button" class="quiet" disabled={busy} onclick={() => stop(current.id)}>Stop</button>
          {/if}
          {#if closable(current)}
            <button
              type="button"
              class="quiet"
              disabled={busy}
              title="Hide it from the list; its record stays on this computer"
              onclick={() => close(current.id)}>Close</button
            >
          {/if}
        </span>
      </div>
      {#if confirming === current.id}{@render confirmClose(current.id)}{/if}
      <!-- What the session is, and what waits: it gives way (and scrolls) before the transcript does. -->
      <div class="info">
        <div class="facts">
          <h3>{sessions.name(current.id)}</h3>
          <p class="status" role="status" data-state={tone(current)}>{sessions.status(current.id)}</p>
          {#if current.billing}
            <!-- An API key bills at API prices, often not what a plan user expects. -->
            <p class="line" class:warn={current.billing.kind === "api_key"} data-testid="agent-billing">
              {accounts.list.length > 1 ? `${accounts.name(current.account)} · ` : ""}{billedShort(current.billing)}
            </p>
          {/if}
          {#if sandbox}
            <p class="line" class:warn={sandbox.warning} data-testid="agent-sandbox">{sandbox.text}</p>
          {/if}
          {#if gaps}
            <p class="line" data-testid="agent-sandbox-gaps">{gaps}</p>
          {/if}
          {#if current.usage.input_tokens + current.usage.output_tokens > 0 || current.billing}
            <details class="more">
              <summary>Details</summary>
              {#if current.billing}<p>{paidText(current.billing)}</p>{/if}
              {#if current.usage.input_tokens + current.usage.output_tokens > 0}
                <p data-testid="agent-usage">Total: {usageText(current.usage, current.billing)}</p>
              {/if}
            </details>
          {/if}
        </div>
        {#if signedOut}
          {@const name = accounts.name(current.account)}
          {@const command = signInCommand(accounts.state(current.account)?.account.config_dir ?? null)}
          <div class="card signed-out" role="alert" data-testid="agent-signed-out">
            <p class="title">{name} account signed out</p>
            {#if fallback}
              <button type="button" class="primary" disabled={busy} onclick={() => current && retry(current, fallback)}
                >Use {fallback.account.name}</button
              >
            {/if}
            <p class="line">{fallback ? "Or sign in" : "Sign in"} from a terminal:</p>
            <p class="command">
              <code data-testid="agent-sign-in">{command}</code>
              <button type="button" class="quiet" onclick={() => copy(command)}>Copy</button>
            </p>
            <details class="more">
              <summary>Details</summary>
              <p>{signedOut}</p>
            </details>
          </div>
        {/if}
        {#if error}
          <p class="error" role="alert">{error}</p>
        {/if}
        {#if changed.length > 0}
          <!-- One agent here changes the folder itself: what it changed, each
               file's diff one press away, and where to keep or undo it. -->
          <section class="card outcome" aria-labelledby="agent-changes-heading" data-testid="agent-changes">
            <h4 id="agent-changes-heading">Changed files ({changed.length})</h4>
            <ul aria-labelledby="agent-changes-heading">
              {#each changed as file (file.path)}
                <li>
                  <button type="button" class="file" onclick={() => showChange(file)}>{file.path}</button>
                  <span class="line">{changeText(file)}</span>
                </li>
              {/each}
            </ul>
            <div class="row">
              <span class="line">Already in your folder.</span>
              <button type="button" class="quiet" onclick={() => current && openReviews([current.id])}>Review</button>
            </div>
          </section>
        {:else if waiting > 0}
          <div class="card outcome row" data-testid="agent-reviews">
            <span>{waiting === 1 ? "1 change waits" : `${waiting} changes wait`} for your review.</span>
            <button
              type="button"
              class="quiet"
              onclick={() =>
                current &&
                openReviews(
                  sessions.roles.get(current.id)?.kind === "coordinator" ? sessions.workers(current.id) : [current.id],
                )}
              >Review</button
            >
          </div>
        {/if}
      </div>
      <Transcript messages={messages ?? []} billing={current.billing} {workerName} />
      {#if !isEnded(current.status) && !signedOut}
        <form
          class="compose"
          onsubmit={(event) => {
            event.preventDefault();
            void send();
          }}
        >
          <label for="agent-reply" class="visually-hidden">Reply to Claude</label>
          <textarea
            id="agent-reply"
            rows="2"
            placeholder={canReply ? "Reply to Claude" : "Claude is working…"}
            bind:this={replyField}
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
            <span class="line">{canReply ? `${modEnter} sends` : ""}</span>
            <button type="submit" class="primary" disabled={busy || !canReply || !reply.trim()}>Send</button>
          </div>
        </form>
      {:else if current.resumable && !signedOut}
        <form
          class="compose"
          onsubmit={(event) => {
            event.preventDefault();
            void resume();
          }}
        >
          <label for="agent-resume" class="visually-hidden">Resume with a message</label>
          <textarea
            id="agent-resume"
            rows="2"
            placeholder="Resume with a message"
            bind:value={reply}
            onkeydown={(event) => submitOnModEnter(event, resume)}
          ></textarea>
          <div class="row">
            <span class="line">Picks up where it stopped</span>
            <button type="submit" class="primary" disabled={busy || !reply.trim()}>Resume</button>
          </div>
        </form>
      {/if}
    </div>
  {:else}
    <div class="scroll">
      {#if list.length === 0}
        <p class="intro" data-testid="agents-empty">No tasks yet.</p>
      {:else}
        <ul class="sessions" aria-label="Tasks">
          {#each list as session (session.id)}
            {@const role = sessions.roles.get(session.id)}
            {@const badge = sandboxBadge(sandboxState(session))}
            {@const action = chip(session)}
            {@const now = session.status.state === "working" ? step(session.id) : null}
            <li class:worker={role?.kind === "worker"}>
              <button
                type="button"
                class="row-button"
                data-state={tone(session)}
                title={role?.kind === "worker"
                  ? `${sessions.name(session.id)}. Branch ${role.branch}${role.kept ? `. Its folder was kept: ${role.kept}` : ""}`
                  : sessions.name(session.id)}
                onclick={() => open(session.id)}
              >
                <span class="state-mark" aria-hidden="true"></span>
                <span class="text">
                  <span class="title">{sessions.name(session.id)}</span>
                  <span class="state">{#if now}<span class="visually-hidden">Working: </span>{now}{:else}{sessions.status(
                        session.id,
                      )}{/if}</span>
                </span>
                {#if action}<span class="pill {action.tone}">{action.text}</span>{/if}
                {#if badge?.warning}
                  <span class="pill warn" data-testid="agent-sandbox-badge">{badge.text}</span>
                {/if}
              </button>
              {#if closable(session)}
                <button
                  type="button"
                  class="quiet close"
                  aria-label={`Close ${sessions.name(session.id)}`}
                  title="Close"
                  disabled={busy}
                  onclick={() => close(session.id)}>{@render icon("M6 6l12 12M18 6L6 18")}</button
                >
              {:else}
                <span class="close-space" aria-hidden="true"></span>
              {/if}
            </li>
            {#if confirming === session.id}<li class="confirm-row">{@render confirmClose(session.id)}</li>{/if}
          {/each}
        </ul>
      {/if}
      {#if error}
        <p class="error" role="alert">{error}</p>
      {/if}
      <ClaudeSetup />
      <ProjectTrust />
    </div>
    <form
      class="compose new"
      onsubmit={(event) => {
        event.preventDefault();
        void start();
      }}
    >
      <label for="agent-prompt" class="visually-hidden">What should Claude do?</label>
      <textarea
        id="agent-prompt"
        rows="2"
        placeholder="What should Claude do?"
        bind:this={promptField}
        bind:value={prompt}
        aria-describedby="agent-mode-hint"
        onkeydown={(event) => submitOnModEnter(event, start)}
      ></textarea>
      <fieldset class="modes">
        <legend class="visually-hidden">How Claude works on it</legend>
        {#each MODES as option (option.id)}
          <label class="mode" title={`${option.hint}. ${option.more}`}>
            <input
              type="radio"
              name="agent-mode"
              value={option.id}
              bind:group={mode}
              aria-describedby="agent-mode-hint"
              onchange={() => project && rememberMode(project.id, option.id)}
            />
            {option.label}
          </label>
        {/each}
      </fieldset>
      <!-- Shown while the box is in use; screen readers always hear it with the options. -->
      <p class="line hint" class:shown={prompt.trim() !== ""} id="agent-mode-hint">{modeHint}</p>
      {#if blocked}
        <p class="blocked" role="note" data-testid="agent-account-blocked">
          <span
            >{accounts.list.length > 1 ? accounts.name(account ?? accounts.projectAccount) : "Claude"} is signed out.</span
          >
          {#if usable}
            <button type="button" class="quiet" onclick={() => (account = usable.account.id)}
              >Use {usable.account.name}</button
            >
          {/if}
        </p>
      {/if}
      <div class="row">
        <AccountPicker project={project.id} bind:account />
        <button
          type="submit"
          class="primary"
          title={prompt.trim() ? `${modEnter} starts` : "Type a task first"}
          disabled={busy || blocked || !prompt.trim()}>Start</button
        >
      </div>
      {#if startError}
        <p class="error" role="alert" data-testid="agent-start-error">
          {startError.text}{#if startError.setup}{" "}<span class="fix">See Claude above, then try again.</span>{/if}
        </p>
      {/if}
    </form>
  {/if}
</section>

<style>
  .agents {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    background: var(--s1);
  }
  .head {
    display: flex;
    flex: none;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 8px 8px 4px 16px;
  }
  h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }
  .capacity {
    font-size: 13px;
    color: var(--text-2);
    white-space: nowrap;
  }
  .slots {
    display: inline-flex;
    flex: none;
    gap: 3px;
  }
  .slots i {
    width: 14px;
    height: 4px;
    border-radius: 2px;
    background: var(--s4);
  }
  .slots i.on {
    background: var(--accent);
  }
  /* New task and the shell's buttons share the header's end. */
  .end {
    display: flex;
    flex: none;
    align-items: center;
    gap: 2px;
    margin-left: auto;
  }
  .icon-only {
    width: 28px;
    padding: 0;
    justify-content: center;
  }
  p {
    margin: 0;
  }
  .intro {
    padding: 8px 16px;
    color: var(--text-2);
  }
  .line,
  .status {
    font-size: 13px;
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .warn {
    color: var(--warn);
  }
  .error {
    margin: 0 16px 8px;
    font-size: 13px;
    color: var(--danger);
  }
  .compose .error {
    margin: 0;
  }
  .icon {
    width: 15px;
    height: 15px;
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  /* Buttons: one filled action per view, the rest quiet. */
  button {
    font: inherit;
    color: var(--text);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  /* A primary that cannot be used is plainly a disabled button, not a pale accent one. */
  .primary:disabled {
    opacity: 1;
    color: var(--text-2);
    background: var(--s3);
  }
  .quiet {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 8px;
    font-size: 13px;
    color: var(--text-2);
  }
  .quiet:hover:not(:disabled) {
    color: var(--text);
    background: var(--s3);
  }
  .primary {
    height: 32px;
    padding: 0 14px;
    font-weight: 500;
    color: var(--accent-ink);
    background: var(--accent);
  }

  /* The list */
  .scroll {
    flex: 1;
    min-height: 72px;
    overflow-y: auto;
    padding: 0 8px 8px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .sessions {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .sessions li {
    position: relative;
    display: flex;
    align-items: flex-start;
    border-radius: 10px;
  }
  .sessions li:hover {
    background: var(--s2);
  }
  .sessions li.worker {
    margin-left: 20px;
  }
  .row-button {
    flex: 1;
    min-width: 0;
    display: grid;
    grid-template-columns: 16px 1fr auto;
    align-items: start;
    column-gap: 10px;
    padding: 10px;
    text-align: left;
    border-radius: 10px;
  }
  .row-button .pill + .pill {
    grid-column: 3;
    margin-top: 4px;
  }
  .state-mark {
    width: 9px;
    height: 9px;
    margin: 6px 0 0 3px;
    border-radius: 50%;
    background: var(--s4);
  }
  [data-state="working"] .state-mark,
  [data-state="starting"] .state-mark {
    width: 14px;
    height: 14px;
    margin: 3px 0 0 1px;
    background: none;
    border: 2px solid var(--accent-soft);
    border-top-color: var(--accent);
    animation: spin 0.9s linear infinite;
  }
  [data-state="waiting_for_approval"] .state-mark {
    background: var(--warn);
  }
  [data-state="idle"] .state-mark {
    background: var(--ok);
  }
  [data-state="failed"] .state-mark {
    background: var(--danger);
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .state-mark {
      animation: none !important;
    }
  }
  .text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .title {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .state {
    font-size: 13px;
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  [data-state="failed"] .state {
    color: var(--danger);
  }
  [data-state="waiting_for_approval"] .state {
    color: var(--warn);
  }
  .pill {
    display: inline-flex;
    align-items: center;
    height: 22px;
    padding: 0 8px;
    border-radius: var(--radius-sm);
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
  }
  .pill.warn {
    color: var(--warn);
    background: var(--warn-soft);
  }
  .pill.ok {
    color: var(--ok);
    background: var(--ok-soft);
  }
  /* Close has its own column, so it never covers a badge; muted like the
     rest of the chrome, at full strength on hover or focus. */
  .close,
  .close-space {
    flex: none;
    width: 28px;
    margin: 8px 4px 0 0;
  }
  .close {
    padding: 0;
    justify-content: center;
    opacity: 0.6;
  }
  .sessions li:hover .close,
  .close:focus-visible {
    opacity: 1;
  }
  .confirm {
    margin: 0 8px 8px;
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
    background: var(--s2);
    border-radius: var(--radius);
  }
  .sessions .confirm {
    flex: 1;
    margin: 0;
  }
  .confirm .buttons {
    justify-content: flex-end;
  }
  .strong {
    color: var(--text);
    font-weight: 500;
  }

  /* One session */
  .detail {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .info {
    flex: 0 1 auto;
    max-height: 45%;
    min-height: 0;
    overflow-y: auto;
  }
  .bar {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    padding: 0 8px;
  }
  .buttons {
    display: flex;
    gap: 2px;
  }
  .facts {
    padding: 4px 16px 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  h3 {
    margin: 0 0 2px;
    font-size: 14px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .status[data-state="failed"] {
    color: var(--danger);
  }
  .status[data-state="waiting_for_approval"] {
    color: var(--warn);
  }
  .more summary {
    width: fit-content;
    font-size: 13px;
    color: var(--text-2);
    cursor: pointer;
  }
  .more p {
    font-size: 13px;
    color: var(--text-2);
  }
  .card {
    margin: 0 8px 8px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    background: var(--s2);
    border-radius: var(--radius);
  }
  .signed-out .title {
    font-weight: 600;
    color: var(--warn);
  }
  .command {
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
  }
  code {
    min-width: 0;
    padding: 4px 8px;
    font-size: 13px;
    background: var(--s3);
    border-radius: var(--radius-sm);
    user-select: all;
    overflow-wrap: anywhere;
  }
  .outcome h4 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }
  .outcome ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .outcome li {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .file {
    padding: 0;
    color: var(--accent);
    text-align: left;
    overflow-wrap: anywhere;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
  }

  /* Text boxes: the start box and the reply box */
  .compose {
    flex: none;
    margin: 8px;
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--s2);
    border-radius: 12px;
  }
  textarea {
    font: inherit;
    color: var(--text);
    background: none;
    border: none;
    resize: none;
    padding: 2px;
  }
  textarea:focus-visible {
    outline: none;
  }
  /* The start box gives way last: the list keeps a few rows, and in a short
     column the box scrolls on its own rather than falling under the edge. */
  .compose.new {
    flex: 0 1 auto;
    min-height: 0;
    overflow-y: auto;
  }
  .compose.new:not(:focus-within) .hint:not(.shown) {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .compose:focus-within {
    box-shadow: 0 0 0 2px var(--accent-soft);
  }
  .compose .row {
    flex-wrap: wrap;
  }
  .compose .row .primary {
    margin-left: auto;
  }
  .modes {
    display: flex;
    gap: 2px;
    margin: 0;
    padding: 2px;
    border: none;
    background: var(--s1);
    border-radius: var(--radius-sm);
  }
  .mode {
    flex: 1;
    position: relative;
    padding: 4px 8px;
    text-align: center;
    font-size: 13px;
    color: var(--text-2);
    border-radius: 5px;
    cursor: pointer;
  }
  .mode:has(input:checked) {
    color: var(--text);
    font-weight: 500;
    background: var(--s3);
  }
  .mode:has(input:focus-visible) {
    outline: 2px solid var(--accent);
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
  .blocked {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    font-size: 13px;
    color: var(--warn);
  }
  .fix {
    color: var(--text-2);
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
    font-size: 13px;
    color: var(--ok);
    background: var(--ok-soft);
    border-radius: 10px;
  }
  .chip span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip button {
    padding: 0 6px;
    color: inherit;
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
    .compose,
    .card,
    .pill {
      border: 1px solid CanvasText;
    }
    .mode:has(input:checked) {
      border: 2px solid Highlight;
    }
    .close {
      opacity: 1;
    }
  }
</style>
