<script lang="ts">
// Pending permission requests, oldest first. Keyboard: Alt+Shift+A (Mod+Shift+A
// in the desktop app) jumps here; on a focused card Y allows, N denies,
// Shift+N denies with a message, arrows move between cards, Escape goes back.
//
// Nothing is answered by accident:
// - focus never moves here from elsewhere;
// - a request is answerable only ARM_MS after it appeared, after the previous
//   answer and after any card moved (a request left the list or the list
//   was reloaded in another order), and the
//   buttons look disabled exactly as long as they are;
// - one key press answers at most one request: auto-repeat is ignored and the
//   next answer needs the key released first;
// - nothing is allowed unseen: Allow counts only once the card's head (who
//   asks, and any sandbox warning) and most of the request's details (or its
//   action, without details) have been in sight; before that a press scrolls
//   the missing part into view and waits again.
import { tick, untrack } from "svelte";
import type { Attachment } from "svelte/attachments";
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { hints } from "../agents/hints.svelte";
import { sandboxGaps, sandboxState } from "../agents/sandbox";
import { agentSessions } from "../agents/sessions.svelte";
import { claudeAccounts } from "../agents/setup/claude-accounts.svelte";
import { listCommands, registerCommand } from "../keys/commands";
import type { Decision, PermissionId, PermissionRequest, Project, ProjectId, SessionId } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { typed } from "../shell/menu";
import { describeTool } from "./describe";
import { ARM_MS, approvalQueue } from "./queue.svelte";
import { pathScopes, sameFolder } from "./scope";

const { transport, workspace } = useOvra();
const queue = approvalQueue(transport);
const sessions = agentSessions(transport);
const accounts = claudeAccounts(transport);

let now = $state(Date.now());
/** The last answer or the last time a request left the list. */
let quietFrom = $state(0);
/** A key answered and has not been released yet. */
let keyHeld = $state(false);
let denying = $state<string | null>(null);
let note = $state("");
let list = $state<HTMLElement>();
let heading = $state<HTMLElement>();
let section = $state<HTMLElement>();
let returnTo: HTMLElement | null = null;
/** The list is hidden; the heading still counts what waits. */
let collapsed = $state(false);
/** Requests after the first whose details the person opened. */
const opened = new SvelteSet<PermissionId>();
/** Said to screen readers: what a jump to a request found. */
let notice = $state("");
/** Requests whose details have been in sight, so Allow may count. */
const seen = new SvelteSet<PermissionId>();
/** Requests whose head (who asks, sandbox warnings) has been in sight. */
const headSeen = new SvelteSet<PermissionId>();
/** A note on one card: why a press did not answer. */
let said = $state<{ id: PermissionId; text: string } | null>(null);
const projects = new SvelteMap<ProjectId, Project>();
/** Projects already asked for, so a project the core does not know is asked once. */
const requested = new Set<ProjectId>();

/** Nothing waits and nothing to report: the section takes no space. */
const idle = $derived(queue.pending.length === 0 && !queue.error && !notice);

const armedAt = (id: string): number => Math.max(queue.arrived.get(id) ?? 0, quietFrom) + ARM_MS;
/** The one test for both the look of the buttons and whether a press counts. */
const armed = (id: string): boolean => !keyHeld && now >= armedAt(id) && !queue.answering.has(id);
/** Allow also needs the details to have been in sight. */
const allowable = (id: PermissionId): boolean => armed(id) && seen.has(id) && headSeen.has(id);

/** Said to screen readers only: the Allow button's fill shows the wait. */
const EARLY = "Ready in a moment — read the request first.";
const UNSEEN = "Scrolled to the request: read it, then allow.";
/** In sight: most of it (a clipped last line still counts), or this many pixels of a taller one. */
const SEEN_RATIO = 0.75;
const SEEN_PX = 120;

/** Adds a request to `into` once the element (its details or its head) is in sight. */
function watch(into: SvelteSet<PermissionId>, id: PermissionId): Attachment<HTMLElement> {
  return (element) => {
    // No observer, no way to tell: the request is never counted as seen.
    if (typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          if (entry.intersectionRatio >= SEEN_RATIO || entry.intersectionRect.height >= SEEN_PX) into.add(id);
        }
      },
      { threshold: [0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1] },
    );
    observer.observe(element);
    return () => observer.disconnect();
  };
}

/** Adds every one of `ids` to `into` once the element is in sight. */
function watchAll(into: SvelteSet<PermissionId>, ids: PermissionId[]): Attachment<HTMLElement> {
  return (element) => {
    const stops = ids.map((id) => watch(into, id)(element));
    return () => {
      for (const stop of stops) if (typeof stop === "function") stop();
    };
  };
}

/** Scrolls a card into view from its top, then its details; in a short
 * column its head stays in sight above them (it sticks to the top). */
function scrollToDetails(target: HTMLElement): void {
  target.scrollIntoView?.({ block: "start" });
  target.querySelector("dl")?.scrollIntoView?.({ block: "nearest" });
}

/** Focuses a card and scrolls it into view, not just its first line. */
async function bringUp(target: HTMLElement): Promise<void> {
  target.focus({ preventScroll: true });
  // Focus opens a card's details; scroll once they are there.
  await tick();
  scrollToDetails(target);
}

function quiet(): void {
  quietFrom = Date.now();
  now = quietFrom;
}

// Re-render when the next request becomes answerable.
$effect(() => {
  const waits = queue.pending.map((r) => armedAt(r.id)).filter((t) => t > now);
  if (waits.length === 0) return;
  const timer = setTimeout(() => (now = Date.now()), Math.max(0, Math.min(...waits) - Date.now()));
  return () => clearTimeout(timer);
});

// A card that moves (or leaves) puts another one under the pointer.
let shown: PermissionId[] = [];
$effect(() => {
  const ids = visible.map((r) => r.id);
  untrack(() => {
    if (shown.some((id, i) => ids[i] !== id)) quiet();
    for (const id of opened) if (!ids.includes(id)) opened.delete(id);
    for (const set of [seen, headSeen]) for (const id of set) if (!ids.includes(id)) set.delete(id);
    if (said && !ids.includes(said.id)) said = null;
  });
  shown = ids;
});

$effect(() => {
  const release = () => {
    // The next card arms ARM_MS after the key that answered comes up.
    if (keyHeld) quiet();
    keyHeld = false;
  };
  window.addEventListener("keyup", release);
  // A key released in another window sends no keyup here; held keys only repeat, which is ignored.
  window.addEventListener("blur", release);
  return () => {
    window.removeEventListener("keyup", release);
    window.removeEventListener("blur", release);
  };
});

// Project names and folders for the cards.
$effect(() => {
  const missing = queue.pending.filter((r) => !projects.has(r.project) && !requested.has(r.project));
  if (missing.length === 0) return;
  for (const r of missing) requested.add(r.project);
  untrack(() => {
    transport
      .call({ method: "projects.list" })
      .then(({ data }) => {
        for (const project of data) projects.set(project.id, project);
      })
      .catch(() => {});
  });
});

$effect(() =>
  registerCommand({ id: "approvals.focus", title: "Go to pending approvals", keys: "Mod+Shift+A", run: enter }),
);

$effect(() => {
  queue.revealer = async (id) => {
    remember();
    if (collapsed) await show();
    const target = card(id);
    // Never another request instead: a key press meant for this one would answer it.
    const behind = queue.pending.some((r) => r.id === id);
    notice = target
      ? ""
      : behind
        ? "That request waits behind an identical one."
        : "That request was already answered.";
    if (target) await bringUp(target);
    else heading?.focus();
  };
  return () => {
    queue.revealer = null;
  };
});

function cards(): HTMLElement[] {
  return [...(list?.querySelectorAll<HTMLElement>("[data-request]") ?? [])];
}

function card(id: PermissionId): HTMLElement | undefined {
  return cards().find((c) => c.dataset.request === id);
}

function remember(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement && !list?.contains(active) && active !== heading) returnTo = active;
}

/** The person folded the Agents column: the shortcut unfolds it, through the shell's own command. */
async function unfoldAgents(): Promise<void> {
  if (!document.querySelector("[data-testid=agents] .strip")) return;
  await listCommands()
    .find((c) => c.id === "layout.toggle_agents")
    ?.run();
  await tick();
}

async function enter(): Promise<void> {
  remember();
  await unfoldAgents();
  if (collapsed) await show();
  const first = cards()[0];
  if (first) await bringUp(first);
  else heading?.focus();
}

async function show(): Promise<void> {
  collapsed = false;
  await tick();
}

/** Only the first request shows its details at once, so a few requests leave room for the agents. */
function isOpen(id: PermissionId): boolean {
  return visible[0]?.id === id || opened.has(id);
}

/** Shows a request's details; the cards below move, so answers wait again. */
function expand(id: PermissionId): void {
  if (isOpen(id)) return;
  opened.add(id);
  quiet();
}

function leave(): void {
  if (returnTo?.isConnected) returnTo.focus();
  else if (document.activeElement instanceof HTMLElement && section?.contains(document.activeElement))
    document.activeElement.blur();
  returnTo = null;
}

/** Scrolls what was not in sight yet into view; what moved under the pointer waits again. */
function showUnseen(request: PermissionRequest): void {
  const target = card(request.id);
  if (target && !seen.has(request.id)) scrollToDetails(target);
  else target?.scrollIntoView?.({ block: "start" });
  said = { id: request.id, text: UNSEEN };
  quiet();
}

async function answer(
  request: PermissionRequest,
  decision: Decision,
  message?: string,
  byPointer = false,
): Promise<void> {
  if (!armed(request.id)) return;
  // Nothing is allowed unseen: the first press shows what it would allow.
  if (decision === "allow" && !isOpen(request.id)) {
    expand(request.id);
    return;
  }
  if (decision === "allow" && !(seen.has(request.id) && headSeen.has(request.id))) {
    showUnseen(request);
    return;
  }
  said = null;
  // Only a request already on screen may take the focus next: one that
  // arrives while the answer is on its way was never seen.
  const before = visible.map((r) => r.id);
  const index = before.indexOf(request.id);
  notice = "";
  const focused = list?.contains(document.activeElement) ?? false;
  quiet();
  const sent = await queue.answer(request.id, decision, message);
  if (!sent) return;
  hints.mark("answered");
  if (denying === request.id) {
    denying = null;
    note = "";
  }
  await tick();
  if (!focused) return;
  const onScreen = [...before.slice(index + 1), ...before.slice(0, index).reverse()];
  const next = onScreen.map(card).find((c) => c !== undefined);
  if (next) next.focus();
  // With nothing seen left, a pointer answer gives focus back, so the empty
  // section does not stay over the agents; keys stay on the heading. A
  // request that arrives later is never focused on its own: typing must not land on it.
  else if (byPointer) leave();
  else heading?.focus();
}

/** Answers from a click; one too early says why instead of doing nothing. */
function answerByClick(event: MouseEvent, request: PermissionRequest, decision: Decision): void {
  if (!armed(request.id)) {
    said = { id: request.id, text: EARLY };
    void revealNote(request.id);
    return;
  }
  // `detail` is 0 for a click from Enter or Space on the button.
  void answer(request, decision, undefined, event.detail > 0);
}

/** Keeps a card's Allow button (and its fill) in sight; if that moved the buttons, they wait again. */
async function revealNote(id: PermissionId): Promise<void> {
  await tick();
  const allow = card(id)?.querySelector<HTMLElement>("button.allow");
  const row = allow?.parentElement;
  if (!allow || !row) return;
  const before = row.getBoundingClientRect().top;
  allow.scrollIntoView?.({ block: "nearest" });
  if (row.getBoundingClientRect().top !== before) quiet();
}

/** Answers from a key press: at most one answer per press; one too early says why, as a click does. */
function answerByKey(event: KeyboardEvent, request: PermissionRequest, decision: Decision, message?: string): void {
  if (event.repeat || keyHeld) return;
  if (!armed(request.id)) {
    said = { id: request.id, text: EARLY };
    void revealNote(request.id);
    return;
  }
  // `answer` checks `armed` before its first await, so mark the key after.
  void answer(request, decision, message);
  keyHeld = true;
}

async function openDeny(request: PermissionRequest): Promise<void> {
  denying = request.id;
  note = "";
  await tick();
  card(request.id)?.querySelector<HTMLTextAreaElement>("textarea")?.focus();
}

function onKeydown(event: KeyboardEvent, request: PermissionRequest): void {
  if (event.target instanceof HTMLTextAreaElement) {
    if (event.code === "Escape") {
      event.preventDefault();
      denying = null;
      card(request.id)?.focus();
    } else if (event.code === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      answerByKey(event, request, "deny", note.trim() || undefined);
    }
    return;
  }
  // Return never allows, not even on a focused Allow button: Space, a click or Y do.
  if (
    event.target instanceof HTMLButtonElement &&
    event.target.classList.contains("allow") &&
    (event.code === "Enter" || event.code === "NumpadEnter")
  ) {
    event.preventDefault();
    return;
  }
  // Buttons answer by click (Enter, Space); everything else is the card's.
  if (event.target instanceof HTMLButtonElement && (event.code === "Enter" || event.code === "Space")) return;
  if (event.ctrlKey || event.metaKey || event.altKey || event.isComposing) return;
  const all = cards();
  const at = all.findIndex((c) => c.dataset.request === request.id);
  // Y and N answer only on the card itself, never from a control inside it.
  const onCard = event.target === event.currentTarget;
  // Y and N by the letter typed (on QWERTZ the key labelled Y), else by
  // the physical key on a non-Latin layout, as the menus and shortcuts do.
  const letter = typed(event);
  const handled = (() => {
    switch (letter ? `Key${letter.toUpperCase()}` : event.code) {
      case "KeyY":
        if (event.shiftKey || !onCard) return false;
        answerByKey(event, request, "allow");
        return true;
      case "KeyN":
        if (!onCard) return false;
        if (event.shiftKey) {
          if (!event.repeat) void openDeny(request);
        } else answerByKey(event, request, "deny");
        return true;
      case "ArrowDown":
        all[Math.min(at + 1, all.length - 1)]?.focus();
        return true;
      case "ArrowUp":
        all[Math.max(at - 1, 0)]?.focus();
        return true;
      case "Escape":
        leave();
        return true;
      default:
        return false;
    }
  })();
  if (handled) event.preventDefault();
}

/** Whether the agent's title only repeats what the card already shows. */
function repeats(title: string, request: PermissionRequest, action: string): boolean {
  const record = typeof request.input === "object" && request.input !== null ? request.input : {};
  return action.includes(title) || Object.values(record).some((v) => v === title);
}

/** The folder paths are judged against, and what to call it. */
function folderOf(request: PermissionRequest, project: Project | undefined): { path: string | null; name: string } {
  const path = request.cwd ?? project?.root ?? null;
  if (path === null) return { path, name: "a project" };
  const own = project !== undefined && sameFolder(path, project.root);
  return { path, name: own ? "the project" : "the agent's folder" };
}

/** A warning when the runtime said the session runs without a sandbox, or
 * in one with an open network; unknown sessions say nothing. */
function sandboxWarning(session: SessionId): string | null {
  const info = sessions.byId.get(session);
  const state = info ? sandboxState(info) : null;
  if (state?.kind === "none") return "No sandbox";
  if (state?.kind === "sandboxed" && state.report.network === "open") return "Network open";
  return null;
}

/** What the session's sandbox does not cover; null without gaps. */
function gapsOf(session: SessionId): string | null {
  const info = sessions.byId.get(session);
  return info ? sandboxGaps(sandboxState(info), false) : null;
}

/** A worker acts on its own copy of the project, not on the project itself. */
function workerCopy(session: SessionId): string {
  const role = sessions.roles.get(session);
  const branch = role?.kind === "worker" ? role.branch : "";
  return `Own copy, branch ${branch}`;
}

/** "Sandboxed" once the runtime said so; warnings above say the rest. */
function sandboxFact(session: SessionId): string | null {
  const info = sessions.byId.get(session);
  return info && sandboxState(info).kind === "sandboxed" ? "Sandboxed" : null;
}

const PROVIDERS: Record<string, string> = { bedrock: "Bedrock", vertex: "Vertex AI", foundry: "Foundry" };

/** Who pays for what is allowed, in a few words; an API key is a warning. */
function billedTo(session: SessionId): { text: string; key: boolean } | null {
  const info = sessions.byId.get(session);
  const billing = info?.billing;
  if (!info || !billing) return null;
  const payer =
    billing.kind === "subscription"
      ? "Claude plan"
      : billing.kind === "api_key"
        ? "API key"
        : (PROVIDERS[billing.provider] ?? (billing.provider || "cloud account"));
  const account = accounts.list.length > 1 ? ` (${accounts.name(info.account)})` : "";
  return { text: `Billed to ${payer}${account}`, key: billing.kind === "api_key" };
}

/** The task a request belongs to: a worker's coordinator, else its own session. */
function taskOf(session: SessionId): SessionId {
  const role = sessions.roles.get(session);
  return role?.kind === "worker" ? role.coordinator : session;
}

/** Requests that would do exactly the same: one card answers them, one at a time. */
const sameAs = (a: PermissionRequest, b: PermissionRequest): boolean =>
  a.project === b.project &&
  a.tool === b.tool &&
  a.cwd === b.cwd &&
  JSON.stringify(a.input) === JSON.stringify(b.input);

type Billed = { text: string; key: boolean };
type Group = { key: string; requests: PermissionRequest[]; stacks: PermissionRequest[][] };

/** Requests by task, oldest first; identical ones stack behind the oldest, which alone is on screen. */
const groups = $derived.by((): Group[] => {
  const out: Group[] = [];
  for (const request of queue.pending) {
    const key = `${request.project} ${taskOf(request.session)}`;
    let group = out.find((g) => g.key === key);
    if (!group) {
      group = { key, requests: [], stacks: [] };
      out.push(group);
    }
    group.requests.push(request);
    const stack = group.stacks.find((s) => s[0] && sameAs(s[0], request));
    if (stack) stack.push(request);
    else group.stacks.push([request]);
  }
  return out;
});
/** The requests on screen, in screen order. */
const visible = $derived(groups.flatMap((g) => g.stacks.flatMap((s) => s.slice(0, 1))));

/** Who asks for a group: the task, or the session itself. */
function groupName(first: PermissionRequest): string {
  const lead = taskOf(first.session);
  return sessions.byId.has(lead) ? sessions.name(lead) : sessions.name(first.session);
}

/** Facts every request of a group shares, said once: sandbox warnings and who pays. */
function shared(requests: PermissionRequest[]): { warnings: string[]; sandboxed: boolean; billed: Billed[] } {
  const ids = [...new Set(requests.map((r) => r.session))];
  const warnings = [...new Set(ids.map(sandboxWarning).filter((w): w is string => w !== null))];
  const billed = ids.map(billedTo).filter((b): b is Billed => b !== null);
  return {
    warnings,
    sandboxed: warnings.length === 0 && ids.every((id) => sandboxFact(id) !== null),
    billed: billed.filter((b, i) => billed.findIndex((o) => o.text === b.text) === i),
  };
}

/** Tools whose object is code or a path: it shows in a mono pill after the verb. */
const MONO = new Set(["Bash", "Edit", "MultiEdit", "NotebookEdit", "Write", "Read", "Glob", "Grep", "WebFetch"]);

/** `Run` and `npm test`; other tools keep their whole summary as the verb. */
function verbAndObject(tool: string, action: string): { verb: string; object: string } {
  const space = action.startsWith("Find files ") ? "Find files".length : action.indexOf(" ");
  if (!MONO.has(tool) || space < 0) return { verb: action, object: "" };
  return { verb: action.slice(0, space), object: action.slice(space + 1) };
}
</script>

{#snippet icon(d: string)}
  <svg class="icon" viewBox="0 0 24 24" aria-hidden="true"><path {d} /></svg>
{/snippet}

<!-- Nothing to say: the section takes no space and is out of sight (screen
     readers still find it). Focus in it (the shortcut, the last answer) shows
     it over the agents, so nothing below moves when focus leaves again.
     While a request has focus, a scrim dims the rest of the window. -->
<section
  class="approvals"
  class:idle
  bind:this={section}
  aria-labelledby="approvals-heading"
  data-testid="approvals"
>
  <div class="body">
  <div class="head">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <h2
    id="approvals-heading"
    tabindex="-1"
    bind:this={heading}
    onkeydown={(event) => {
      if (event.code === "Escape") leave();
      else if (event.code === "ArrowDown") {
        event.preventDefault();
        cards()[0]?.focus();
      }
    }}
  >
    Approvals{#if queue.pending.length > 0}{" "}<span class="count">({queue.pending.length} waiting)</span>{/if}
  </h2>
  {#if queue.pending.length > 0}
    <button
      type="button"
      class="toggle"
      aria-expanded={!collapsed}
      aria-controls="approvals-list"
      onclick={() => (collapsed = !collapsed)}>{collapsed ? "Show" : "Hide"}</button
    >
  {/if}
  </div>
  <p class="notice" aria-live="polite" data-testid="approvals-notice">{notice}</p>
  {#if queue.error}
    <p class="error" role="alert">{queue.error}</p>
  {/if}
  {#if queue.pending.length === 0}
    <p class="empty">Nothing waits for your approval.</p>
  {/if}
  <!-- The keys show on the buttons; screen readers hear them with each request. -->
  <p class="sr-only" id="approvals-keys">Y allows, N denies, Shift+N denies with a message.</p>
  <ul bind:this={list} id="approvals-list" hidden={collapsed}>
    {#if !collapsed}
    {#each groups as group (group.key)}
      {@const first = group.requests[0] as PermissionRequest}
      {@const project = projects.get(first.project)}
      {@const facts = shared(group.requests)}
      {@const headId = `approval-group-${first.id}`}
      <li class="group">
        <!-- Who asks and what is shared, said once for the task: Allow needs it in sight too. -->
        <div class="who" id={headId} {@attach watchAll(headSeen, group.requests.map((r) => r.id))}>
          <p class="session">
            <span class="dot" aria-hidden="true"></span>{groupName(first)} · {project?.name ?? "unknown project"}
          </p>
          <p class="chips">
            {#each facts.warnings as warning (warning)}
              <span class="chip warn" data-testid="approval-unsandboxed" title={warning === "No sandbox" ? "It can reach anything you can" : "Sandboxed, but its network is open"}>{warning}</span>
            {/each}
            {#if first.project !== workspace.project?.id}
              <span class="chip warn" data-testid="approval-other-project">Not this window's project</span>
            {/if}
            {#if !project}
              <span class="chip danger" data-testid="approval-unknown-project">Unknown project: paths cannot be checked</span>
            {/if}
            {#if facts.sandboxed}<span class="fact">Sandboxed</span>{/if}
            {#each facts.billed as billed (billed.text)}
              <span class="fact" class:key={billed.key} data-testid="approval-billing">{billed.text}</span>
            {/each}
          </p>
        </div>
        <ul class="cards">
    {#each group.stacks as stack (stack[0]?.id)}
      {@const request = stack[0] as PermissionRequest}
      {@const summary = describeTool(request.tool, request.input)}
      {@const what = verbAndObject(request.tool, summary.action)}
      {@const ready = armed(request.id)}
      {@const domId = `approval-${request.id}`}
      {@const folder = folderOf(request, project)}
      {@const gaps = gapsOf(request.session)}
      {@const role = sessions.roles.get(request.session)}
      {@const agentNote = request.title && !repeats(request.title, request, summary.action) ? request.title : null}
      <li>
        <!-- A focusable group is how the keys reach one request (Y, N, arrows). -->
        <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
        <div
          class="card"
          role="group"
          tabindex="0"
          data-request={request.id}
          aria-labelledby="{domId}-action"
          aria-describedby="{headId} {domId}-session approvals-keys"
          onkeydown={(event) => onKeydown(event, request)}
          onfocusin={() => {
            queue.focused = request.id;
            expand(request.id);
          }}
          onfocusout={(event) => {
            const to = event.relatedTarget;
            if (!(to instanceof Node && event.currentTarget.contains(to)) && queue.focused === request.id)
              queue.focused = null;
          }}
        >
          <p class="worker" id="{domId}-session">
            {#if role?.kind === "worker"}{role.name}{/if}{#if stack.length > 1}<span data-testid="approval-stack"
                >{role?.kind === "worker" ? " · " : ""}{stack.length} identical requests, answered one at a time</span
              >{/if}
          </p>
          <h3 id="{domId}-action" {@attach summary.details.length === 0 && watch(seen, request.id)}>
            {what.verb}{#if what.object}{" "}<code class="object">{what.object}</code>{/if}
          </h3>
          {#each pathScopes(request.input, folder.path) as scope}
            {#if scope.scope === "inside"}
              <p class="line" data-testid="approval-scope" title={scope.path}>In {folder.name}: {scope.relative}</p>
            {:else}
              <p class="chip danger" data-testid="approval-scope">
                {scope.scope === "outside" ? "Outside" : "Cannot tell if in"}{" "}{folder.name}: {scope.path}
              </p>
            {/if}
          {/each}
          {#if request.reason}
            <p class="line" data-testid="approval-reason">{request.reason.text}</p>
          {/if}
          {#if isOpen(request.id)}
            <dl {@attach watch(seen, request.id)}>
              {#each summary.details as detail}
                <dt>{detail.label}</dt>
                <dd><pre>{detail.text}</pre></dd>
              {/each}
            </dl>
          {:else if summary.details.length > 0}
            <button type="button" class="quiet more" data-testid="approval-details" onclick={() => expand(request.id)}
              >Show details</button
            >
          {/if}
          {#if gaps || role?.kind === "worker" || agentNote || request.tool === "Bash"}
            <!-- Context, not the request itself: one click away. -->
            <details class="extra" data-testid={gaps ? "approval-sandbox-gaps" : "approval-extra"}>
              <summary>{@render icon("m9 6 6 6-6 6")}{gaps ? "Sandbox has gaps" : "Details"}</summary>
              {#if gaps}<p>{gaps}</p>{/if}
              {#if role?.kind === "worker"}<p data-testid="approval-worker-copy">{workerCopy(request.session)}</p>{/if}
              {#if request.tool === "Bash"}<p>Paths in commands are not checked.</p>{/if}
              {#if agentNote}<p>Agent's note: {agentNote}</p>{/if}
            </details>
          {/if}
          <div class="actions">
            <!-- Deny first, Allow once trailing: the macOS order. Return never allows. -->
            <button
              type="button"
              class="deny"
              aria-disabled={!ready}
              aria-keyshortcuts="N"
              onclick={(event) => answerByClick(event, request, "deny")}>Deny <kbd aria-hidden="true">N</kbd></button
            >
            <button
              type="button"
              class="allow"
              aria-disabled={!allowable(request.id)}
              aria-keyshortcuts="Y"
              onclick={(event) => answerByClick(event, request, "allow")}
              >Allow once <kbd aria-hidden="true">Y</kbd>{#if !ready}{#key armedAt(request.id)}<span
                    class="fill"
                    aria-hidden="true"
                    data-testid="approval-arming"
                    style:animation-duration="{Math.max(0, armedAt(request.id) - now)}ms"
                  ></span>{/key}{/if}</button
            >
          </div>
          <div class="after">
            <button type="button" class="quiet" aria-keyshortcuts="Shift+N" onclick={() => openDeny(request)}
              >Deny with a message…</button
            >
            <!-- Why a press did not answer: the fill says "not yet" on screen, this says it to screen readers. -->
            <span class="wait" class:sr-only={said?.text === EARLY} aria-live="polite" data-testid="approval-wait"
              >{said?.id === request.id && (said.text !== EARLY || !ready) ? said.text : ""}</span
            >
          </div>
          {#if denying === request.id}
            <form
              onsubmit={(event) => {
                event.preventDefault();
                void answer(request, "deny", note.trim() || undefined);
              }}
            >
              <label for="{domId}-note">Message to the agent</label>
              <textarea id="{domId}-note" rows="2" bind:value={note}></textarea>
              <div class="actions">
                <button type="button" class="quiet" onclick={() => (denying = null)}>Cancel</button>
                <button type="submit" class="deny" aria-disabled={!ready} aria-keyshortcuts="Control+Enter"
                  >Send denial</button
                >
              </div>
            </form>
          {/if}
        </div>
      </li>
    {/each}
        </ul>
      </li>
    {/each}
    {/if}
  </ul>
  </div>
</section>

<style>
  .approvals {
    /* At most about half the column: the agents stay in sight. */
    flex: 0 1 auto;
    max-height: 50%;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 10px 12px;
  }
  /* While something waits, it keeps room for a request even in a short
     column (the stacked layout): a 26 px strip shows only who asks. */
  .approvals:not(.idle) {
    min-height: min(100%, 220px);
  }
  /* A request in focus is the one thing to decide: the rest of the window dims. */
  .approvals:not(.idle):has(.card:focus-within) {
    position: relative;
    z-index: 5;
  }
  .approvals:not(.idle):has(.card:focus-within)::before {
    content: "";
    position: fixed;
    inset: 0;
    z-index: -1;
    background: var(--scrim);
  }
  .body {
    flex: 0 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .approvals.idle {
    position: relative;
    padding: 0;
  }
  .idle .body {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 1;
    padding: 10px 12px;
    background: var(--s4);
    box-shadow: var(--card-shadow);
  }
  /* Visually hidden, like .visually-hidden elsewhere. */
  .idle:not(:focus-within) .body,
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    margin: 0 0 8px;
  }
  h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }
  .count {
    color: var(--warn);
    font-weight: 500;
  }
  /* A 24 px target that does not make the heading row (and so the column
     above the request) taller: the request's details stay in sight. */
  .toggle {
    margin-block: -3px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
    overflow: auto;
  }
  .card {
    padding: 12px 12px 10px;
    border-radius: 12px;
    background: var(--s2);
    /* A warning bar on the leading edge marks what waits for a decision. */
    box-shadow:
      inset 3px 0 0 var(--warn),
      var(--card-shadow);
    /* Its top shows below the group head that sticks over the list. */
    scroll-margin-top: 56px;
  }
  /* The card the keys act on, however focus got there (the shortcut has
     modifiers, so browsers do not count it as keyboard focus). Inside: the
     list's scrolling would clip a ring outside the card. */
  .card:focus-within {
    box-shadow:
      inset 3px 0 0 var(--warn),
      inset 0 0 0 2px var(--accent),
      var(--card-shadow);
  }
  .card:focus-visible {
    outline: none;
  }
  /* Who asks stays in sight while a tall request scrolls under it. */
  .who {
    position: sticky;
    top: 0;
    z-index: 1;
    padding-bottom: 2px;
    background: var(--s1);
  }
  .session {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    color: var(--text-2);
    font-size: 13px;
  }
  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--warn);
  }
  h3 {
    margin: 6px 0 2px;
    font-size: 16px;
    font-weight: 600;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }
  p {
    margin: 0;
  }
  .line,
  .empty,
  .notice:not(:empty) {
    margin: 0 0 4px;
    color: var(--text-2);
    font-size: 13px;
    overflow-wrap: anywhere;
  }
  .notice:not(:empty) {
    color: var(--warn);
  }

  /* Risk chips: what reaches past the sandbox or the project. */
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 8px;
    margin: 4px 0 0;
    font-size: 13px;
    color: var(--text-2);
  }
  .chips:empty {
    display: none;
  }
  .chip {
    display: block;
    width: fit-content;
    max-width: 100%;
    margin: 4px 0 2px;
    padding: 2px 8px;
    border-radius: var(--radius-xs);
    font-size: 13px;
    font-weight: 500;
    overflow-wrap: anywhere;
  }
  .chips .chip {
    margin: 0;
  }
  .chip.warn {
    background: var(--warn-soft);
    color: var(--warn);
  }
  .chip.danger {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .fact.key {
    color: var(--warn);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .cards {
    gap: 8px;
    overflow: visible;
  }
  .worker:empty {
    display: none;
  }
  .worker {
    margin: 0;
    font-size: 13px;
    color: var(--text-2);
  }
  .object {
    padding: 1px 6px;
    font: 500 14px / 1.4 var(--mono);
    background: var(--s3);
    border-radius: var(--radius-xs);
  }
  /* Label and value side by side: the buttons stay in sight in a short column. */
  dl {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    /* Scrolled to whole: fractional heights would leave a sliver clipped. */
    scroll-margin: 4px;
    align-items: baseline;
    gap: 4px 12px;
    margin: 8px 0 0;
    padding: 8px 10px;
    border-radius: 10px;
    background: var(--s1);
  }
  dt {
    color: var(--text-2);
    font-size: 13px;
  }
  dd {
    margin: 0;
  }
  pre {
    margin: 0;
    max-height: 160px;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-size: 13px;
  }
  /* Muted: every Linux card has gaps, and the warning colour on every card
     would teach people to skip the real warnings. */
  .extra {
    margin: 8px 0 0;
    color: var(--text-2);
    font-size: 13px;
  }
  .extra summary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 6px;
    margin-left: -6px;
    border-radius: var(--radius-sm);
    list-style: none;
    cursor: pointer;
  }
  .extra summary::-webkit-details-marker {
    display: none;
  }
  .extra summary:hover {
    color: var(--text);
    background: var(--s3);
  }
  .extra[open] summary .icon {
    transform: rotate(90deg);
  }
  .extra p {
    margin: 4px 0 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 10px;
  }
  .after {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
  }
  .after .quiet {
    margin-left: -6px;
    font-size: 13px;
  }
  .wait:empty {
    display: none;
  }
  .wait {
    flex: 1 1 12em;
    color: var(--warn);
    font-size: 13px;
    /* Scrolled to whole: fractional heights would leave a sliver clipped. */
    scroll-margin: 4px;
  }
  button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font: inherit;
    font-weight: 500;
    padding: 4px 10px;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--radius);
    cursor: pointer;
  }
  button:hover {
    background: var(--s4);
  }
  button.allow {
    position: relative;
    overflow: hidden;
    background: var(--accent);
    color: var(--accent-ink);
  }
  /* Bordered quiet: Deny must be found without hovering. */
  button.deny {
    background: var(--s3);
  }
  /* The wait before Allow counts, as a 2 px fill along the button's foot. */
  .fill {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 2px;
    width: 100%;
    background: currentColor;
    transform-origin: left;
    animation-name: arm;
    animation-timing-function: linear;
    animation-fill-mode: both;
  }
  @keyframes arm {
    from {
      transform: scaleX(0);
    }
    to {
      transform: scaleX(1);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .fill {
      animation: none;
      transform: scaleX(0.5);
    }
  }
  button.allow:hover {
    filter: brightness(1.08);
  }
  button.quiet,
  .toggle {
    background: transparent;
    color: var(--text-2);
    font-weight: 400;
    padding-inline: 6px;
  }
  button.quiet:hover,
  .toggle:hover {
    background: var(--s3);
    color: var(--text);
  }
  .toggle,
  .more {
    padding: 1px 8px;
    font-size: 13px;
  }
  .more {
    margin-top: 6px;
  }
  button[aria-disabled="true"] {
    opacity: 0.55;
    cursor: default;
  }
  kbd {
    padding: 1px 5px;
    border-radius: 5px;
    background: color-mix(in srgb, currentColor 14%, transparent);
    font: 500 12px / 1.3 var(--font);
  }
  form {
    margin-top: 10px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  label {
    color: var(--text-2);
    font-size: 13px;
  }
  textarea {
    font: inherit;
    color: inherit;
    padding: 6px 8px;
    background: var(--s3);
    border: 0;
    border-radius: var(--radius);
  }
  form .actions {
    margin-top: 0;
  }
  .icon {
    flex: none;
    width: 15px;
    height: 15px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .error {
    color: var(--danger);
    margin: 0 0 6px;
  }
  @media (forced-colors: active) {
    .card,
    .chip {
      border: 1px solid CanvasText;
    }
    .card:focus-within {
      outline: 2px solid Highlight;
      outline-offset: -3px;
    }
    button {
      border: 1px solid ButtonText;
    }
    button[aria-disabled="true"] {
      color: GrayText;
    }
  }
</style>
