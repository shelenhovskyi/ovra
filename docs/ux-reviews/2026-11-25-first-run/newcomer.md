# Nadia, the newcomer: first run (2026-11-25)

Build: cbe74a1, headless, scripted fake agent, Chromium 158.0.8127.36, 1280x800.
Scope: the whole first session after the first-run simplification (`plan.md`
items A, B, C, E, F, G): first screen and opening a folder, the header
(Reviews, More), the right column (Claude setup card, project trust card,
"What should Claude do?" form), the main-area start panel, the footer, a first
task with the fake agent, answering approvals, and finding the result in
Reviews. Fixtures: `small` ("demo-app", seeded and unseeded), `empty`.
Claude setup states other than "ready" were made by rewriting the
`agents.setup` reply with `page.routeWebSocket`.

## Summary

Nadia can open a folder, give Claude a first task, answer the approvals and,
in "Plan and split", find the result in Reviews. She does it all with the
mouse and without help, and the first screen is now calm and readable. The
weak point is the end of the task. A "One agent, here" task finishes with
"Done." and "Waiting for you": nothing says what changed, and Reviews says
"No agent changes wait for review". A "Plan and split" task ends with a toast
that does not mention Reviews, while the task's transcript shows internal
messages ("agent_id s2 ... list_agents or read_output"). She also has no
mouse path back to the project list, and a reload drops her project and
renames her task "Session s1". Overall it feels much friendlier than the
pre-plan state, but the end of the first task still loses her.

## Findings

### NEW-1 major: After a "One agent, here" task, nothing shows what Claude changed, and Reviews is empty

- **Area:** ui/src/agents (transcript, session detail); ui/src/inbox (empty-state text)
- **Steps:** 1. Open demo-app. 2. Type "Fix the typo in the readme", keep "One agent, here", press Start. 3. Allow "Run ls -la" and "Edit README.md". 4. Look for the result. 5. Click Reviews.
- **Expected:** a clear end state, for example "Claude changed 1 file: README.md", with links that open the file or its diff, and a hint that changes made "here" are already in the folder (see the Git tab).
- **Actual:** the transcript ends "Claude: Done." and "Turn ended: 0 tokens in, 0 out". "Edit README.md" is plain text: the transcript has no links or buttons at all. The main area still says "No file open". Reviews says "No agent changes wait for review." (`newcomer-one-agent-done.png`, s2 `18-reviews-after-one-agent`.)
- **Why it matters for Nadia:** she came for "agents do the work, you approve". The plan promises she finds the result in Reviews, but in the default mode she cannot, and nothing points her to the Git tab or the changed file. She ends up wondering whether anything happened at all.
- **Suggested fix:** when a session in "here" mode ends a turn after edits, show a "Changed files" block in the session detail (each file opens its diff in the main area). In the Reviews empty state, add one line: "Tasks run with 'One agent, here' change your folder directly; see the Git tab." Make tool lines for file tools (Edit, Write) links to the file.

### NEW-2 major: The task looks unfinished ("Waiting for you") while the toast says "finished", and the Plan-and-split toast does not mention Reviews

- **Area:** ui/src/notify (`agent-events.ts`), ui/src/agents (`status.ts`)
- **Steps:** 1. Run a task in either mode and allow every request. 2. Read the toast and the task row.
- **Expected:** one consistent message saying it is done and what to do next. For example "Fix the typo is done. Reply to continue, or Stop." or, for a split task, "3 changes wait for your review" with a Review button that opens the inbox.
- **Actual:** the one-agent toast says "Fix the typo in the readme finished and waits for your reply", while the row says "Waiting for you" with a Stop button. The split toast says "Task: Write notes ... finished: none of its agents is working" and has no action; meanwhile the header shows "Reviews (3 waiting)", and the four task rows all say "Waiting for you" (`newcomer-plan-finished.png`).
- **Why it matters for Nadia:** she reads messages literally. "Finished" plus "waits for your reply" makes her think she has to answer, and "none of its agents is working" sounds like an error. The only hint that her work is in Reviews is a small count in the header.
- **Suggested fix:** for a task whose workers asked for review, the toast reads "<task>: N changes wait for your review" with a "Review" button (as approval toasts already have). For an idle single agent: "<task> is done. Reply to ask for more." Use a "Done, you can reply" status label instead of "Waiting for you" when the turn ended without a pending question.

### NEW-3 major: No visible way back to the project list or to open another folder

- **Area:** ui/src/shell (header)
- **Steps:** 1. Open demo-app. 2. Try to open the `empty` folder or go back to the project list.
- **Expected:** the project name in the header is a button or menu ("Switch project", "Open folder…", "Close project"), as in VS Code's File menu or the title-bar picker.
- **Actual:** "Project demo-app" is a plain `<span>` (cursor auto, no role), so clicking it does nothing. The project list disappears once a project is open. The only ways out are the palette commands "Switch project" (which lists only known projects and has no "Open a folder…" entry) and "Close project". Neither is shown anywhere with the mouse.
- **Why it matters for Nadia:** her checklist says "a way back". She only vaguely knows Ctrl+Shift+P, so after her first folder she is stuck in it.
- **Suggested fix:** make the header project name a menu button: recent projects, "Open another folder…", "Close project". Add an "Open a folder…" item at the bottom of the Switch project palette list.

### NEW-4 major: A reload forgets the open project and turns her task into "Session s1"

- **Area:** ui/src/shell (restoring the last project); ui/src/agents (`sessions.svelte.ts:251` fallback name); ui/src/approvals
- **Steps:** 1. Open demo-app, start "Fix the typo", leave the first approval waiting. 2. Reload the page (or restart the window).
- **Expected:** demo-app opens again, and the approval card still names "Fix the typo".
- **Actual:** the header says "No project open" and the project list is back. The pending card reads "Session s1 in demo-app asks to use Bash" with a yellow "Not the project open in this window" (`newcomer-reload-session-s1.png`). The agent keeps running, but nothing on screen leads back to it.
- **Why it matters for Nadia:** she does not know what "Session s1" is, and the yellow warning suggests something went wrong. She is asked to approve a command for a task she cannot see.
- **Suggested fix:** reopen the last project on start. Fetch session titles before approval cards render, or fall back to "a task in demo-app" instead of "Session s1". On a card from another project, add "Open demo-app" instead of only the warning.

### NEW-5 major: Internal messages and jargon in the task transcript

- **Area:** crates/core (`orchestration/tasks.rs`, `tell_coordinator`) for the message text; ui/src/agents (`Transcript.svelte`) for how it is shown
- **Steps:** 1. Choose "Plan and split", start "Write notes for docs, tests and lint". 2. Allow the three requests. 3. Read the task transcript.
- **Expected:** plain lines, for example "Worker docs asked you to review its changes."
- **Actual:** "Ovra: worker docs (agent_id s2) asked for review. Check with list_agents or read_output, then go on with the task.", repeated for each worker, each followed by "Turn ended: 0 tokens in, 0 out". Tool lines read "Tool Bash", "Tool Edit", "Ovra tool", "task Write the docs notes.". The approval card says "... asks to use Bash".
- **Why it matters for Nadia:** these are instructions for the coordinator agent, not for her. Words like `agent_id`, `list_agents` and "Turn ended: 0 tokens" are exactly the code-like text that scares her.
- **Suggested fix:** mark messages to the coordinator as internal and show the person a short form (or hide them). Hide the "Turn ended" line when usage is 0, or move usage behind the session's Total line. Label tools by what they do: "Ran a command", "Edited a file", "Started a worker".

### NEW-6 minor: The trust card's summary sounds as if risky settings are already active, and Start works without a word about trust

- **Area:** ui/src/agents/setup (`ProjectTrust.svelte`, `texts.ts`)
- **Steps:** 1. Add `.claude/settings.json` with `Edit` allowed and a PostToolUse hook. 2. Open the project. 3. Read the card, then press Start without trusting.
- **Expected:** a clear current state and choice: "Not trusted yet: Claude ignores these settings. If you trust it, Claude will edit files without asking and 1 hook will run commands." Plus a line that she can start without trusting.
- **Actual:** "Claude will use Edit without asking you." and "1 hook runs commands on your computer." are shown in present or future tense, in yellow, before she has trusted anything. Start then works silently, and nothing says the project's settings were left out. With no settings, the card says "No Claude settings in this project / Trust project / Trusting it starts language servers, which run the project's code in the sandbox", so she has to decide on something she does not need yet.
- **Why it matters for Nadia:** she cannot tell whether she must click "Trust project" before her first task, or whether trusting already happened.
- **Suggested fix:** start the summary with the state ("Not trusted: Claude ignores these settings") and put the warnings in a conditional ("If you trust it: ..."). When there are no settings, collapse the card to one muted line ("Code features (language servers) are off. Turn on…") placed below the task form, not above it.

### NEW-7 minor: The details view still uses specialist words

- **Area:** ui/src/agents/setup (`ProjectTrust.svelte`, `texts.ts`)
- **Steps:** 1. Use the trust card from NEW-6 and click "Show details".
- **Expected:** plain words; specialist terms only in a tooltip.
- **Actual:** "in the sandbox (bubblewrap, Landlock, seccomp)". The group heading and its badge run together for screen readers and in copied text: "Hooks: commands that run on agent events runs code". Raw rule text such as `PostToolUse (Edit): ./tool/fmt.sh` appears with no explanation.
- **Why it matters for Nadia:** "Show details" is where she goes when she is unsure, and that is where it gets hardest to read.
- **Suggested fix:** drop the mechanism names (keep them in a tooltip or the docs). Write the badge as a separate sentence or set it apart ("· runs code"). Put one plain line above each raw rule ("After Claude edits a file, runs ./tool/fmt.sh").

### NEW-8 minor: An early click on Allow is silently ignored

- **Area:** ui/src/approvals
- **Steps:** 1. Start a task. 2. Click Allow on the first request as soon as it appears (within 0.5 s).
- **Expected:** the button looks disabled and says why, or the click counts once the short wait is over.
- **Actual:** the button has `aria-disabled="true"` but no title, and looks almost the same as when enabled. The click does nothing and the card stays as it was (s5, `40-early-click`).
- **Why it matters for Nadia:** she thinks the click missed or the app froze, and clicks again. The second click can land on the next request as it arrives.
- **Suggested fix:** show the wait (for example a short fill on the Allow button, or a "Read first…" title). Keep ignoring clicks during the wait, since that guard is for safety.

### NEW-9 minor: Approvals, Approve, Allow and Reviews are four words for two ideas

- **Area:** ui/src/approvals, ui/src/inbox, ui/src/shell (header)
- **Steps:** 1. Run a split task. 2. Answer the "Approvals" panel with "Allow". 3. Open "Reviews"; its first line reads "waiting for you to approve or reject", and its button is "Approve".
- **Expected:** one word for "may the agent do this step" and another for "accept the finished change". For example, the panel is "Requests" (Allow / Deny) and the inbox is "Reviews" (Accept changes / Reject).
- **Actual:** the side panel heading "Approvals" holds Allow buttons, while the review inbox uses Approve.
- **Why it matters for Nadia:** after answering "Approvals" she believes she has already approved the result, and does not open Reviews.
- **Suggested fix:** rename the side panel heading to "Requests" or "Claude asks". In the inbox, name the button "Accept changes" (or "Copy into my folder", which matches its explanation).

### NEW-10 polish: The first screen flashes "Checking Claude…" with an account link, and the start panel's shortcuts are not VS Code's

- **Area:** ui/src/agents/setup (`ClaudeSetup.svelte`); ui/src/shell (start panel)
- **Steps:** 1. Load the app without a project.
- **Expected:** a quiet first screen; the start panel lists keys she knows.
- **Actual:** for about 200 ms the right column shows "Checking Claude… / Check again / Use another Claude account (work, personal)", then "Claude is ready / Change" (s1 `01-first`). The account link appears again in every not-ready state, under the install steps, where it competes with them. The start panel offers "Find a file Alt+P" (VS Code users try Ctrl+P), and the folder box has no placeholder, only the hint underneath.
- **Why it matters for Nadia:** "Use another Claude account" is the most eye-catching link during the first second, and it is not her next step.
- **Suggested fix:** while checking, show only "Checking Claude…". Hide the account link until Claude is ready (or put it under "Change"). If Ctrl+P is taken, mention it ("Alt+P (Ctrl+P in VS Code)"). Add a placeholder such as `/home/you/my-app` to the folder box.

## What worked well

- The first screen is calm: 65 DOM elements, one focused field ("Open a folder") with an example path, and two header buttons. It took 0.2 s to connected.
- Path mistakes get plain, helpful errors: "Folder not found: /nonexistent/project". For `~/project`: "Use the full path to a folder, for example /home/you/my-app". For a file: "That is not a folder. Choose the folder itself, not a file in it." A trailing slash just works.
- The "No file open" start panel gives real next steps, and "Give Claude a task" puts the cursor in the task field.
- The task form is clear: one Start button, two plain options, and only the chosen option's hint ("Claude plans the work and gives parts to up to 3 agents, each on its own copy of the project. You review their changes before anything is merged.").
- The More menu gives every advanced panel a one-line purpose; Reviews has a tooltip with its key and a "(3 waiting)" count.
- Every Claude setup state says what to do in plain words, including that "Signing in to the Claude desktop app or claude.ai does not sign in the claude command line program". A saved path that leads nowhere offers "Find automatically".
- The trust card leads with a one-line count ("1 broad permission · 1 hook that runs code · 4 narrow permissions") and keeps the lists behind "Show details".
- The footer just says "Connected", with the version in its tooltip. Hint text contrast is 6.5:1 (#9a9aa6 on #16161a).
- The review inbox explains what Approve does ("copies the worker's changes into your project folder") before she clicks it.

## Heuristic scores (Nielsen, 0-4)

| # | Heuristic | Score | Key issue |
|---|-----------|-------|-----------|
| 1 | Visibility of system status | 2 | The end of a task is unclear: "Waiting for you" vs "finished" (NEW-1, NEW-2) |
| 2 | Match with the real world | 2 | `agent_id`, "Tool Bash", "Turn ended: 0 tokens", "Session s1" (NEW-4, NEW-5) |
| 3 | User control and freedom | 2 | No mouse path back to the project list or to a second folder (NEW-3) |
| 4 | Consistency and standards | 2 | Approvals / Allow / Approve / Reviews (NEW-9); toast vs row status |
| 5 | Error prevention | 3 | Approval delay and path checks work, but the delay is invisible (NEW-8) |
| 6 | Recognition rather than recall | 2 | Switching projects only through palette commands she has to remember |
| 7 | Flexibility and efficiency | 3 | Shortcuts shown next to actions; Alt+P differs from VS Code |
| 8 | Aesthetic and minimalist design | 3 | Calm first screen; the trust card above the task form is still noisy |
| 9 | Error recognition and recovery | 3 | Good path and setup messages; reload loses context (NEW-4) |
| 10 | Help and documentation | 2 | Good inline hints, but no hint about where results go in "here" mode |
| **Total** | | **24/40** | **Acceptable** |
