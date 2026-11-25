<script lang="ts">
// Agent sessions of the open project: a compact list of tasks with a start
// box under it; a row opens its transcript, where the person replies, stops
// and closes it. Ctrl+Enter (Cmd+Enter) sends from a text field.
import { tick, untrack } from "svelte";
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

const { transport, workspace } = useOvra();
const sessions = agentSessions(transport);
const picks = browserPicks(transport);
const accounts = claudeAccounts(transport);
const modEnter = formatShortcut("Mod+Enter", isApple());

const MODES: { id: TaskMode; label: string; hint: string }[] = [
  { id: "here", label: "One agent, here", hint: "Works in this folder. Asks before risky steps." },
  {
    id: "split",
    label: "Plan and split",
    hint: "Up to 3 agents at once, each on its own copy. You review before anything is merged.",
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
/** A task's workers whose changes wait in Reviews. */
const waiting = $derived(
  current && sessions.roles.get(current.id)?.kind === "coordinator" ? sessions.waitingInTask(current.id) : 0,
);
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
  if (!isBusy(s.status) && (sessions.changes(s.id).length > 0 || sessions.waitingInTask(s.id) > 0))
    return { text: "Review", tone: "ok" };
  return null;
}

/** A session that is done or failed can be closed; one in a turn first needs a stop. */
function closable(s: SessionInfo): boolean {
  return !isBusy(s.status) && s.status.state !== "queued";
}

function tone(s: SessionInfo): string {
  if (s.status.state === "failed" || sessions.status(s.id).startsWith("Failed")) return "failed";
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

function close(id: SessionId): Promise<void> {
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
    {#if current}
      <button type="button" class="quiet new-task" onclick={newTask}>{@render icon("M12 5v14M5 12h14")}New task</button>
    {/if}
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
          <button type="button" class="quiet" onclick={() => current && openReviews(sessions.workers(current.id))}
            >Review</button
          >
        </div>
      {/if}
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
              {/if}
            </li>
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
        rows="3"
        placeholder="What should Claude do?"
        bind:this={promptField}
        bind:value={prompt}
        aria-describedby="agent-mode-hint"
        onkeydown={(event) => submitOnModEnter(event, start)}
      ></textarea>
      <fieldset class="modes">
        <legend class="visually-hidden">How Claude works on it</legend>
        {#each MODES as option (option.id)}
          <label class="mode">
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
      <p class="line" id="agent-mode-hint">{modeHint}</p>
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
          title={`${modEnter} starts`}
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
