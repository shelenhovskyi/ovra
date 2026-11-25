# Agent orchestrator: first-run simplification

Build: cbe74a1, headless (scripted fake agent), Chromium 158.0.8127.36, 1280x800.
Scope: the "More" header menu (Audit log, Policies, Network, Servers, Browser) with
its shortcuts and palette commands, "One agent, here" / "Plan and split" + Start,
Mod+Enter, several tasks in the Tasks list, the quiet Approvals section (hidden
when empty, an overlay when focused), the Reviews inbox, the new account picker
next to Start. Fixtures: `small` (demo-app, seeded), `monorepo`.

## Summary

Andrii can still do the whole job: he starts tasks with Ctrl+Enter, answers
approvals with Alt+Shift+A and Y/N, and reviews workers' diffs in the inbox.
Every advanced panel can still be opened from the palette. The worst problems
show up once more than one task runs. A request that arrives while the Review
inbox is open is hidden and cannot be reached. Two "Plan and split" tasks give
workers with the same names, and toasts use three different naming schemes. Two
tasks also run eight agents at once, while the form says "up to 3". The
simplification itself made the expert path slightly slower in a few places but
did not break it: the More menu costs one more click and shows no shortcuts, and
the Tasks list now sits below the setup card, the trust card and the start form.

## Findings

### ORCH-1 major: an approval that arrives while the Review inbox is open is hidden and cannot be reached

- **Area:** ui/src/inbox (the dialog), ui/src/notify (toast layer), ui/src/approvals
- **Steps:** 1. Start "Plan and split" ("Write notes") and allow the three
  workers' writes. 2. Press Reviews (header shows "Reviews (3 waiting)") and
  start reading a worker's diff. 3. Another agent asks for approval (in my run a
  direct session started through `agents.start` from a script: "Run ls -la").
  4. Press Alt+Shift+A.
- **Expected:** while reviewing, Andrii sees that an agent is blocked on him
  (a count in the dialog, or a toast above it), and Alt+Shift+A takes him to the
  request.
- **Actual:** the inbox covers 1232x752 of the 1280x800 window and is
  `aria-modal`. The toast "Session s5 needs approval: Run ls -la" exists in the
  DOM but is drawn under the dialog (`elementFromPoint` on the toast hits
  `inbox-title`; toasts use z-index 10, the inbox 90). Nothing inside the inbox
  mentions the waiting request. Alt+Shift+A does nothing: focus stays on the
  inbox item `r1`. Screenshot `orchestrator-inbox-hides-new-approval.png`
  (s5/40).
- **Why it matters for Andrii:** reviewing diffs is his main job, and he does it
  while other agents work. An agent waiting unseen for minutes wastes its
  session and his plan's usage. Now that the right column is quiet, the inbox
  was the one place he could have noticed it.
- **Suggested fix:** draw toasts above modal dialogs, or have the inbox show a
  line in its header ("2 approvals waiting, Alt+Shift+A"). Let `approvals.focus`
  close (or step out of) the inbox and return to it afterwards, the same way the
  inbox already gives focus back when it closes.

### ORCH-2 major: workers of two tasks get the same names, and toasts name the same agent three ways

- **Area:** ui/src/agents (`sessions.name`, Tasks list), ui/src/approvals (card
  header), ui/src/notify (`agent-events.ts`)
- **Steps:** 1. Select "Plan and split", type "Task A", Ctrl+Enter. 2. Type
  "Task B", Ctrl+Enter (the mode stays on split).
- **Expected:** every agent has a name that is unique on screen, for example
  "Task B / docs", and the toast, the approval card and the Tasks row use that
  same name.
- **Actual:** the Tasks list has "Worker docs", "Worker tests" and "Worker lint"
  twice each. The rows are told apart only by indentation under "Task: Task A"
  and "Task: Task B", and by a `title` tooltip ("Branch ovra/t1/docs" or
  "ovra/t2/docs"). The approval card says "Worker docs in demo-app asks to use
  Write". Only the branch line "ovra/t1/docs" tells which task it belongs to.
  The four toasts on screen read "Write the lint notes. needs approval",
  "Write the docs notes. needs approval", "Session s7 needs approval" and
  "Session s8 needs approval". A direct session started from a script showed as
  "Session s5" in its toast even though `agents.start` returned the title
  "Background job". So the toast uses whatever name is known when the request
  arrives. Screenshot `orchestrator-two-splits-same-names.png` (s6/50).
- **Why it matters for Andrii:** with 6 cards waiting he answers with Y/N at
  speed. "Worker docs" twice, plus "Session s7", gives him no way to tell which
  task he is allowing or denying. Allowing the wrong one is the error the
  approvals design works hardest to prevent.
- **Suggested fix:** name workers with their task ("Task B: docs"), or show the
  task name on the card and in the Tasks row. Build the toast text when it is
  shown, or update it when the session's title and role arrive, so it never
  falls back to "Session sN" once a name exists.

### ORCH-3 major: two "Plan and split" tasks run 8 agents at once; the form promises "up to 3"

- **Area:** crates/core/src/orchestration (limits), ui/src/agents (mode hint)
- **Steps:** 1. Plan and split "Task A", Ctrl+Enter. 2. Plan and split
  "Task B", Ctrl+Enter. 3. Optionally start "One agent, here" too.
- **Expected:** the default cap of 3 parallel agents (CLAUDE.md, ADR 0006
  rules) applies across tasks. If the second task cannot start its workers
  yet, the form says so ("3 agents are working; Task B's workers start when
  one finishes").
- **Actual:** both tasks start with no error or note. 2 coordinators and 6
  workers run, and 6 approval cards wait at once. The only limit I found is
  per task (`spawn_blocked` in `orchestration/tasks.rs`, `max_workers`). The
  hint under "Plan and split" still says "gives parts to up to 3 agents".
- **Why it matters for Andrii:** he does want 5 to 10 agents. But he needs to
  trust the number Ovra shows, because it decides his usage limits and his
  review load. The project rule says agents are capped by default. Today the
  cap neither holds nor tells him it was raised.
- **Suggested fix:** enforce the documented default across sessions in the
  core and refuse or queue with a clear message. Show "N of 3 agents working"
  next to Start. If the cap is meant to be per task, fix the rule and the hint
  instead. This is an owner decision because it touches an ADR rule.

### ORCH-4 major: once tasks run, the Tasks list is buried under the setup card, the trust card and the start form

- **Area:** ui/src/agents (AgentsPanel order), ui/src/agents/setup
- **Steps:** 1. Start one "here" agent and one "Plan and split" task (5
  sessions, 4 approvals waiting). 2. Look at the right column at 1280x800.
- **Expected:** after the first session, the column turns from a first-run
  checklist into a working view. The plan (item E) says the checklist order
  holds "until a session exists". Approvals come first, then the Tasks list,
  then the selected task. The start form stays one field and one row.
- **Actual:** Approvals takes 350 px (its 45 % cap). Below it, the Agents panel
  (390 px visible, 999 px of content) still shows the "Claude is ready /
  Change" row, the trust card with "Trust project" and a two-line explanation,
  the prompt, the mode switch, the mode's three-line hint and Start. The
  "Tasks" heading is at y = 720 and the list at y = 734, so 1.5 rows of 5 are
  visible (s2/12-split, s6/50). Answering approvals does not help much: the
  panel still has to be scrolled to see all workers.
- **Why it matters for Andrii:** the status column ("Needs approval", "Waiting
  for you") of every agent is his dashboard, and he reads it far more often than
  he starts a task. Today it is the one thing out of sight.
- **Suggested fix:** once the project has a session, put Tasks above the start
  form and fold the Claude row and the trust card into one line ("Claude ready
  · project not trusted"). Show the mode hint only on focus or as the radio's
  description, and keep the form at a fixed small height.

### ORCH-5 minor: the More menu costs a click, hides the shortcuts and uses other names than the palette

- **Area:** ui/src/shell (MoreMenu.svelte), ui/src/keys (default keys for the
  panels)
- **Steps:** 1. Click More (menu opens in 33 ms). 2. Read the items. 3. Open
  the palette and search "polic", "network" and "browser". 4. In the menu,
  press P.
- **Expected:** the menu shows each item's shortcut, as the empty main area
  does ("Find a file Alt+P"), so the menu teaches the keys. Names match the
  palette. Panels used every day by an orchestrator have a key.
- **Actual:** no shortcut is shown in the menu, though Servers has Ctrl+Shift+S
  and Browser has Ctrl+Shift+B. Audit log, Policies and Network have no
  shortcut at all, so the fast path is now Ctrl+Shift+P, a search and Enter
  (about 8 keys), or 2 clicks where it used to be 1. Names differ from the
  palette: "Policies" is "Project policies", "Network" is "Network and
  secrets", and "Browser" is "Agent browser", so searching for what the menu
  said ("network") works, but "agent browser" from the menu's own description
  does not. Pressing P in the open menu does not jump to Policies (no
  typeahead). The old test ids `open-audit`, `open-policy`, `open-network`,
  `open-servers` and `open-browser` are gone, and the menu items have none, so
  scripts that drove those buttons break.
- **Why it matters for Andrii:** he checks the Audit log and Network after every
  batch of agents. Hiding them was right for a newcomer, but for him the
  replacement path should be faster than before, not slower. He also drives
  Ovra from scripts.
- **Suggested fix:** show `formatShortcut` on the right of each menu item. Use
  the palette titles as menu names, or the reverse. Add default keys for Audit
  log (for example Mod+Shift+L) and Network. Add first-letter typeahead in the
  menu. Give the items `data-testid="more-audit"` and so on.

### ORCH-6 minor: after answering the last request with the mouse, the empty Approvals overlay stays over the agents and eats the next click

- **Area:** ui/src/approvals
- **Steps:** 1. Start a task, answer every request by clicking Allow. 2. Look
  at the top of the right column. 3. Click the text under the overlay (the
  mode hint in my run).
- **Expected:** with nothing left, the section goes away. The overlay is meant
  only for keyboard users who jumped there.
- **Actual:** clicking Allow puts focus in the list. After the last answer
  focus moves to the heading, so the `idle:focus-within` overlay ("Approvals /
  Nothing waits for your approval", 339x54 px) stays over the top of the
  Agents panel. When the panel is scrolled, it covers the prompt and the mode
  hint. The first click there focuses `<body>` and does nothing else.
  Screenshot `orchestrator-approvals-overlay-after-mouse.png` (s4b/31).
- **Why it matters for Andrii:** he clears approvals in bursts with the mouse
  and then goes straight to the task list or the prompt. A box that stays open
  and a lost click make the simplified column feel unreliable.
- **Suggested fix:** when the last request is answered by pointer, give focus
  back to where it was before (`returnTo`) or blur, as Escape does. Keep the
  overlay only when focus came from the shortcut. Make the overlay's
  "Nothing waits" fade out after a short time and stay for screen readers.

### ORCH-7 minor: the start mode resets on reload, and neither mode nor account is in the palette

- **Area:** ui/src/agents (AgentsPanel), ui/src/keys
- **Steps:** 1. Pick "Plan and split". 2. Reload the page. 3. Open the palette
  and search "split" or "account".
- **Expected:** the last mode is kept, at least for the project. Palette
  commands such as "New task: plan and split" and "Switch Claude account" exist.
- **Actual:** after reload the mode is "One agent, here" again. The account,
  by contrast, is kept, so the two settings next to Start behave differently.
  The palette has only "New task for Claude", "Interrupt the agent's turn" and
  "Stop the agent". There is nothing for mode or account, and nothing to move
  between tasks (with 8 rows, every move is a click).
- **Why it matters for Andrii:** Ctrl+Enter starts whatever the hidden state
  is. After a reload, his habitual split becomes a direct agent in the main
  checkout, which then fails with "another agent already works in this
  project's main checkout" if one is already there.
- **Suggested fix:** keep the mode per project (local storage). Add palette
  commands for each mode, for "Next task" and "Previous task" (with
  "Needs approval" first), and for switching the account.

### ORCH-8 minor: the account picker changes the project's default as soon as it is touched, and tasks do not show their account

- **Area:** ui/src/agents/setup (AccountPicker), ui/src/agents (Tasks rows)
- **Steps:** 1. Add a second account ("Work", folder `.claude-work`).
  2. "Account [Default ▾]" appears between "Ctrl+Enter starts." and Start.
  3. Pick Work without starting anything. 4. Reload.
- **Expected:** the picker chooses the account for this start. If it also
  becomes the project's default, it says so. Each Tasks row shows its account
  when there is more than one.
- **Actual:** the choice is saved as the project's account immediately
  (`agents.set_project_account` on change) and is still "Work" after reload.
  The only label is "Account". The Tasks row for the new session reads
  "Refactor auth · No sandbox · Needs approval", with no account shown. The
  account appears only in the selected task's billing line, and only when the
  runtime reports billing (the fake agent reports none).
- **Why it matters for Andrii:** he splits work and personal usage. A test
  click on the picker quietly moves every later task, including Plan-and-split
  workers, to the other plan.
- **Suggested fix:** label it "Account for this project", or save it only on
  Start. Add a small account tag to Tasks rows when more than one account
  exists.

### ORCH-9 polish: a second "One agent, here" fails with an accurate message but no way forward

- **Area:** ui/src/agents
- **Steps:** with one direct agent running, type a second task and press
  Ctrl+Enter.
- **Expected:** the message suggests the alternative.
- **Actual:** "Could not start: another agent already works in this project's
  main checkout". The prompt is kept, which is good.
- **Suggested fix:** add "Choose Plan and split to run it on its own copy" with
  a button that switches the mode and starts.

## What worked well

- Ctrl+Enter starts from the prompt, focus stays in the prompt, the field is
  cleared, and the first approval appears 82 ms later without taking focus.
- Alt+Shift+A, then Y or N, then arrows is still the fastest way to clear
  requests. Cards arm after a short delay, and the next card is never focused
  when it arrives, so typing in the prompt cannot answer anything.
- "Hide" folds the queue to "Approvals (3 waiting)", and a new request shows
  the count again. Nothing waiting was ever invisible in the column itself.
- The More menu follows the menu-button pattern: ArrowDown on the button opens
  it on the first item, and Escape returns focus to More. Every item's command
  is also in the palette.
- "Reviews (3 waiting)" in the header is the right count to keep visible, and
  the inbox lists the three workers with "also in another open review" overlap
  markers.
- The worker approval card says "Works in its own copy of the project, on
  branch ovra/t1/docs". That line is the one clue that separates two tasks
  today (see ORCH-2).
