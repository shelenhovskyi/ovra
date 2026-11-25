# Andrii, the agent orchestrator: servers and agent browser

Build: 7cf1845, headless (`--fake-agent --fake-servers --fake-browser`), Chromium 158.0.8127.36, 1280x800.
Scope: managed dev servers (Servers dialog, Mod+Shift+S) and the agent browser pane (Mod+Shift+B) seen
from someone running a coordinator and several workers; the audit log's "Servers" and "Network and
secrets" filters. Fixtures: `small`, with a committed `.ovra/servers.json` (`web` 5173 and `api` 8080 on
the `ready` preset, `worker-crash` 9000 on an unknown preset), a task "Write the notes" (coordinator plus
workers docs, tests, lint) and direct "Browse ..." sessions.

Limits of the harness: the fake agent never calls `server_start`, so every server here was started by
the person; "started by an agent" is judged from the code (`ServersPanel.svelte` `who()`,
`audit/describe.ts`). Only one direct session can work in the main checkout at a time and worker
sessions of the fake agent have no sandbox, so two sessions with live pages at once could not be set
up; picking for "the wrong session" is judged from the code.

## Summary

Andrii could start servers per work dir, see "in Worker docs / started by you / run 1" on each row,
answer a navigation ask from the badge on the Browser button, and send a pick to the session it was
made in. But the Servers dialog shows one work dir at a time with no overview, a worker's server can be
restarted after its review was approved, and ending a session that has an open page throws a page error
and leaves the browser pane blank until it is hidden and shown again. The audit trail is complete but
speaks in internal ids (`srv4`, `session s2`) and never says that an approval stopped a server.

## Findings

### ORCH-1 major: ending a session with an open page breaks the browser pane

- **Area:** ui/src/browser
- **Steps:** 1. Start "Browse the login page". 2. Open the Browser pane, open `localhost:5173`, Allow.
  3. Press Stop on the session in the Agents panel. 4. Start "Browse the settings page".
- **Expected:** the pane says the first session's pages ended with it, then shows the new session with
  its address field.
- **Actual:** step 3 logs `pageerror Cannot read properties of null (reading 'id')`; after step 4 the pane
  shows only "Browser, Session [Browse the settings page], Hide" and nothing else: no address field, no
  "No pages open" line (`browser2/failure.png`; address field count 0). Hide then Browser brings it back.
  The likely cause is PageView's teardown reading `page.id` (`screencast(false)`, `picker.stop()`) after
  the pane's derived `page` turned null.
- **Why it matters for Andrii:** sessions end all day; every end with an open page silently kills the
  pane, and the next agent's navigation asks or picks need a workaround he would not guess.
- **Suggested fix:** capture `page.id` once in PageView (a `const id = page.id` at mount) and use it in
  cleanups; add a unit test that drops the page list while a PageView is mounted. Say "The session ended;
  its pages closed." when the shown session's pages vanish.

### ORCH-2 major: the Servers dialog shows one work dir at a time

- **Area:** ui/src/servers
- **Steps:** 1. Start task "Write the notes" (three workers). 2. Open Servers; start `web`, `api` in
  Main checkout; switch "Runs in" to Worker docs, start `web`; to Worker tests, start `api`. 3. Close and
  reopen the dialog.
- **Expected:** one list of every running server of the project, grouped or labelled by work dir, so
  "what runs where" is one glance; the header button shows how many run.
- **Actual:** the dialog reopens on Main checkout and lists only its three rows; Worker docs and Worker
  tests' servers appear only after picking each from the select (four selects for four work dirs,
  `servers/02-servers-main.png`, `servers/05-worker2.png`). The header button reads "Servers" whatever
  runs. The "in Worker docs" label on each row is redundant inside a per-work-dir view.
- **Why it matters for Andrii:** with five to ten workers he must open the select ten times to learn
  which pods hold ports and which crashed; a failed server in an unselected worktree is invisible.
- **Suggested fix:** add an "All work dirs" choice (default) that lists every server of the project
  with the existing "in <work dir>" label, keep the select to filter and to choose where Start runs; show
  a count of running or failed servers on the header button, like the Browser button's badge.

### ORCH-3 major: a worker's server can be restarted after its review was approved

- **Area:** ui/src/servers (and the core's `servers.restart` / `servers.start` checks)
- **Steps:** 1. In Worker docs start `web`. 2. Allow the workers' writes; approve Worker docs' review
  in the inbox. 3. Open Servers, Runs in: Worker docs.
- **Expected:** the row says the server stopped because the review was approved; Start and Restart are
  off for a worker whose work landed, or at least warn; the select marks the worker as approved or ended.
- **Actual:** the row says `exited (signal)` with no reason; Restart is enabled and starts run 2, which
  gets ready and a new loopback grant (audit: "Server web started, Run 2", "Server reachable on
  localhost:5173"), in the worktree of an approved worker (`restart/r4-restart-approved.png`). The select
  still lists "Worker docs" like a live worker.
- **Why it matters for Andrii:** the spec stops a worker's pod so nothing writes its worktree while its
  changes land; after that one click quietly brings it back, and a server that writes build output there
  can make the finished worker's tree look changed again. He also cannot tell which workers are done.
- **Suggested fix:** in the core, refuse `servers.start`/`restart` for a worker whose session ended or
  whose review was approved or taken, with a message the dialog shows; in the UI, disable Start/Restart
  for such work dirs, add the worker's state to the select ("Worker docs, approved"), and give
  `exited` a reason when Ovra stopped it ("stopped: review approved", "stopped: session ended").

### ORCH-4 major: a per-session refusal shows as "the agent browser cannot run here" and sticks

- **Area:** ui/src/browser
- **Steps:** 1. Have a sandboxed "Browse the settings page" session and a task with workers (the fake
  workers run without a sandbox). 2. Browser pane, Session: Worker docs, open `localhost:5173`.
  3. Switch Session back to "Browse the settings page".
- **Expected:** for the worker: "This session runs without a sandbox, so it has no browser" next to that
  session only; other sessions keep working.
- **Actual:** the pane shows the global notice about installing Chromium and the Linux sandbox, with
  "The core said: the agent browser runs only for sessions whose agent runs in the sandbox..." below it
  (`browser2/c7-worker-open.png`). After switching back to the sandboxed session the same notice stays
  and the address field is gone (`browser2/c8-back-sandboxed.png`) until "Try again" is pressed.
  `unavailable` is one flag for all sessions in `browser.svelte.ts`.
- **Why it matters for Andrii:** he is sent to check his Chromium install for a problem of one session,
  and every other session's browser looks broken.
- **Suggested fix:** keep "unavailable" for the whole-machine case only; store a per-session refusal
  (or have the core use a distinct error code) and show it inline for that session. Grey out sessions in
  the select that cannot browse ("Worker docs, no sandbox").

### ORCH-5 minor: Escape stops closing the Servers dialog after Start

- **Area:** ui/src/servers
- **Steps:** 1. Open Servers. 2. Click "Start web". 3. Press Escape.
- **Expected:** the dialog closes, as before the click.
- **Actual:** the Start button turns disabled while the server runs, focus drops to `<body>`, the
  dialog's keydown handler never sees Escape and the modal stays open (`restart/r0-after-escape.png`);
  Tab is no longer trapped either. Clicking behind it hits the backdrop.
- **Why it matters for Andrii:** starting a server then Escape back to the review is his most common
  move; agents driving the UI by keyboard get stuck the same way.
- **Suggested fix:** after a Start, move focus to the new row's Stop or Output button (or keep the
  button enabled and say "running"); also listen for Escape on the backdrop or document while open.

### ORCH-6 minor: the audit trail speaks in internal ids and blurs who did what

- **Area:** ui/src/audit
- **Steps:** 1. Do ORCH-2 and ORCH-3's steps. 2. Audit log, Show: Servers.
- **Expected:** "Server web (Worker docs) reachable on localhost:5173 for Worker docs", "Server web
  stopped: review approved", "started by Worker tests" with names.
- **Actual:** details read "The session may reach server srv4 (run 1) while it runs." and "Server srv4
  (run 1) stopped or the session ended." (`servers/13-audit-servers.png`). The who column of
  "Server web started, started by a person" shows "Worker docs", the work dir's session, so it reads as if
  the worker started it; servers of the main checkout show "Project", which does not say "main
  checkout". An agent's start would read "started by session s4" (raw id, `describe.ts`). The revoke
  after the approval says "stopped or the session ended", never "review approved". The dialog's intro
  still lists only "session starts, tool decisions, trust, policy and review decisions".
- **Why it matters for Andrii:** the audit is how he answers "which agent could reach which port, and
  why did it stop"; ids he never sees elsewhere force him to cross-reference by time.
- **Suggested fix:** resolve server ids to names and sessions to their names in `describe.ts` (the
  sessions store already has `name()`); label the column as the work dir ("Main checkout", "Worker
  docs") for server events and name the starter in the detail; give `loopback_revoked` and
  `server_stopped` a reason from the core (stopped, restarted, session ended, review approved/taken);
  mention servers and network in the intro.

### ORCH-7 minor: a Deny cannot be undone and later tries fail with a stale message

- **Area:** ui/src/browser
- **Steps:** 1. Browse session, open `localhost:5173` with no server running; Deny. 2. Open
  `localhost:5173/again`.
- **Expected:** the second try says it was refused because of the earlier Deny and offers to ask again
  (or to allow now).
- **Actual:** both show the same red line "http://localhost:5173 was not allowed"; no ask comes back and
  nothing tells that the earlier answer holds for the whole session (`browser2/c4-second-try.png`).
- **Why it matters for Andrii:** a misclick on Deny blocks an agent's page for the rest of the session;
  restarting the agent is the only way out.
- **Suggested fix:** word the refusal "Refused for this session (you denied it at 03:53)" and offer
  "Allow now"; clear the error line when a new open starts.

### ORCH-8 minor: pages and tabs do not say which work dir's server they show

- **Area:** ui/src/browser
- **Steps:** 1. Run `web` on 5173 in Main checkout and in Worker docs. 2. In the Browser pane, look at a
  tab of `http://localhost:5173`.
- **Expected:** the tab or the pane names the pod ("localhost:5173, web in Worker docs"), since the same
  URL means a different server per worktree.
- **Actual:** the tab shows only `http://localhost:5173`; the session select is the only hint, and the
  Agents panel's selection is independent of it (judged from the code; workers could not browse in this
  harness).
- **Why it matters for Andrii:** comparing two workers' UIs side by side, he must remember that the
  session decides the pod.
- **Suggested fix:** when a page's origin matches a granted server, add "web in <work dir>" next to the
  tabs; follow the Agents panel's selected session by default when the pane opens.

### ORCH-9 minor: picks for a session not shown in the Agents panel are easy to lose

- **Area:** ui/src/browser (with ui/src/agents)
- **Steps:** (from the code) 1. Agents panel shows session B. 2. In the Browser pane choose session A and
  pick an element.
- **Expected:** the pick says where it goes and offers to jump to session A's message box.
- **Actual:** the chip lands in session A's message box, which is not on screen; the pane's hint says
  "Picks go with your next message to this session" without naming it. In the one-session run the chip
  appeared correctly and was sent with the reply (`browser/b7-picked.png`, chips 0 after send), but the
  reply box sat below the fold, under the "finished and waits for your reply" toast that also covered the
  pick note field.
- **Why it matters for Andrii:** with several browsing agents, a pick typed for one may wait unsent while
  he messages another.
- **Suggested fix:** name the session in the hint ("go with your next message to Browse the login page")
  and add a "Write message" button that selects that session and focuses its reply box; keep toasts off
  the pane's bottom edge.

### ORCH-10 polish: a server that failed to start says only "exited (code 1)"

- **Area:** ui/src/servers
- **Steps:** 1. Start `worker-crash` (unknown preset).
- **Expected:** the row shows the last error line ("error: no such preset") or a hint to open Output.
- **Actual:** `exited (code 1)` with grey styling like a normal stop; the reason is only under Output
  (`servers/06-crash-output.png`).
- **Suggested fix:** colour non-zero exits like `failed` and show the last output line on the row.

## What worked well

- Each server row says where it runs, who started it and which run it is ("in Worker docs, started by
  you, run 1"); the dialog opens in 37 ms and keeps focus on its heading.
- Approving Worker docs' review stopped only that worker's `web` and revoked only its grant; Worker
  tests' `api` and the main checkout's servers kept running.
- Two pods use the same port (5173 in main and in Worker docs) without a clash, as the spec promises.
- A navigation to a granted server (`localhost:5173` with `web` ready) opens with no ask; an outside
  origin asks once, names the session ("Browse the login page: Let the agent open http://example.com?"),
  and the hidden pane's button announces "Browser, 1 waiting for you" with a badge.
- Picking highlights the element with Ovra's own overlay, numbers it, accepts a note, and the chip is
  sent with the next reply and cleared afterwards.
- Every grant, revoke, start, stop and browser decision is in the audit log with time and session, and
  the "Servers" and "Network and secrets" filters find them.
