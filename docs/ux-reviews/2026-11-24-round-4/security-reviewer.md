# Sofia, the careful approver: Round 4 (2026-11-24)

Build: 81d16b1, headless, Chromium 158.0.8127.36, 1280x800.
Scope: approvals (permission requests), the Agents panel, Claude onboarding and
"Trust project", the review inbox (Alt+Shift+R), direct mode, terminals.
Fixtures: `small` (and `empty` as a second project for runs with several
requests at once), with the scripted fake agent and with `--claude
denied_outside_folder | interrupted | edit_allowed` (real driver, fake-claude).

## Summary

Sofia can run an agent and answer its requests, and the basics are careful:
nothing ever takes focus, Enter on a card does nothing, there is no "always"
button, and the 500 ms guard stops a press that comes too soon. But she can
still let things through by accident. Holding Y for 2.5 s allowed four requests
from two projects, including two that arrived during the hold. A card that
disappears moves the next one under her pointer. A toast's "Review" button
focuses a different request from the one it names. Worst of all, Reject in
direct mode silently undid her own edit, which the inbox had shown as the
agent's. Trust is explained well for hooks. It does not warn that trusting a
project can switch off Ovra's approval cards for every shell command.

## Findings

### SEC-1 blocker: Holding Y allows every waiting request, including ones that arrive during the hold

- **Area:** ui/src/approvals
- **Steps:** 1. Open `demo-app` in one tab and `empty` in another, and start
  one fake-agent session in each (two "Run ls -la" cards). 2. Wait 700 ms,
  focus the first card. 3. Hold Y for 2.5 s (keydown repeats every ~33 ms, as
  OS auto-repeat does).
- **Expected:** One request is allowed. Repeated keydowns (`event.repeat`) are
  ignored, and the next card needs a new, separate press.
- **Actual:** Four `permissions.answer … "allow"` frames, about 500 ms apart
  (perm1 to perm4). These were both `ls -la` requests and both `Edit README.md`
  requests, which arrived while Y was held. The panel ends with "Nothing waits
  for your approval." (s3 `session.log`, `01-after-hold-y.png`). The guard only
  spaces the approvals out. Focus moves to the next card after each answer, so
  the repeating key keeps approving.
- **Why it matters for Sofia:** A key that sticks or is held a little long
  approves shell commands and edits she never saw, across projects. This is
  exactly the accident she relies on Ovra to prevent.
- **Suggested fix:** Ignore `event.repeat` in `onKeydown`. Require a `keyup`
  of the answering key before any card can be answered again. Arm the next
  card 500 ms after that keyup, not after the answer.

### SEC-2 blocker: Reject in direct mode silently reverts the person's own edit

- **Area:** crates/core/src/orchestration (direct attribution), ui/src/inbox
- **Steps:** 1. `--claude interrupted`, open `demo-app`, start the recorded
  task (`ping -c 120 127.0.0.1`), Allow. 2. While the command runs, the person
  saves their own change to `src/lib.rs` (adds `// my own careful work` and
  `checked_add`). The terminal or the editor works the same way. 3. Interrupt,
  reply "Never mind…", Stop. 4. Alt+Shift+R, select the item, Reject…, then
  Reject the changes.
- **Expected:** The inbox marks `src/lib.rs` as changed by the person, or by
  both, at least as "may include your edits". Reject keeps it and lists it as a
  conflict, or says what will be lost and how to get it back.
- **Actual:** `review.list` reports `src/lib.rs` with `"by":"agent"` and no
  marker. The hint says only "Reject restores the agent's files from the start
  of the session." After Reject the file is back to `a + b` and the person's
  edit is gone. The outcome says "Rejected." with no conflicts
  (`03-confirm.png`, s6 log). The spec rule causes this: "A person's edit
  during a running Bash call is the agent's". The only copy left is a reflog
  entry of `refs/ovra/snapshot`, and the UI never mentions it.
- **Why it matters for Sofia:** She accepted a review flow that promises
  "changes made outside the agent stay as they are". Here her own work was
  thrown away, and nothing warned her.
- **Suggested fix:** For files that changed during a Bash interval, record
  whether the editor or terminal saved them in that interval. Mark such files
  "may include your edits" and treat them as "both" on reject. When Reject
  overwrites anything, offer "Undo reject" from the pre-reject snapshot in the
  outcome line, or at least name the snapshot.

### SEC-3 major: A card that disappears puts the next request under the pointer, and the click allows it

- **Area:** ui/src/approvals
- **Steps:** 1. Two pending requests, from sessions in `empty` and `demo-app`.
  Both are armed. 2. The person moves the pointer to the first card's Allow
  button. 3. Meanwhile the first request is answered elsewhere (a second
  window here; a session ending or failing has the same effect). 4. The click
  lands about 80 ms later.
- **Expected:** A click that arrives just after the layout changed does
  nothing. The new card under the pointer waits 500 ms like a new one.
- **Actual:** `permissions.answer {"id":"perm2","decision":"allow"}` was sent
  for Task A's `ls -la`, which the person never looked at (s2 log,
  `02-after-stale-click.png`). `lastAnswer` resets only for answers given in
  this tab, so removals from outside do not re-arm the cards.
- **Why it matters for Sofia:** She approves after reading one card, and the
  approval goes to a different one.
- **Suggested fix:** Whenever the list changes, re-arm every card whose
  position moved, or the whole list, for 500 ms. That covers
  `permissions.resolved` from elsewhere, a withdrawn request, a reload or a
  new card above. Also briefly highlight the card that moved into place.

### SEC-4 major: The toast's "Review" button focuses a different request from the one it names

- **Area:** ui/src/notify, ui/src/approvals (`approvals.focus`)
- **Steps:** 1. Two pending requests (Task B in `empty`, then Task A in
  `demo-app`). 2. Click "Review" on the toast "Task A needs approval: Run ls
  -la".
- **Expected:** Focus goes to Task A's card.
- **Actual:** Focus goes to the first card, "Task B asks to use Bash" (s8
  log). The next Y answers Task B's request.
- **Why it matters for Sofia:** She acts on what the toast said. The key then
  answers another session's request, possibly in another project.
- **Suggested fix:** Pass the request id with the toast action and focus that
  card. `approvals.focus` without an id can keep focusing the oldest card.

### SEC-5 major: Allow and Deny look disabled for seconds but already work

- **Area:** ui/src/approvals
- **Steps:** 1. Open `demo-app` and leave the page idle for 6 s (reading the
  task). 2. Start a fake-agent session. 3. Sample `aria-disabled` on Allow
  every 100 ms. 4. In a second run, click Allow at 700 ms.
- **Expected:** The button looks enabled at 500 ms, when it starts to work.
- **Actual:** `aria-disabled="true"` until 6,746 ms, which is the idle time
  plus 500 ms (s5). A click at 700 ms, while the button is dimmed,
  allowed the request (s5b: `permissions.answer … allow`,
  `01-looks-disabled.png`). With `page.clock`, Y at 400 ms is correctly
  ignored, Y at 600 ms allows, and `aria-disabled` is still `true` (s11).
  Cause: the re-arm timer waits `armedAt - now`, but `now` was last set
  whenever something else updated it.
- **Why it matters for Sofia:** The visual state is her cue for "not yet".
  She cannot trust a button that looks disabled but accepts her click.
  A screen reader user hears "dimmed" too.
- **Suggested fix:** Compute the delay from `Date.now()` and set `now` when a
  request arrives. Add a unit test with fake timers that starts after a long
  idle time.

### SEC-6 major: Approval cards do not say which project a request comes from

- **Area:** ui/src/approvals (needs the session's project; `agents/sessions`
  or the protocol)
- **Steps:** 1. Sessions in `demo-app` and `empty`, both reaching "Edit
  README.md".
- **Expected:** Each card names the project, for example "demo-app · Task A
  asks to use Edit".
- **Actual:** Both cards read "… asks to use Edit / Edit README.md" with
  `file README.md` (s2 log). The panel lists requests from every project
  while the header shows only the open one.
- **Why it matters for Sofia:** Allowing an edit in the wrong repository
  breaks her own rules for production repositories.
- **Suggested fix:** Show the project name on every card and toast. Group or
  sort cards by project, and highlight cards that are not from the open
  project.

### SEC-7 major: Inside or outside the project is not shown on file and command requests

- **Area:** ui/src/approvals (`describe.ts`), crates/agent-runtime/src/policy.rs
- **Steps:** 1. `--claude edit_allowed`: the card reads "Edit
  /tmp/ovra-ux-…/demo-app/README.md". 2. `--claude denied_outside_folder`:
  the card reads "Write /notes.txt", then "Why it asks: Ovra asks before an
  agent changes files outside its folder. (hook)", then "Agent's note:
  /notes.txt". 3. Fake agent: "Edit README.md", a relative path.
- **Expected:** File paths are shown relative to the project with an
  "inside project" label, or absolute with a clear "OUTSIDE the project"
  badge. This holds for Read and Bash too, not only when Ovra's hook asked.
- **Actual:** She has to compare long absolute paths against the project
  root, which the UI never shows. "Outside" appears only as the reason line,
  and only for Edit, Write and NotebookEdit. A `Bash` command that writes
  outside the folder gets no hint (`policy.rs` passes all Bash). The
  "(hook)" and "(other)" suffixes are internal jargon, and "Agent's note"
  repeats the path.
- **Why it matters for Sofia:** "Inside or outside the project" is her first
  question for every request.
- **Suggested fix:** Have `describeTool` take the project root. Show paths
  relative to it with an Inside or Outside badge, and show the badge on
  `Read`, `Glob` and `Grep` paths too. Drop the kind suffix and the
  duplicate note. Say plainly that shell commands are not checked for paths.

### SEC-8 major: "Trust project" can turn off approvals for every shell command, and the card does not say so

- **Area:** ui/src/agents/setup (`texts.ts`, `ProjectTrust.svelte`)
- **Steps:** 1. Add `.claude/settings.json` with `permissions.allow:
  ["Bash(*)", "Edit", "WebFetch"]`, a PostToolUse hook, `env.AWS_PROFILE` and
  `mcpServers`. 2. Agents panel, Review and trust….
- **Expected:** Allow rules get the same weight as hooks. For example: "Bash(*)
  lets the agent run any shell command without asking you. Ovra will not
  show approval cards for these." The summary line says that trusting changes
  what Ovra asks about.
- **Actual:** Only the hook gets "runs code" and the yellow warning. "Tools
  allowed without asking you: Bash(*), Edit, WebFetch" is a plain list with
  no warning (`01-review.png`). The summary line names "settings, hooks,
  agents, commands and skills", and does not mention permissions. A later
  added rule `Write(/**)` also shows without a warning. Ovra's own hook asks
  only about file edits outside the folder, so after trusting, `Bash(*)`
  means no cards at all.
- **Why it matters for Sofia:** Trust is the switch that decides whether she
  sees approval cards at all. She would trust a project for its slash
  commands and lose her approvals without knowing it.
- **Suggested fix:** Treat `allow_rules` as risky. Mark wildcards and broad
  tools (`Bash(*)`, `Bash`, `Write`, `WebFetch`) as "skips your approval".
  Include them in the warning and the summary text. Consider trusting
  settings without allow rules as a separate, safer choice.

### SEC-9 major: Reject is one keypress away after "Reject…", with focus on the destructive button

- **Area:** ui/src/inbox
- **Steps:** 1. Open a direct review item. 2. Focus "Reject…" and press
  Enter. 3. Press Enter again.
- **Expected:** The confirmation takes focus on "Keep reviewing", or arms
  like approvals do. It lists the files that will be restored and says
  whether this can be undone.
- **Actual:** Focus lands on "Reject the changes", so the second Enter (a
  double Enter) rejects at once. In s6, `review.reject` was sent 3 ms after
  the confirm appeared. The text is one generic line, with no file list and
  no "cannot be undone", and the outcome is just "Rejected.".
- **Why it matters for Sofia:** Combined with SEC-2, a double Enter destroys
  work. She expects destructive review decisions to ask first, really.
- **Suggested fix:** Focus "Keep reviewing" and arm "Reject the changes" for
  500 ms. List the files that will change on disk, and say how to recover.

### SEC-10 minor: Stopping a session hides its request silently, and the toast says "finished"

- **Area:** ui/src/notify, ui/src/approvals
- **Steps:** 1. Task B waits for "Run ls -la". 2. Stop Task B.
- **Expected:** "Task B stopped. Its request to run `ls -la` was withdrawn
  (not run)."
- **Actual:** The card disappears and focus falls to `<body>`. The toast
  reads "Task B finished and waits for you" although the status is Stopped
  (s8 log).
- **Why it matters for Sofia:** She needs to know whether the request ran.
  "Finished" suggests it did.
- **Suggested fix:** Use a separate toast text for stopped sessions and name
  any withdrawn requests. Keep focus in the approvals list, as after an
  answer.

### SEC-11 minor: The same session has different names on toasts and cards

- **Area:** ui/src/agents (`sessions.svelte.ts`), ui/src/notify
- **Steps:** 1. A session in another project asks for a permission. 2. Reload
  the page while a card is pending.
- **Expected:** Always the task name, for example "Task B".
- **Actual:** The toast says "Session s1 needs approval", while the card says
  "Task B asks to use Bash". After a reload the card says "Session s2 asks to
  use Bash" (s2, s8).
- **Why it matters for Sofia:** She matches requests to the tasks she set.
  "s2" tells her nothing.
- **Suggested fix:** Load the session info for every pending request,
  whatever its project, before rendering the card and the toast.

### SEC-12 minor: Revoking trust does not affect running sessions and shows nothing about them

- **Area:** ui/src/agents/setup
- **Steps:** 1. Trust `demo-app` and start a session. 2. Click "Stop
  trusting".
- **Expected:** "Stop trusting" says which running sessions still have the
  project's hooks and settings loaded, and offers to stop them. The trust
  state is visible outside the Agents panel, for example in the header.
- **Actual:** The card only says "Changes apply to sessions started
  afterwards." Running sessions keep the hooks, and nothing tells her which
  ones. The header shows only "Ovra | demo-app | Reviews".
- **Why it matters for Sofia:** She revokes trust because she no longer
  trusts the hooks. They keep running anyway.
- **Suggested fix:** Mark sessions started with project settings ("with
  project settings") in the session list. After Stop trusting, list them
  with a Stop button.

### SEC-13 minor: The trust review lists MCP servers as if they load, and says nothing about `.mcp.json`

- **Area:** ui/src/agents/setup, crates/agent-runtime/src/project.rs
- **Steps:** 1. Put `mcpServers` in `.claude/settings.json` and add a
  `.mcp.json`. 2. Review and trust….
- **Expected:** "MCP servers from this project never start in Ovra" (the
  launcher passes `--strict-mcp-config` and never loads `.mcp.json`).
- **Actual:** `mcpServers: {"db":{"args":["db-mcp"],"command":"npx"}}` shows
  under "Other settings", without "runs code" and without saying it is
  ignored. `.mcp.json` is not mentioned.
- **Why it matters for Sofia:** She has to guess whether a server command
  will run. She asked for MCP servers to be named explicitly.
- **Suggested fix:** Add a fixed line or a kind "MCP servers (not loaded by
  Ovra)" that covers both sources.

### SEC-14 minor: The trust list scrolls, but "Trust project" is always in reach, and opening the review drops focus

- **Area:** ui/src/agents/setup
- **Steps:** 1. A project with several kinds of settings. 2. Review and
  trust….
- **Expected:** Focus moves to the review. "Trust project" is visible after
  the whole list, or the list shows "n more below".
- **Actual:** The list is capped at 30vh. "Other settings" and "Slash
  commands" are below the fold while "Trust project" is visible
  (`01-review.png`). Focus goes to `<body>`. After trusting, focus jumps to
  the file tree.
- **Why it matters for Sofia:** She can trust without having seen everything
  that will load.
- **Suggested fix:** Focus the review heading and show a count ("6 items")
  next to the button. Keep focus on the trust card after the decision.

### SEC-15 polish: Early presses are ignored without feedback, and Y works while Deny has focus

- **Area:** ui/src/approvals
- **Steps:** 1. Press Y or click Allow within 500 ms. 2. Tab to the card's
  Deny button and press Y.
- **Expected:** A short "Wait a moment…" on the card. Inside the card, Y and
  N act only when the card itself has focus.
- **Actual:** Nothing visible happens, which invites a second press. Y on the
  focused Deny button allows, because the card's handler catches bubbling
  keys from its buttons.
- **Why it matters for Sofia:** Silent no-ops make her press again, which
  feeds SEC-1.
- **Suggested fix:** Show a brief hint while a card is not armed, and handle
  Y and N only when `event.target` is the card.

## What worked well

- Nothing takes focus: a new card never steals the keyboard. Enter and Space
  on a focused card do nothing, and only Y, N or a click answers.
- The 500 ms guard works under `page.clock`: Y at 400 ms is ignored, and a
  double press after an answer is blocked.
- There is no "always allow" button, so no lasting rule can be created by
  accident from a card.
- Deny with message: Shift+N opens it, plain Enter adds a newline instead of
  sending, Ctrl+Enter sends, and the agent gets the exact message
  (`denied_outside_folder`).
- Ovra asks about edits outside the folder even when the user's settings
  allow them, and shows why it asks.
- After a reload, pending cards come back and re-arm.
- Trust change detection is very good. Changed settings stop loading, and
  the diff shows `+ SessionStart: rm -rf ~/.ssh` with "runs code" and a
  warning.
- The inbox never opens or decides on its own. Reject has a confirmation
  step and a plain hint, and Approve explains that it keeps the changes.
- The terminal opens in the project folder and shows the path in the prompt.
