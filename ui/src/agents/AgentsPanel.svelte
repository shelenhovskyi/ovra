<script lang="ts" module>
import { registerContextKeys } from "../keys/context-keys";

// This panel's own keys, for Settings > Keyboard; the panel handles them itself.
registerContextKeys(
  [
    { keys: "Enter", title: "Start the task, or send the reply" },
    { keys: "Shift+Enter", title: "New line" },
  ].map((key) => ({ ...key, where: "Agents" })),
);
</script>

<script lang="ts">
// Agent sessions of the open project: a compact list of tasks with a start
// box under it; a row opens its transcript, where the person replies, stops
// and closes it. Enter sends from a text field (Ctrl+Enter or Cmd+Enter
// too); Shift+Enter starts a new line.
import KeepAwakeButton from "../power/KeepAwakeButton.svelte";
import { tick, untrack } from "svelte";
import { describeTool } from "../approvals/describe";
import { approvalQueue } from "../approvals/queue.svelte";
import Board from "../board/Board.svelte";
import { taskBoards } from "../board/boards.svelte";
import { sessionBrowsers } from "../browser/browser.svelte";
import { browserPicks, pickLabel } from "../browser/picks.svelte";
import { listCommands, registerCommand } from "../keys/commands";
import type { AccountState, ContextItem, FileChange, SessionId, SessionInfo } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { isMenuKey, type MenuEntry, openContextMenu } from "../menu/context-menu.svelte";
import { projectSandbox } from "../settings/sandbox";
import { useOvra } from "../shell/context";
import Icon from "../shell/Icon.svelte";
import { menuTarget } from "../shell/menu";
import ContextInput from "./ContextInput.svelte";
import { agentAsks, contextDrafts, type Suggestion } from "./context.svelte";
import { resultText, undoneTurns } from "./checkpoints";
import { signedOutError } from "./errors";
import FirstSteps from "./FirstSteps.svelte";
import { hints } from "./hints.svelte";
import { agentPrefs } from "./prefs.svelte";
import Leftovers from "./Leftovers.svelte";
import { chatLink } from "./links";
import { diskLabel, pressureNote, resourcesLine, resourcesTip } from "./resources";
import { listWarning, sandboxGaps, sandboxLine, sandboxState } from "./sandbox";
import { agentSessions, openReviews, usageText } from "./sessions.svelte";
import AccountPicker from "./setup/AccountPicker.svelte";
import { billedShort, paidText, signInCommand } from "./setup/claude-texts";
import { setupAsks } from "./setup/asks.svelte";
import AgentSetup from "./setup/AgentSetup.svelte";
import { claudeAccounts, DEFAULT_ACCOUNT } from "./setup/claude-accounts.svelte";
import ProjectTrust from "./setup/ProjectTrust.svelte";
import { isBusy, isEnded } from "./status";
import { providers } from "./providers.svelte";
import { inFolder } from "./steps";
import Transcript from "./Transcript.svelte";
import WorkerSetup from "./WorkerSetup.svelte";
import { secretLooking } from "./worktrees";
import { REPORT_DIR, reportFile, reportOf, reportPath, taskPrompt } from "./report";
import { rememberedMode, rememberMode, type TaskMode } from "./task-mode";

const { transport, workspace } = useOvra();
const sessions = agentSessions(transport);
const picks = browserPicks(transport);
const accounts = claudeAccounts(transport);
const names = providers(transport);
const approvals = approvalQueue(transport);
const drafts = contextDrafts(transport);
const browsers = sessionBrowsers(transport);
const boards = taskBoards(transport);

/** `more`: what choosing it means, in the menu; the hint under the box adds the sandbox. */
const MODES: { id: TaskMode; label: string; more: string }[] = [
  { id: "here", label: "One agent, here", more: "Works in this folder. Asks before risky steps." },
  {
    id: "split",
    label: "Plan and split",
    more: "Up to 3 agents, each on its own copy. You review before anything is merged.",
  },
  { id: "research", label: "Research", more: "Reads the project and searches the web. Changes nothing." },
];

/** The session whose transcript is open; null shows the list. */
let selected = $state<SessionId | null>(null);
let prompt = $state("");
let reply = $state("");
let busy = $state(false);
let error = $state<string | null>(null);
/** Why the last start failed; `setup` points to the agent's setup card. */
let startError = $state<{ text: string; setup: boolean } | null>(null);
/** Started from the form and not yet past starting: its failure is a start error. */
let launched = $state<SessionId | null>(null);
/** What `launched` was asked to do. */
let launchedPrompt = "";
/** How the next task runs; the last choice is kept per project. */
let mode = $state<TaskMode>("here");
let promptField = $state<HTMLTextAreaElement>();
/** The agent's setup card is open although the agent is ready (from the start form's footer button). */
let setupOpen = $state(false);
let replyField = $state<HTMLTextAreaElement>();
/** The @ menus and chips of the start box and the reply box. */
let taskContext = $state<ContextInput>();
let replyContext = $state<ContextInput>();
/** The account the picker shows; null while there is only one. */
let account = $state<string | null>(null);
/** Transcripts asked for to tell a failed turn from a done one. */
const asked = new Set<SessionId>();

const project = $derived(workspace.project);
/** The new-task box's chips are the project's own. */
const newBox = $derived(`new:${workspace.project?.id ?? ""}`);
const list = $derived(project ? sessions.shown(project.id) : []);
/** How many of the project's agents work, of how many may. */
const capacity = $derived(project ? sessions.capacity.get(project.id) : undefined);
/** Why queued workers wait although a place may be free. */
const pressure = $derived(project ? sessions.pressure.get(project.id) : undefined);
/** The disk the project's worktrees use, once counted. */
const projectDisk = $derived(project ? sessions.disk.get(project.id) : undefined);
/** "No sandbox" for the whole list, once under the header. */
const unsandboxed = $derived(listWarning(list));
const current = $derived(list.find((s) => s.id === selected) ?? null);
const messages = $derived(current ? sessions.transcripts.get(current.id) : undefined);
/** The open task's board, if the open session is a task's coordinator. */
const currentTask = $derived.by(() => {
  const role = current ? sessions.roles.get(current.id) : undefined;
  return role?.kind === "coordinator" ? role.task : null;
});
/** A task shows its conversation or its board; a task opens on its conversation. */
let tab = $state<"talk" | "board">("talk");
/** The open session's id: unlike `current`, it changes only when another session opens. */
const currentId = $derived(current?.id ?? null);
$effect.pre(() => {
  void currentId;
  tab = "talk";
});

/** Left and Right move between a task's tabs, as in any tab list. */
function tabKeys(event: KeyboardEvent): void {
  if (event.code !== "ArrowLeft" && event.code !== "ArrowRight") return;
  event.preventDefault();
  tab = tab === "talk" ? "board" : "talk";
  void tick().then(() => document.getElementById(`task-tab-${tab}`)?.focus());
}
// Every listed task's board is watched, so a question for the person shows on its row.
$effect(() => {
  for (const s of list) {
    const role = sessions.roles.get(s.id);
    if (role?.kind === "coordinator") boards.watch(role.task);
  }
});
/** Elements picked in the session's browser, sent with the next reply. */
const chips = $derived(current ? picks.of(current.id) : []);
/** The open session's browser pages, offered as screenshots in the reply's @ menu. */
const pageSuggestions = $derived(
  current
    ? browsers.of(current.id).map(
        (page): Suggestion => ({ what: { kind: "page", page: page.id }, label: "Screenshot", detail: page.title || page.url }),
      )
    : [],
);
const sandbox = $derived(current ? sandboxLine(sandboxState(current), isEnded(current.status)) : null);
const gaps = $derived(current ? sandboxGaps(sandboxState(current), isEnded(current.status)) : null);
/** How the project's newest session that said so runs: the next one most likely runs the same way. */
const sandboxWord = $derived.by(() => {
  const known = projectSandbox(list);
  return known ? { sandboxed: ", in a sandbox", none: ", without a sandbox" }[known] : "";
});
/** The line under the header says "No sandbox" already: the hint does not say it again. */
const modeSandbox = $derived(unsandboxed ? "" : sandboxWord);
const modeHint = $derived(
  mode === "here"
    ? `Works in this folder${modeSandbox}. Asks before risky steps.`
    : mode === "research"
      ? `Reads this folder and searches the web${modeSandbox}. Changes nothing.`
      : `Up to 3 agents on their own copies${modeSandbox}. You review first.`,
);
/** The mode menu is open. */
let modeMenu = $state(false);
/** Mod+Enter came with nothing typed: the field says what it needs. */
let typeFirst = $state(false);
/** The task row the list's one Tab stop is on; null: the first. */
let rowFocus = $state<SessionId | null>(null);
/** Focus is in the start box. */
let startFocus = $state(false);
/**
 * Tasks run and requests wait: the start box folds to its field, until
 * focused or holding text, so the list and the requests keep the room.
 */
const foldStart = $derived(
  list.length > 0 &&
    approvals.pending.length > 0 &&
    !startFocus &&
    !modeMenu &&
    !prompt.trim() &&
    !startError &&
    drafts.of(newBox).length === 0,
);
let modeList = $state<HTMLElement>();
/** Projects known not to be git repositories: Plan and split cannot run there. */
let noRepo = $state<string | null>(null);
const noGit = $derived(mode === "split" && project !== null && noRepo === project.id);
/** The open session cannot go on: its account is signed out. */
const signedOut = $derived(current ? signedOutError(current.status, messages) : null);
/** A signed-in account to start over with, other than the one that failed. */
const fallback = $derived.by((): AccountState | undefined => {
  const ready = accounts.signedIn;
  return ready && ready.account.id !== (current?.account ?? DEFAULT_ACCOUNT) ? ready : undefined;
});
/** The account a new task would start with is known to be signed out. */
const blocked = $derived(accounts.signedOut(account ?? accounts.projectAccount));
/** Every place is taken: One agent, here waits in line (Plan and split queues its workers). */
const atCap = $derived(mode === "here" && capacity !== undefined && capacity.working >= capacity.limit);
/** Research has places of its own (ADR 0013); all taken, or none in Settings. */
const researchFull = $derived(
  mode === "research" && capacity !== undefined && capacity.research_working >= capacity.research_limit,
);
/** Why Start cannot work right now, said before the press instead of after it. */
const startBlock = $derived(
  blocked
    ? "The account is signed out"
    : noGit
      ? "Plan and split needs git"
      : researchFull
        ? capacity?.research_limit === 0
          ? "Research is off in Settings"
          : `All ${capacity?.research_limit} research agents are busy`
        : null,
);
const usable = $derived(accounts.signedIn);
/** Who pays for the next task, in two or three words; it opens the agent's setup. */
const billing = $derived.by(() => {
  const state = accounts.state(account ?? accounts.preferred);
  const paid = state?.setup.billing;
  if (!paid) return "Ready";
  const many = accounts.list.length > 1 && state ? `${state.account.name} · ` : "";
  return `${many}${billedShort(paid)}`;
});
/** An API key bills the next task: the footer says it in the warning colour. */
const billingKey = $derived(accounts.state(account ?? accounts.preferred)?.setup.billing?.kind === "api_key");

/** Whether the project is a git repository. */
function checkGit(id: string): void {
  transport
    .call({ method: "git.status", params: { project: id } })
    .then((reply) => {
      noRepo = reply.type === "git.no_repo" ? id : noRepo === id ? null : noRepo;
    })
    .catch(() => {});
}

$effect(() => {
  if (prompt.trim()) typeFirst = false;
});

// Asked once Plan and split is chosen.
$effect(() => {
  const id = project?.id;
  if (!id || mode !== "split") return;
  untrack(() => checkGit(id));
});

/** The Git area's "Set up git" (`git init`), then the question again. */
async function setUpGit(id: string): Promise<void> {
  await listCommands()
    .find((c) => c.id === "git.init")
    ?.run();
  checkGit(id);
}

/** Where the menu opens: fixed, above its button, so the start box's own scrolling never clips it. */
let menuAt = $state({ left: 0, bottom: 0 });

async function toggleModes(open: boolean): Promise<void> {
  const box = document.querySelector("[data-testid=agent-mode]")?.getBoundingClientRect();
  if (box) menuAt = { left: box.left, bottom: window.innerHeight - box.top + 4 };
  modeMenu = open;
  if (!open) return;
  await tick();
  modeList?.querySelector<HTMLElement>("[aria-checked=true]")?.focus();
}

function pickMode(id: TaskMode): void {
  mode = id;
  modeMenu = false;
  if (project) rememberMode(project.id, id);
  document.querySelector<HTMLElement>("[data-testid=agent-mode]")?.focus();
}

function onModeKey(event: KeyboardEvent): void {
  const items = [...(modeList?.querySelectorAll<HTMLElement>("[role=menuitemradio]") ?? [])];
  if (event.code === "Escape" || event.code === "Tab") {
    if (event.code === "Escape") event.preventDefault();
    modeMenu = false;
    document.querySelector<HTMLElement>("[data-testid=agent-mode]")?.focus();
    return;
  }
  const next = menuTarget(event, items);
  if (next) {
    event.preventDefault();
    next.focus();
  }
}
/** A direct session's own files once its turn is over; a worker's live in its worktree, so Reviews shows them. */
const changed = $derived(
  current && current.mode === "direct" && sessions.roles.get(current.id)?.kind !== "worker" && !isBusy(current.status)
    ? sessions.changes(current.id)
    : [],
);
/** An idle research session's answer, for Save as doc and Turn into task. */
const report = $derived(
  current && current.mode === "research" && !isBusy(current.status) ? reportOf(messages) : null,
);
/** Where the open session's report was saved, while that report is the one shown. */
let savedReport = $state<{ session: SessionId; report: string; path: string } | null>(null);
const saved = $derived(
  savedReport && current && savedReport.session === current.id && savedReport.report === report
    ? savedReport.path
    : null,
);
/** The question the report answers: the person's first message. */
const question = $derived.by(() => {
  const first = messages?.find((m) => m.kind === "user_text" && m.from === "person");
  return first?.kind === "user_text" ? first.text : (current?.title ?? "Research");
});

/** Save as doc: a new file under docs/research/, written by the person's press, then opened. */
function saveReport(): Promise<void> {
  const id = current?.id;
  const text = report;
  const at = project?.id;
  if (!id || !text || !at) return Promise.resolve();
  return run(async () => {
    const now = new Date();
    for (let n = 1; n <= 20; n++) {
      const path = reportPath(question, now, n);
      try {
        await transport.call({ method: "fs.create", params: { project: at, path } });
      } catch (err) {
        if (err instanceof CoreError && err.error.code === "conflict") continue;
        throw err;
      }
      await transport.call({
        method: "fs.write_file",
        params: { project: at, path, text: reportFile(question, text), expected_mtime_ms: null },
      });
      savedReport = { session: id, report: text, path };
      workspace.openFile(path, "edit");
      return;
    }
    throw new Error(`every name for this report in ${REPORT_DIR} is taken`);
  });
}

/** Turn into task: the form switches to Plan and split with the findings quoted; the person says what to do and starts it. */
async function toTask(): Promise<void> {
  if (!report) return;
  prompt = taskPrompt(question, report);
  mode = "split";
  if (project) rememberMode(project.id, "split");
  selected = null;
  await tick();
  promptField?.focus();
  promptField?.setSelectionRange(prompt.length, prompt.length);
}

/** A task's workers, or a worker itself, whose changes wait in Reviews. */
const waiting = $derived.by(() => {
  const kind = current ? sessions.roles.get(current.id)?.kind : undefined;
  if (!current || !kind) return 0;
  if (kind === "coordinator") return sessions.waitingInTask(current.id);
  return sessions.waitsForReview(current.id) ? 1 : 0;
});
/** The session whose first "Done · Changed …" line shows; said once, then never again. */
let doneLine = $state<SessionId | null>(null);
$effect(() => {
  const id = current?.id;
  if (!id || changed.length === 0 || hints.has("done_line")) return;
  untrack(() => {
    doneLine = id;
    hints.mark("done_line");
  });
});
/** A task waits for review, and nobody opened a review yet: one line says what Review does. */
const reviewHint = $derived(!hints.has("reviewed") && list.some((s) => chip(s)?.tone === "ok"));

/** The first steps' "Check the work": the first task that asks the person, else the first whose changes wait. */
const asking = $derived(list.find((s) => chip(s)?.tone === "warn") ?? null);
const reviewable = $derived(asking ? null : (list.find((s) => chip(s)?.tone === "ok") ?? null));
/** With nothing working, asking or to review: the task that ended, for Get started's last step. */
const ended = $derived.by(() => {
  if (asking || reviewable || list.some((s) => isBusy(s.status) || s.status.state === "queued")) return undefined;
  const last = list.find((s) => ["idle", "stopped", "failed"].includes(s.status.state));
  return last ? { failed: last.status.state === "failed", open: () => open(last.id) } : undefined;
});

/** Opens a task that asks something and moves to its request, so Y or N answers it. */
async function answer(id: SessionId): Promise<void> {
  open(id);
  await tick();
  await listCommands()
    .find((c) => c.id === "approvals.focus")
    ?.run();
}

/** Opens Reviews on a task's changes: a task's all its workers', a session's its own. */
function reviewOf(id: SessionId): void {
  openReviews(sessions.roles.get(id)?.kind === "coordinator" ? sessions.workers(id) : [id]);
}

/** The task the person pressed Close on: it asks once before closing it with its workers. */
let confirming = $state<SessionId | null>(null);
/** Planning coordinators take no place, so the count says them apart. */
const planning = $derived(project ? sessions.planning(project.id) : 0);
/** Agents in a place that wait for the person's answer; coordinators take none. */
const needYou = $derived(
  list.filter((s) => s.status.state === "waiting_for_approval" && sessions.roles.get(s.id)?.kind !== "coordinator")
    .length,
);
/** Every place is held by an agent that waits for the person: the count says so instead of "3 of 3". */
const allWait = $derived(capacity !== undefined && capacity.working > 0 && needYou >= capacity.working);

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
    startError = { text: `${names.name(failed.agent)} could not start: ${failed.status.message}`, setup: true };
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
    registerCommand({ id: "agents.new_session", title: "New task", keys: "Mod+N", run: newTask }),
    registerCommand({
      id: "agents.add_account",
      title: "Use another account…",
      category: "Agents",
      keywords: ["account", "sign in", "work", "personal"],
      run: () => askSetup("account"),
    }),
    registerCommand({
      id: "agents.set_claude_path",
      title: "Set path to claude…",
      category: "Agents",
      keywords: ["executable", "install", "binary"],
      run: () => askSetup("path"),
    }),
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

async function openSetup(): Promise<void> {
  setupOpen = true;
  await tick();
  document.querySelector("[data-testid=claude-setup]")?.scrollIntoView?.({ block: "nearest" });
}

/** Opens the agent's setup card in the task list with the path field or the account form. */
async function askSetup(what: "path" | "account"): Promise<void> {
  selected = null;
  setupAsks[what] = true;
  await openSetup();
}

// Ask agent (the editor, a problem, the terminal): the item goes to the
// open box, the reply of the shown task or else a new task.
$effect(() => {
  const waiting = agentAsks.waiting;
  const id = project?.id;
  if (waiting.length === 0 || !id) return;
  untrack(() => {
    agentAsks.waiting = [];
    const replying = current !== null && !isEnded(current.status) && !signedOut;
    const target = replying && current ? current.id : newBox;
    if (!replying) selected = null;
    for (const what of waiting)
      drafts.add(id, target, what).catch((err: unknown) => {
        error = `Could not add it: ${text(err)}`;
      });
    void showBox(target === newBox ? () => promptField : () => replyField);
  });
});

/** Shows the panel, if folded, and focuses the field `field` returns. */
async function showBox(field: () => HTMLTextAreaElement | undefined): Promise<void> {
  if (document.querySelector("[data-testid=agents] .strip"))
    await listCommands()
      .find((c) => c.id === "layout.toggle_agents")
      ?.run();
  await tick();
  field()?.focus();
}

/** From a folded Agents column, New task unfolds it first, through the shell's own command. */
async function newTask(): Promise<void> {
  selected = null;
  await showBox(() => promptField);
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

/** Where a session works: a worker in its copy of the project, the others in the project. */
function folder(id: SessionId): string | null {
  const role = sessions.roles.get(id);
  return role?.kind === "worker" ? role.worktree : (project?.root ?? null);
}

/** A link the person clicked in what the agent said: a web page in the browser, a project file in the editor. */
function openLink(id: SessionId, href: string): void {
  const link = chatLink(href, project?.root ?? null, folder(id));
  if (link?.kind === "web") void transport.call({ method: "open.url", params: { url: link.url } }).catch(() => {});
  else if (link?.kind === "file")
    workspace.openFile(link.path, "view", link.line ? { line: link.line, from: 0, to: 0 } : undefined);
}

/** What a working session does now: its newest tool call, in words. */
function step(id: SessionId): string | null {
  const transcript = sessions.transcripts.get(id) ?? [];
  for (let i = transcript.length - 1; i >= 0; i--) {
    const message = transcript[i];
    if (message?.kind === "tool_use")
      return describeTool(message.name, message.input, inFolder(message.call, folder(id))).action;
    if (message?.kind === "user_text") return null;
  }
  return null;
}

/**
 * The row's call to action, if the person is needed. A task says how many
 * of its workers' changes wait; its workers do not say it again.
 */
function chip(s: SessionInfo): { text: string; tone: "warn" | "ok" } | null {
  if (s.status.state === "waiting_for_approval") return { text: "Approve", tone: "warn" };
  const role = sessions.roles.get(s.id);
  if (role?.kind === "coordinator" && boards.openForPerson(role.task).length > 0) return { text: "Answer", tone: "warn" };
  if (isBusy(s.status)) return null;
  const inTask = sessions.waitingInTask(s.id);
  if (inTask > 0) return { text: inTask > 1 ? `Review ${inTask}` : "Review", tone: "ok" };
  const told = role?.kind === "worker" && list.some((t) => t.id === role.coordinator && chip(t)?.tone === "ok");
  return sessions.waitsForReview(s.id) && !told ? { text: "Review", tone: "ok" } : null;
}

/**
 * A row's title: a worker under its task by its own name and what it was
 * asked to do (its first line), a task or a session by its title.
 */
function rowTitle(s: SessionInfo): string {
  const role = sessions.roles.get(s.id);
  if (role?.kind === "worker") return s.title ? `${role.name} · ${s.title}` : role.name;
  return s.title || sessions.name(s.id);
}

/** The row's tooltip: the whole title, and a worker's branch and kept folder. */
function rowTip(s: SessionInfo): string {
  const role = sessions.roles.get(s.id);
  if (role?.kind !== "worker") return sessions.name(s.id);
  const kept = role.kept ? `\nIts folder was kept: ${role.kept}` : "";
  return `${role.name}${s.title ? `: ${s.title}` : ""}\nBranch ${role.branch}${kept}`;
}

/** Needs the person first, then working, then queued, then the rest. */
function rank(s: SessionInfo): number {
  if (chip(s)) return 0;
  if (isBusy(s.status)) return 1;
  return s.status.state === "queued" ? 2 : 3;
}

/** Tasks in the order "needs you" first; a task's workers stay under it, in their order. */
const ordered = $derived.by(() => {
  const groups: SessionInfo[][] = [];
  for (const s of list) {
    const role = sessions.roles.get(s.id);
    const last = groups.at(-1);
    if (role?.kind === "worker" && last?.[0]?.id === role.coordinator) last.push(s);
    else groups.push([s]);
  }
  return groups
    .map((group, at) => ({ group, at, rank: Math.min(...group.map(rank)) }))
    .sort((a, b) => a.rank - b.rank || a.at - b.at)
    .flatMap((g) => g.group);
});
/** The task row that takes Tab: the last one focused while it is listed, else the first. */
const tabStop = $derived(ordered.some((s) => s.id === rowFocus) ? rowFocus : (ordered[0]?.id ?? null));

/** How many of a task's workers are done (or wait for review), of how many; null without workers. */
function progress(coordinator: SessionId): { done: number; total: number } | null {
  const workers = sessions.workers(coordinator);
  const total = workers.length + (sessions.failedStarts.get(coordinator)?.length ?? 0);
  if (total === 0) return null;
  const done = workers.filter((w) => sessions.waitsForReview(w) || sessions.status(w) === "Done").length;
  return { done, total };
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
async function begin(body: string, how: TaskMode, use: string | null, context: ContextItem[] = []): Promise<SessionId> {
  const id = project?.id;
  if (!id) throw new Error("No project is open");
  // A task's coordinator and workers use the project's account: make it
  // the picked one before the task starts.
  if (use && use !== accounts.projectAccount) await accounts.setProject(id, use);
  const started =
    how === "split"
      ? await sessions.startTask(id, body, context)
      : (await sessions.start(id, body, use, how === "research" ? "research" : "direct", context)).id;
  selected = started;
  await tick();
  replyField?.focus();
  return started;
}

async function start(): Promise<void> {
  const body = prompt.trim();
  if (!project || !body || busy || blocked) return;
  if (drafts.unconfirmed(newBox).length > 0) {
    startError = { text: "Choose Send anyway or remove the marked item first.", setup: false };
    return;
  }
  busy = true;
  startError = null;
  try {
    launched = await begin(body, mode, account, drafts.items(newBox));
    hints.mark("started");
    launchedPrompt = body;
    prompt = "";
    drafts.clear(newBox);
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
  const how: TaskMode =
    sessions.roles.get(old.id)?.kind === "coordinator" ? "split" : old.mode === "research" ? "research" : "here";
  return run(async () => {
    await begin(body, how, use.account.id);
    await sessions.close(old.id);
  });
}

function send(): Promise<void> {
  const id = current?.id;
  const body = reply.trim();
  if (!id || !body) return Promise.resolve();
  if (drafts.unconfirmed(id).length > 0) {
    error = "Choose Send anyway or remove the marked item first.";
    return Promise.resolve();
  }
  return run(async () => {
    // The session's next message carries the picks.
    await picks.attach(id);
    await sessions.send(id, body, drafts.items(id));
    reply = "";
    drafts.clear(id);
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

/** What the person last asked, to ask again after a turn that ended in an error. */
const lastAsk = $derived.by(() => {
  const found = [...(messages ?? [])].reverse().find((m) => m.kind === "user_text" && m.from === "person");
  return found?.kind === "user_text" ? found.text : null;
});

/** Retry on the error that ended the turn: the last request again, as a reply or by resuming. */
const retryTurn = $derived.by(() => {
  const id = current?.id;
  const body = lastAsk;
  if (!id || !body || signedOut || busy) return undefined;
  if (canReply) return () => void run(() => sessions.send(id, body));
  if (current?.resumable) return () => void run(() => sessions.resume(id, body));
  return undefined;
});

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
    // The question shows under the row, which may sit at the list's end.
    return tick().then(() => {
      document.querySelector("[data-testid=agents-close-confirm]")?.scrollIntoView?.({ block: "nearest" });
    });
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

/**
 * Mod+Enter starts the task from anywhere in the start box, the mode menu
 * too (with the mode in focus there); the field handles its own Enter.
 */
function startOnModEnter(event: KeyboardEvent): void {
  if (!(event.code === "Enter" || event.code === "NumpadEnter") || !(event.ctrlKey || event.metaKey)) return;
  if (event.target === promptField) return;
  event.preventDefault();
  const option = event.target instanceof HTMLElement ? event.target.dataset.mode : undefined;
  if (option === "here" || option === "split") pickMode(option);
  if (!prompt.trim()) {
    typeFirst = true;
    promptField?.focus();
  } else if (startBlock === null) void start();
}

/** Task rows that open a transcript, in screen order: the list's arrow keys move among them. */
function rowButtons(list: Element | null): HTMLElement[] {
  return [...(list?.querySelectorAll<HTMLElement>(".row-button[data-session]") ?? [])];
}

/** Closes a row's task and keeps focus in the list, on the row that takes its place. */
async function closeRow(row: HTMLElement, id: SessionId): Promise<void> {
  const list = row.closest(".sessions");
  const at = rowButtons(list).indexOf(row);
  await close(id);
  await tick();
  if (row.isConnected) return;
  const left = rowButtons(list);
  left[Math.min(at, left.length - 1)]?.focus();
}

/** Open, Stop and Close for a task row, from Shift+F10, the Menu key or a right click. */
function rowMenu(event: KeyboardEvent | MouseEvent, s: SessionInfo): void {
  const row = event.currentTarget instanceof HTMLElement ? event.currentTarget : null;
  const coordinator = sessions.roles.get(s.id)?.kind === "coordinator";
  const entries: MenuEntry[] = [
    { label: "Open", run: () => open(s.id) },
    "separator",
    !isEnded(s.status) &&
      sessions.status(s.id) !== "Done" && {
        // A waiting session never started: stopping it takes it out of line.
        label: s.status.state === "queued" ? "Cancel" : "Stop",
        run: () => stop(s.id),
      },
    closable(s) && {
      label: coordinator ? "Close…" : "Close",
      keys: "Delete",
      danger: true,
      run: () => (row ? closeRow(row, s.id) : close(s.id)),
    },
  ];
  openContextMenu(event, entries);
}

const ROW_MOVES = new Set(["ArrowDown", "ArrowUp", "Home", "End"]);

function onRowKey(event: KeyboardEvent, s: SessionInfo): void {
  const row = event.currentTarget as HTMLElement;
  if (isMenuKey(event)) {
    rowMenu(event, s);
  } else if (event.code === "Delete" && !event.ctrlKey && !event.metaKey && !event.altKey) {
    event.preventDefault();
    if (closable(s) && !busy) void closeRow(row, s.id);
  } else if (ROW_MOVES.has(event.code)) {
    event.preventDefault();
    const rows = rowButtons(row.closest(".sessions"));
    const at = rows.indexOf(row);
    const to = { ArrowDown: at + 1, ArrowUp: at - 1, Home: 0, End: rows.length - 1 }[event.code] ?? at;
    rows[Math.max(0, Math.min(to, rows.length - 1))]?.focus();
  }
}

/** Enter submits, as in Claude's own apps; Shift+Enter is a new line, and Enter that ends an IME composition is not a send. */
function submitOnEnter(event: KeyboardEvent, submit: () => Promise<void>): void {
  const composing = event.isComposing || event.keyCode === 229;
  if ((event.code === "Enter" || event.code === "NumpadEnter") && !event.shiftKey && !composing) {
    event.preventDefault();
    void submit();
  }
}

/** Working or waiting for an approval: a turn is in progress. */
const busyAgent = $derived(
  current !== null && (current.status.state === "working" || current.status.state === "waiting_for_approval"),
);
const canReply = $derived(current !== null && current.status.state === "idle");

/**
 * Going back to an earlier turn (`docs/specs/checkpoints.md`): for direct
 * sessions only, not a task's coordinator, and while a box takes the
 * message back.
 */
const goesBack = $derived(
  current !== null &&
    current.mode === "direct" &&
    !sessions.roles.has(current.id) &&
    (!isEnded(current.status) || current.resumable),
);
/** The open session's turns that can be gone back to. */
let checkpoints = $state<{ session: SessionId; turns: Set<number> } | null>(null);
// Asked again whenever a turn ends: a new checkpoint, or an old one dropped.
$effect(() => {
  const id = goesBack ? current?.id : undefined;
  const idle = current !== null && !isBusy(current.status);
  if (!id || !idle) return;
  void messages?.length;
  void sessions.checkpoints(id).then((turns) => {
    if (current?.id === id) checkpoints = { session: id, turns };
  });
});
/** What the last go-back or its undo did, until the transcript moves on. */
let wentBack = $state<{ session: SessionId; text: string; at: number } | null>(null);
const wentBackLine = $derived(
  wentBack && current && wentBack.session === current.id && wentBack.at === (messages?.length ?? 0)
    ? wentBack.text
    : null,
);
const undone = $derived(
  current ? undoneTurns(messages ?? [], current.undone_from, current.can_undo_go_back ?? false) : null,
);

function goBack(turn: number, text: string): Promise<void> {
  const id = current?.id;
  if (!id) return Promise.resolve();
  return run(async () => {
    const { restored, conflicts } = await sessions.goBack(id, turn);
    wentBack = { session: id, text: resultText(restored, conflicts, false), at: messages?.length ?? 0 };
    // The message comes back to be edited and sent again, or cleared.
    reply = text;
    await tick();
    replyField?.focus();
  });
}

function undoGoBack(): Promise<void> {
  const id = current?.id;
  if (!id) return Promise.resolve();
  return run(async () => {
    const { restored, conflicts } = await sessions.undoGoBack(id);
    wentBack = { session: id, text: resultText(restored, conflicts, true), at: messages?.length ?? 0 };
  });
}
/** Running, or a task whose workers still run: a Done session has nothing to stop, Close ends it. */
const stoppable = $derived(current !== null && !isEnded(current.status) && sessions.status(current.id) !== "Done");
</script>

{#snippet icon(path: string)}
  <svg class="icon" viewBox="0 0 24 24" aria-hidden="true"><path d={path} /></svg>
{/snippet}

{#snippet addContext(run: () => void)}
  <button
    type="button"
    class="quiet add-context"
    aria-label="Add context"
    title="Add a file, selection, problem or terminal output (@)"
    data-testid="add-context"
    onclick={run}><Icon name="at" small /></button
  >
{/snippet}

{#snippet confirmClose(id: SessionId)}
  {@const workers = sessions.workers(id).length}
  {@const kept = sessions.waitingInTask(id)}
  {@const working = sessions.workers(id).some((w) => isBusy(sessions.byId.get(w)?.status ?? null))}
  <div class="confirm" role="group" aria-label="Close this task?" data-testid="agents-close-confirm">
    <p>
      Close this task{workers > 0 ? ` and its ${workers === 1 ? "worker" : `${workers} workers`}` : ""}?{kept > 0
        ? " Their changes stay in Reviews."
        : ""}{working ? " Working ones stop." : ""}
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
      <span
        class="capacity"
        data-testid="agents-capacity"
        title={allWait
          ? `All ${capacity.working} agents wait for your answer`
          : "Agents working in this project, of how many may"}
      >
        {allWait
          ? `${needYou} ${needYou === 1 ? "needs" : "need"} you`
          : `${capacity.working} of ${capacity.limit}`}{capacity.queued > 0 ? ` · ${capacity.queued} queued` : ""}{planning
          ? ` · ${planning} planning`
          : ""}{capacity.research_working > 0 ? ` · ${capacity.research_working} researching` : ""}{pressure
            ? ` · ${pressureNote(pressure)}`
            : ""}
      </span>
      <span class="slots" aria-hidden="true">
        {#each { length: capacity.limit } as _, n (n)}<i class:on={n < capacity.working}></i>{/each}
      </span>
    {/if}
    {#if projectDisk}
      <span class="capacity" data-testid="agents-disk" title="Disk used by this project's worktrees"
        >{diskLabel(projectDisk)}</span
      >
    {/if}
    <span class="end">
      <KeepAwakeButton />
      {#if current}
        <button
          type="button"
          class="quiet icon-only"
          title="New task"
          aria-label="New task"
          onclick={newTask}>{@render icon("M12 5v14M5 12h14")}</button
        >
      {/if}
    </span>
  </header>
  {#if unsandboxed && !current}
    <p class="no-sandbox security" role="note" data-testid="agents-no-sandbox">
      <Icon name="shield-alert" small />{unsandboxed}
    </p>
  {/if}

  {#if !project}
    <p class="intro">Open a project to start an agent.</p>
  {:else if current}
    <div class="detail" data-testid="agent-session">
      <div class="bar">
        <button type="button" class="quiet back" onclick={() => open(null)}>{@render icon("M15 18l-6-6 6-6")}Tasks</button>
        <span class="buttons">
          {#if busyAgent}
            <button
              type="button"
              class="quiet"
              disabled={busy}
              title="Stops this step; you can reply"
              onclick={() => interrupt(current.id)}>Interrupt</button
            >
          {/if}
          {#if stoppable}
            <button
              type="button"
              class="quiet"
              disabled={busy}
              title={sessions.roles.get(current.id)?.kind === "coordinator"
                ? "Ends the task and its workers"
                : current.status.state === "queued"
                  ? "Takes it out of line; it never starts"
                  : "Ends the session"}
              onclick={() => stop(current.id)}>{current.status.state === "queued" ? "Cancel" : "Stop"}</button
            >
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
          <!-- Name and status share one line; the account and plan wait under Details. -->
          <div class="title-row">
            <h3 class="clamp" title={sessions.name(current.id)}>{sessions.name(current.id)}</h3>
            <p class="status" role="status" data-state={tone(current)}>{sessions.status(current.id)}</p>
          </div>
          {#if current.billing?.kind === "api_key"}
            <!-- An API key bills at API prices, often not what a plan user expects: it stays in sight. -->
            <p class="line warn" data-testid="agent-billing">
              {accounts.list.length > 1 ? `${accounts.name(current.account)} · ` : ""}{billedShort(current.billing)}
            </p>
          {/if}
          <div class="meta-row">
            {#if sandbox}
              <!-- One line, never hidden; what the sandbox covers, and what not, is under Details. -->
              <p class="line" class:security={sandbox.warning} data-testid="agent-sandbox">
                {#if sandbox.warning}<Icon name="shield-alert" small />{/if}{sandbox.text}
              </p>
            {/if}
            {#if current.usage.input_tokens + current.usage.output_tokens > 0 || current.billing || sandbox?.more}
            <details class="more">
              <summary>Details</summary>
              {#if current.billing && current.billing.kind !== "api_key"}
                <p data-testid="agent-billing-more">
                  {accounts.list.length > 1 ? `${accounts.name(current.account)} · ` : ""}{billedShort(current.billing)}
                </p>
              {/if}
              {#if sandbox?.more}<p data-testid="agent-sandbox-more">{sandbox.more}</p>{/if}
              {#if gaps}<p data-testid="agent-sandbox-gaps">{gaps}</p>{/if}
              {#if current.billing}<p>{paidText(current.billing)}</p>{/if}
              {#if current.usage.input_tokens + current.usage.output_tokens > 0}
                <p data-testid="agent-usage">Total: {usageText(current.usage, current.billing)}</p>
              {/if}
            </details>
            {/if}
          </div>
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
            {#if doneLine === current.id}
              <p class="row" data-testid="agent-done-hint">
                <span
                  >Done · Changed {changed[0]?.path}{changed.length > 1 ? ` and ${changed.length - 1} more` : ""}</span
                >
                <button type="button" class="quiet" onclick={() => current && openReviews([current.id])}
                  >Show changes</button
                >
              </p>
            {/if}
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
        {:else if report}
          <!-- A research session changes nothing: its answer is the outcome. -->
          <div class="card outcome" data-testid="agent-report">
            <span class="line">{saved ? `Saved as ${saved}` : "Research changes no files."}</span>
            <div class="row">
              {#if saved}
                <button type="button" class="quiet" onclick={() => saved && workspace.openFile(saved, "edit")}
                  >Open</button
                >
              {:else}
                <button type="button" class="quiet" data-testid="report-save" onclick={() => void saveReport()}
                  >Save as doc</button
                >
              {/if}
              <button type="button" class="quiet" data-testid="report-task" onclick={() => void toTask()}
                >Turn into task</button
              >
            </div>
          </div>
        {:else if waiting > 0}
          <div class="card outcome waits" data-testid="agent-reviews">
            <span>{waiting === 1 ? "1 change waits" : `${waiting} changes wait`} for your review.</span>
            <button
              type="button"
              class="quiet soft"
              onclick={() => current && reviewOf(current.id)}
              >Review</button
            >
          </div>
        {/if}
      </div>
      {#if currentTask}
        {@const questions = boards.openForPerson(currentTask).length}
        <div class="tabs" role="tablist" aria-label="Task" tabindex="-1" onkeydown={tabKeys}>
          <button
            type="button"
            role="tab"
            id="task-tab-talk"
            class="quiet"
            aria-selected={tab === "talk"}
            aria-controls="task-panel-talk"
            tabindex={tab === "talk" ? 0 : -1}
            onclick={() => (tab = "talk")}>Conversation</button
          >
          <button
            type="button"
            role="tab"
            id="task-tab-board"
            class="quiet"
            aria-selected={tab === "board"}
            aria-controls="task-panel-board"
            tabindex={tab === "board" ? 0 : -1}
            data-testid="agent-board-tab"
            onclick={() => (tab = "board")}>Board{questions > 0 ? ` (${questions})` : ""}</button
          >
        </div>
      {/if}
      {#if currentTask && tab === "board"}
        <Board task={currentTask} id="task-panel-board" labelledby="task-tab-board" />
      {:else}
      <div
        class="talk"
        role={currentTask ? "tabpanel" : undefined}
        id={currentTask ? "task-panel-talk" : undefined}
        aria-labelledby={currentTask ? "task-tab-talk" : undefined}
      >
      <Transcript
        messages={messages ?? []}
        agent={names.name(current.agent)}
        billing={current.billing}
        root={folder(current.id)}
        {workerName}
        status={current.status}
        partial={sessions.partial.get(current.id) ?? ""}
        onretry={retryTurn}
        denied={(use) => sessions.denied(current.id, use.name, use.input)}
        checkpoints={checkpoints?.session === current.id ? checkpoints.turns : undefined}
        goBackBlocked={isBusy(current.status) ? "Stop the agent first" : null}
        {undone}
        ongoback={goesBack ? (turn, text) => void goBack(turn, text) : undefined}
        onlink={(href) => openLink(current.id, href)}
      />
      {#if wentBackLine || (goesBack && current.can_undo_go_back)}
        <div class="went-back" data-testid="go-back-result">
          {#if wentBackLine}<p>{wentBackLine}</p>{/if}
          {#if goesBack && current.can_undo_go_back}
            <button type="button" class="quiet soft" disabled={busy || isBusy(current.status)} onclick={() => void undoGoBack()}
              >Undo go back</button
            >
          {/if}
        </div>
      {/if}
      {#if !isEnded(current.status) && !signedOut}
        <form
          class="compose"
          onsubmit={(event) => {
            event.preventDefault();
            void send();
          }}
        >
          <label for="agent-reply" class="visually-hidden">Reply</label>
          <textarea
            id="agent-reply"
            rows="2"
            placeholder={canReply ? "Reply" : `${names.name(current.agent)} is working…`}
            bind:this={replyField}
            bind:value={reply}
            oninput={() => replyContext?.refresh()}
            onclick={() => replyContext?.refresh()}
            onkeydown={(event) => {
              if (!replyContext?.keydown(event)) submitOnEnter(event, send);
            }}
          ></textarea>
          <ContextInput
            bind:this={replyContext}
            field={replyField}
            bind:value={reply}
            project={current.project}
            target={current.id}
            extra={pageSuggestions}
            id="agent-reply-context"
          />
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
            {@render addContext(() => replyContext?.start())}
            <span class="line">{canReply ? "Shift+Enter: new line" : ""}</span>
            <button type="submit" class="primary" disabled={busy || !canReply || !reply.trim() || drafts.unconfirmed(current.id).length > 0}>Send</button>
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
            onkeydown={(event) => submitOnEnter(event, resume)}
          ></textarea>
          <div class="row">
            <span class="line">Picks up where it stopped</span>
            <button type="submit" class="primary" disabled={busy || !reply.trim()}>Resume</button>
          </div>
        </form>
      {/if}
      </div>
      {/if}
    </div>
  {:else}
    <div class="scroll">
      {#if list.length === 0}
        <p class="intro" data-testid="agents-empty">No tasks yet.</p>
      {:else}
        <ul class="sessions" aria-label="Tasks">
          {#each ordered as session (session.id)}
            {@const role = sessions.roles.get(session.id)}
            {@const action = chip(session)}
            {@const parts = role?.kind === "coordinator" ? progress(session.id) : null}
            {@const now = session.status.state === "working" ? step(session.id) : null}
            {@const taken = agentPrefs.showResources ? sessions.resources.get(session.id) : undefined}
            {@const disk = role?.kind === "worker" ? role.disk : null}
            <li class:worker={role?.kind === "worker"}>
              <!-- One Tab stop for the list (arrows move, Delete closes, Shift+F10 opens its menu). -->
              <button
                type="button"
                class="row-button"
                data-state={tone(session)}
                data-session={session.id}
                title={rowTip(session)}
                tabindex={session.id === tabStop ? 0 : -1}
                aria-keyshortcuts={closable(session) ? "Delete Shift+F10" : "Shift+F10"}
                onfocus={() => (rowFocus = session.id)}
                onkeydown={(event) => onRowKey(event, session)}
                oncontextmenu={(event) => rowMenu(event, session)}
                onclick={() => open(session.id)}
              >
                <span class="state-mark" aria-hidden="true"></span>
                <span class="text">
                  <span class="title">{rowTitle(session)}</span>
                  <span class="state">{#if now}<span class="visually-hidden">Working: </span>{now}{:else}{sessions.status(
                        session.id,
                      )}{/if}</span>
                  {#if taken && resourcesLine(taken)}
                    <!-- Only when asked for (Settings): what it takes of the computer; the split on hover. -->
                    <span class="taken" data-testid="agent-resources" title={resourcesTip(taken, disk)}
                      >{resourcesLine(taken)}</span
                    >
                  {/if}
                  {#if disk}
                    <span class="taken" data-testid="agent-disk" title="Cloned files count in full: the real use may be less"
                      >{diskLabel(disk)}</span
                    >
                  {/if}
                  {#if parts}
                    <span class="progress" data-testid="agent-task-progress">
                      <span
                        class="meter"
                        role="progressbar"
                        aria-label="Workers done"
                        aria-valuemin={0}
                        aria-valuemax={parts.total}
                        aria-valuenow={parts.done}
                        ><i style:width="{(100 * parts.done) / parts.total}%"></i></span
                      >
                      {parts.done} of {parts.total} done
                    </span>
                  {/if}
                </span>
                {#if action}<span class="pill {action.tone}">{action.text}</span>{/if}
              </button>
              {#if closable(session)}
                <button
                  type="button"
                  class="quiet close"
                  aria-label={`Close ${sessions.name(session.id)}`}
                  title="Close"
                  tabindex={-1}
                  disabled={busy}
                  onclick={() => close(session.id)}>{@render icon("M6 6l12 12M18 6L6 18")}</button
                >
              {:else}
                <span class="close-space" aria-hidden="true"></span>
              {/if}
            </li>
            {#if confirming === session.id}<li class="confirm-row">{@render confirmClose(session.id)}</li>{/if}
            {#if role?.kind === "worker" && session.status.state === "preparing" && (role.setup || secretLooking(role.included).length > 0)}
              <li class="setup-row">
                <WorkerSetup session={session.id} setup={role.setup} secrets={secretLooking(role.included)} />
              </li>
            {/if}
            {#each role?.kind === "coordinator" ? (sessions.failedStarts.get(session.id) ?? []) : [] as missed (missed.name)}
              <!-- A worker that never started has no conversation: its row says why. -->
              <li class="worker" data-testid="agent-failed-start">
                <span class="row-button" data-state="failed">
                  <span class="state-mark" aria-hidden="true"></span>
                  <span class="text">
                    <span class="title">{missed.name}</span>
                    <span class="state" title={missed.reason}>Did not start: {missed.reason}</span>
                  </span>
                </span>
                <span class="close-space" aria-hidden="true"></span>
              </li>
            {/each}
          {/each}
        </ul>
      {/if}
      {#if project}<Leftovers project={project.id} />{/if}
      {#if reviewHint}
        <p class="line hint" data-testid="agents-review-hint">Changes wait for you: Review keeps or discards them.</p>
      {/if}
      {#if error}
        <p class="error" role="alert">{error}</p>
      {/if}
      <FirstSteps
        agent={names.name()}
        ready={usable !== undefined}
        started={list.length > 0}
        ask={asking ? () => asking && answer(asking.id) : undefined}
        review={reviewable ? () => reviewable && reviewOf(reviewable.id) : undefined}
        {ended}
      />
      <AgentSetup open={setupOpen} />
      <ProjectTrust />
    </div>
    <!-- While requests wait, the box is one line, so the list and the requests keep the room; focus opens it.
         Mod+Enter from any of its controls starts the task, as from the field. -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <form
      class="compose new"
      class:folded={foldStart}
      onsubmit={(event) => {
        event.preventDefault();
        void start();
      }}
      onkeydown={startOnModEnter}
      onfocusin={() => (startFocus = true)}
      onfocusout={(event) => {
        const to = event.relatedTarget;
        if (!(to instanceof Node && event.currentTarget.contains(to))) startFocus = false;
      }}
    >
      <label for="agent-prompt" class="label" class:visually-hidden={foldStart}>Describe a task</label>
      <textarea
        id="agent-prompt"
        rows={foldStart ? 1 : 2}
        placeholder={foldStart ? "Describe a task…" : "Describe a change, or ask about the code"}
        bind:this={promptField}
        bind:value={prompt}
        aria-describedby="agent-mode-hint"
        oninput={() => taskContext?.refresh()}
        onclick={() => taskContext?.refresh()}
        onkeydown={(event) => {
          if (!taskContext?.keydown(event)) submitOnEnter(event, start);
        }}
      ></textarea>
      <ContextInput
        bind:this={taskContext}
        field={promptField}
        bind:value={prompt}
        project={project.id}
        target={newBox}
        id="agent-prompt-context"
      />
      {#if !foldStart}
      <div class="row">
        <!-- How the task runs: a menu button whose options say what each one means. -->
        <span class="mode-menu">
          <button
            type="button"
            class="quiet"
            aria-haspopup="menu"
            aria-expanded={modeMenu}
            aria-controls="agent-mode-menu"
            data-testid="agent-mode"
            onclick={() => toggleModes(!modeMenu)}
            onkeydown={(event) => {
              if (event.code === "ArrowDown" || event.code === "ArrowUp") {
                event.preventDefault();
                void toggleModes(true);
              }
            }}
            >{MODES.find((m) => m.id === mode)?.label}<Icon name="chevron" small /></button
          >
          {#if modeMenu}
            <div
              class="menu"
              id="agent-mode-menu"
              style:left="{menuAt.left}px"
              style:bottom="{menuAt.bottom}px"
              role="menu"
              tabindex="-1"
              aria-label="How the agent works on it"
              bind:this={modeList}
              onkeydown={onModeKey}
              onfocusout={(event) => {
                const to = event.relatedTarget;
                const toggle = document.querySelector("[data-testid=agent-mode]");
                if (!(to instanceof Node && (modeList?.contains(to) || to === toggle))) modeMenu = false;
              }}
            >
              {#each MODES as option (option.id)}
                <button
                  type="button"
                  role="menuitemradio"
                  data-mode={option.id}
                  aria-checked={mode === option.id}
                  onclick={() => pickMode(option.id)}
                  ><span class="name">{option.label}</span><span class="what">{option.more}</span></button
                >
              {/each}
            </div>
          {/if}
        </span>
        {@render addContext(() => taskContext?.start())}
        <AccountPicker project={project.id} bind:account />
        <button
          type="submit"
          class="primary"
          title={startBlock ??
            (!prompt.trim()
              ? "Type a task first"
              : atCap
                ? "Starts when an agent finishes"
                : "Enter starts · Shift+Enter: new line")}
          disabled={busy || startBlock !== null || !prompt.trim() || drafts.unconfirmed(newBox).length > 0}>Start</button
        >
      </div>
      <p class="line" id="agent-mode-hint">{modeHint}</p>
      {#if typeFirst}
        <p class="line warn" role="status" data-testid="agent-type-first">Type a task first.</p>
      {/if}
      {#if blocked}
        <p class="blocked" role="note" data-testid="agent-account-blocked">
          <span
            >{accounts.list.length > 1 ? accounts.name(account ?? accounts.projectAccount) : names.name()} is signed out.</span
          >
          {#if usable}
            <button type="button" class="quiet" onclick={() => (account = usable.account.id)}
              >Use {usable.account.name}</button
            >
          {/if}
        </p>
      {:else if noGit}
        <p class="blocked" role="note" data-testid="agent-needs-git">
          <span>Needs git: Plan and split works on copies of a git repository.</span>
          <button type="button" class="quiet soft" onclick={() => project && setUpGit(project.id)}>Set up git</button>
        </p>
      {:else if atCap}
        <p class="blocked calm" role="note" data-testid="agent-at-cap">
          <span>All {capacity?.limit} agents are busy. This starts when one finishes.</span>
        </p>
      {/if}
      {/if}
      {#if startError}
        <p class="error" role="alert" data-testid="agent-start-error">
          {startError.text}{#if startError.setup}{" "}<button type="button" class="quiet fix" onclick={openSetup}
              >Check {names.name()}</button
            >{/if}
        </p>
      {/if}
    </form>
    <!-- Who pays for the next task, always in sight; it opens the agent's setup. -->
    <p class="foot">
      <button
        type="button"
        class="quiet"
        class:warn={billingKey}
        title="{names.name()}: setup, accounts, billing"
        aria-expanded={setupOpen}
        data-testid="agents-footer"
        onclick={() => (setupOpen ? (setupOpen = false) : openSetup())}>{names.name()} · {billing}</button
      >
    </p>
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

  /* A task's Conversation and Board */
  .tabs {
    display: flex;
    gap: 2px;
    padding: 0 16px 6px;
  }
  .tabs button {
    height: 24px;
    padding: 0 8px;
    font-size: 13px;
  }
  /* The conversation keeps the detail's own layout. */
  .talk {
    display: contents;
  }
  .tabs [aria-selected="true"] {
    color: var(--text);
    background: var(--s3);
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
  /* A preparing worker's setup sits under its row, indented like it. */
  .sessions li.setup-row {
    display: block;
    margin: -2px 0 0 20px;
  }
  .sessions li.setup-row:hover {
    background: none;
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
  /* Every state has its own shape too: a ring while queued, a square once stopped. */
  [data-state="queued"] .state-mark {
    background: none;
    box-shadow: inset 0 0 0 2px var(--text-3);
  }
  [data-state="stopped"] .state-mark {
    border-radius: 2px;
    background: var(--text-3);
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
  .hint {
    padding: 0 8px;
  }
  .no-sandbox {
    flex: none;
    padding: 0 16px 6px;
    font-size: 13px;
  }
  /* A security fact: full-strength text with a shield, apart from the
     yellow of "Needs approval". */
  .security {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text);
    font-weight: 500;
  }
  .taken {
    font-size: 12px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .progress {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 2px;
    font-size: 13px;
    color: var(--text-2);
  }
  .meter {
    flex: 1;
    max-width: 120px;
    height: 4px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--s4);
  }
  .meter i {
    display: block;
    height: 100%;
    background: var(--ok);
  }
  /* Close has its own column, so it never covers a pill; it shows on
     hover or focus (always without hover). */
  .close,
  .close-space {
    flex: none;
    width: 28px;
    margin: 8px 4px 0 0;
  }
  .close {
    padding: 0;
    justify-content: center;
    opacity: 0;
  }
  .sessions li:hover .close,
  .sessions li:focus-within .close {
    opacity: 1;
  }
  @media (hover: none) {
    .close {
      opacity: 1;
    }
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
  .went-back {
    margin: 0 16px 4px;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-2);
  }
  .went-back p {
    flex: 1;
    min-width: 0;
    margin: 0;
    overflow-wrap: anywhere;
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
    padding: 2px 16px 6px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .title-row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }
  .title-row h3 {
    flex: 0 1 auto;
    min-width: 0;
  }
  .title-row .status {
    flex: none;
  }
  /* The sandbox line and Details share a row; opened, Details takes the full width below. */
  .meta-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    column-gap: 12px;
    row-gap: 2px;
    min-width: 0;
  }
  .meta-row > .line {
    min-width: 0;
  }
  .meta-row > .more[open] {
    flex-basis: 100%;
  }
  h3 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  /* The title is the task's first line: one line here, whole on hover. */
  h3.clamp {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
  /* In line with the other cards: text on the left, a button that looks like one on the right. */
  .card.waits {
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
  }
  .soft {
    flex: none;
    color: var(--text);
    background: var(--s3);
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
  /* The field looks like one (an input's --s3), so it is clear where to type;
     the label, the field, the controls and the hint share the box's left edge. */
  textarea {
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: none;
    border-radius: var(--radius-sm);
    resize: none;
    padding: 6px 10px;
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
  .compose.new.folded {
    padding: 6px 10px;
  }
  .compose:focus-within {
    box-shadow: 0 0 0 2px var(--accent-soft);
  }
  .compose .row {
    flex-wrap: wrap;
    justify-content: flex-start;
    gap: 6px;
  }
  /* The start box's controls are boxes of one height, like the account picker. */
  .compose.new .row .quiet {
    height: var(--control-h);
    background: var(--s3);
    border-radius: var(--radius-sm);
  }
  .compose.new .row .quiet:hover {
    background: var(--s4);
  }
  .add-context {
    width: 28px;
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .compose .row .primary {
    margin-left: auto;
  }
  .compose.new .add-context {
    width: var(--control-h);
  }
  .label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-2);
  }
  .mode-menu {
    position: relative;
  }
  .mode-menu > .quiet {
    gap: 4px;
  }
  .menu {
    position: fixed;
    z-index: 20;
    width: max-content;
    max-width: 280px;
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: var(--s4);
    border-radius: var(--radius-lg);
    box-shadow: var(--card-shadow);
  }
  .menu button {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    padding: 6px 10px;
    text-align: left;
  }
  .menu button:hover,
  .menu button:focus-visible {
    color: var(--accent-ink);
    background: var(--accent);
    outline: none;
  }
  .menu .name {
    font-weight: 500;
  }
  .menu [aria-checked="true"] {
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .menu .what {
    font-size: 13px;
    opacity: 0.85;
  }
  /* Its text starts where the start box's content does (8 px margin + 10 px padding). */
  .foot {
    flex: none;
    margin: -4px 8px 6px;
  }
  .foot .quiet {
    height: 24px;
    padding: 0 10px;
    font-size: 12px;
  }
  .foot .warn {
    color: var(--warn);
  }
  .blocked {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    font-size: 13px;
    color: var(--warn);
  }
  /* Its action keeps to one line; the reason wraps beside it. */
  .blocked button {
    flex: none;
    white-space: nowrap;
  }
  .fix {
    color: var(--text-2);
  }
  /* Busy agents are no fault: the reason reads as plain text. */
  .blocked.calm {
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
    .close {
      opacity: 1;
    }
  }
</style>
