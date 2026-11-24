# Nadia, the newcomer: Round 4 (2026-11-24)

Build: 81d16b1 (main), headless, real core, Playwright's bundled Chromium 158.0.8127.36 (Linux), 1280x800.
Scope: features new since round 3: Claude onboarding (setup card, path, sign-in), "Trust project", the Agents panel (first task), Approvals, terminals, the review inbox (Alt+Shift+R in the browser), direct mode (review, approve, reject). Fixtures: `small`, `empty`, plus throwaway git folders the scripts create (`nadia-*` with a README that has a typo, `teamproj-*` with `.claude/` settings).

Runs: the scripted fake agent (default daemon), the real Claude driver against `fake-claude` (`edit_allowed`, `denied_outside_folder`, `interrupted`, `subagent`), and one daemon started without `--fake-agent` with `PATH=/usr/bin:/bin` and an empty `HOME`, so that no `claude` can be found. Screenshots and logs are in `/tmp/claude-0/ux4-newcomer/<run>/` and are named here as `<run>/<file>`. New IDs continue after NEW-29 from round 3.

Harness limit: `fake-claude` replays the recorded messages but does not write files. To get a diff in the inbox, the scripts wrote the agent's change themselves right after Allow. Those writes land after the tool's snapshot interval has closed, so the core correctly counts them as "changed outside the agent". So I could not check how agent edits are attributed. Findings NEW-31 and NEW-38 only rely on how the UI reports what the core returned.

## Summary

Nadia can get her first task going. The setup card says clearly whether Claude is installed. The task box is easy to find. Approval cards show the command or the old and new text, and she cannot click them by accident. The worst problem is the core itself: it crashes with a stack overflow as soon as a second direct session in the same project ends (NEW-30). In the browser that means "Connection to core lost" and every request hangs. Ending a second session is something she does within her first ten minutes. After that, the things that mislead her most are the review inbox's "Rejected." when nothing was restored (NEW-31) and a transcript squeezed into 74 px (NEW-32), so she cannot see what the agent did. Overall the agent flow feels promising but fragile. The words are mostly plain, but they assume she already knows ".claude/", "hooks", "main checkout" and where the terminal is.

New: 1 blocker, 2 major, 7 minor, 1 polish.

## Findings

### NEW-30 blocker: The core crashes (stack overflow) when a second direct session in the same project ends

- **Area:** crates/core (src/orchestration, direct-mode review and attribution between sessions)
- **Steps:** 1. Default daemon (scripted fake agent), seed `small`, open demo-app. 2. Task "Fix the typos in the README", Start, then Stop. 3. Task "Second task", Start. 4. Stop it, or let its turn end.
- **Expected:** The second session ends, it gets its own review item (or none), and the app keeps working.
- **Actual:** The core emits `review.changed` for `r2`. The next `review.list`, `review.diff` and `agents.list` requests (ids 33 to 37) never get an answer. The footer keeps saying "Connected to ovra 0.1.0 (headless)" for several seconds. Then the daemon prints `thread 'tokio-rt-worker' has overflowed its stack` / `fatal runtime error: stack overflow, aborting`, and the UI shows "Connection to core lost, reconnecting…". A file she clicks then reads "Not connected to Ovra's core, so Cargo.toml cannot be read right now." (`11/02-later.png`, `10b/02-later.png`). It also happens without any file change (run 11). It happens with the real Claude driver too, where the second session failed at once (run `07b`, same stack overflow).
- **Why it matters for Nadia:** Starting a second task is the most natural next step after the first one. Everything stops working, the open review and the attribution live only in memory (per the spec), and round 3's NEW-23 means the page may never reconnect on its own. She would conclude that Ovra is broken.
- **Suggested fix:** Find the recursion when a later direct session's review is computed against an earlier one's ("to an earlier session a later one's changes are changed outside it"). Add a core test: two direct sessions in sequence in one project, the second one ends, and `review.list` answers.

### NEW-31 major: Reject says "Rejected." and restores nothing when the files count as changed outside the agent; then the list is empty

- **Area:** ui/src/inbox (with crates/core/src/orchestration for the outcome data)
- **Steps:** 1. `--claude edit_allowed`, open a git folder whose README says "teh". 2. Start the recorded task, allow the Edit (the script writes the fix), reply with the recorded Bash task, allow it (the script writes `done.txt`), Stop. 3. Click "Reviews (1 waiting)". The item says "2 files, 2 changed outside the agent". 4. Click "Reject…". The confirmation reads "Reject restores the agent's files from the start of the session. Changes made outside the agent stay as they are and are listed for you." 5. Click "Reject the changes".
- **Expected:** Before she confirms: how many files will be restored and which ones stay. After: the files that stayed are still listed, as the confirmation promised.
- **Actual:** The outcome is just "Rejected.". The item now shows "0 files" and "This review has no files." (`03/09-rejected.png`). But `README.md` still has the change and `done.txt` still exists (`git status`: ` M README.md`, `?? done.txt`). The spec says that after a core restart every file counts as changed outside. So after a restart, or after NEW-30, Reject always restores nothing and still just says "Rejected.".
- **Why it matters for Nadia:** She reads "Rejected." as "the agent's work is gone". Then she finds the changes still in her files and has no list of what is left. This is the "can she reject by mistake, or think she rejected" case from the brief.
- **Suggested fix:** In the confirmation, name the files: "Restores: none. Stays as it is: README.md, done.txt (changed outside the agent)". When nothing would be restored, say so and offer only "Close the review" instead of a reject button. After a reject, keep the files that stayed in the item with their marker. The outcome line should say "Rejected. 0 files restored; 2 files kept: …".

### NEW-32 major: The transcript gets 74 px of the right column, so she cannot see what the agent did

- **Area:** ui/src/agents (layout with ui/src/shell)
- **Steps:** 1. 1280x800, `--claude interrupted`, start the recorded task, allow, then Interrupt. 2. Look at the session detail.
- **Expected:** The conversation and tool calls fill most of the panel. Setup and trust stay out of the way once handled.
- **Actual:** From the top, the column stacks Approvals, the "Claude 2.1.331 is ready" card, the "Project settings: not loaded" card (4 lines), the task box, the session list, the session header, two status lines and the reply box. The transcript box measures 339x74 px, with a scrollHeight of 564 px (`13/02-interrupted.png`, `02/failure.png`). Only "Error / Interrupted." is visible. In `02/failure.png` the visible part starts in the middle of a clipped line.
- **Why it matters for Nadia:** "Did the agent do what I asked?" is the question the panel must answer. She has to scroll a tiny box to find out, and she probably does not notice that it scrolls.
- **Suggested fix:** Collapse the setup card to one line ("Claude 2.1.331 ready ▸") when it is ready. Collapse the trust card when nothing would load. Let the transcript take the remaining height (flex: 1), or open the session in the main area. Hide the new-task form while a session is selected, behind a "New task" button.

### NEW-33 minor: With Claude not installed she can still press Start; the session fails, its title disappears, and "Resume" is offered

- **Area:** ui/src/agents
- **Steps:** 1. Daemon without `--fake-agent`, `PATH=/usr/bin:/bin`, empty `HOME`. 2. The setup card correctly says "Claude is not installed" and lists the install commands. 3. Open demo-app, type "Explain this project", Start.
- **Expected:** Start is disabled (or explains itself) until Claude is found: "Install Claude first (see above)".
- **Actual:** The session is created and fails with "Failed: claude was not found. Install Claude Code and sign in once by running `claude` in a terminal, or choose its path in the agent setup." The session row shows only that red text, so the task title is pushed out of view. A red toast repeats it. Below it, "Resume with a message / Continues this session where it ended." is offered for a session that never started (`04/03-start-anyway.png`). The runtime's message itself is good (crates/agent-runtime `detect.rs`, `Problem::NotFound(None)`).
- **Why it matters for Nadia:** She did the obvious thing and got a failure, a toast and a "Resume" that cannot work. The setup card above already had the answer.
- **Suggested fix:** Disable Start unless setup is `ready`, with the hint pointing to the card. In the session list, show the title plus a short state ("Failed"), with the full message only in the detail. Do not offer Resume for sessions that have no Claude session id.

### NEW-34 minor: Path mistakes in "Path to claude" give a lowercase core error, a raw `--version` dump, or a stale "Using …"

- **Area:** ui/src/agents/setup (wording from crates/agent-runtime `detect.rs` and the `agents.set_path` check in crates/core)
- **Steps:** In the not-installed run, type each of these into "Path to claude" and click Save path: `~/.local/bin/claude`, `/usr/bin/`, `/usr/bin/ls`, `claude`.
- **Expected:** Plain guidance for each mistake: expand `~` or say "Use the full path, for example /home/you/.local/bin/claude". "That is a folder; choose the claude file in it." "That file is not Claude Code."
- **Actual:**
  - `~/.local/bin/claude` gives "the path must be absolute" (lowercase) under the unchanged heading "Claude is not installed".
  - `/usr/bin/` gives "Nothing was found at /usr/bin/." There is no hint that it is a folder.
  - `/usr/bin/ls` gives the heading "Claude could not be checked" and the body `could not read the version of claude from "ls (GNU coreutils) 9.4\nCopyright (C) 2023 Free Software Foundation, Inc.\nLicense GPLv3+ …\n\nWritten by Richard M. Stallman and David MacKenzie."`, nine lines with literal `\n` (`04/04-path-_usr_bin_ls.png`).
  - `claude` after that gives "the path must be absolute" below the old "Using /usr/bin/ls" and the GNU text.
- **Why it matters for Nadia:** She types paths the way she would in a terminal, and she reads every message literally. A license text from `ls` looks like something broke.
- **Suggested fix:** Expand `~`, or reuse the folder field's wording from round 3 ("Use the full path…"). Detect a folder. For an unreadable version, say "This does not look like Claude Code (it printed something else for --version)" and hide the raw output behind "Details". Clear the old result when a save fails.

### NEW-35 minor: A second task while one runs fails with "another agent already works in this project's main checkout"

- **Area:** ui/src/agents (message from crates/core)
- **Steps:** 1. Fake agent, demo-app, start "Fix the typos in the README". 2. While its approval waits, type "Also add a license file" and press Start.
- **Expected:** Start is disabled while a direct session is active, with a plain reason and a way forward: "Claude is already working on 'Fix the typos in the README'. Stop it or wait until it is done."
- **Actual:** Start is enabled. Pressing it shows the red core text "another agent already works in this project's main checkout" (lowercase, with "main checkout" jargon). Her text stays in the box, with no hint of what to do (`06/01-second-task.png`).
- **Why it matters for Nadia:** "Main checkout" means nothing to her, and the message does not say which agent or what to do next.
- **Suggested fix:** Disable Start while a direct session runs in the project and explain why next to the button. Rephrase the core message for the UI, and name the session.

### NEW-36 minor: There is no visible way to open a terminal, but the setup card tells her to run commands "in a terminal"

- **Area:** ui/src/terminal, ui/src/shell
- **Steps:** 1. Look for a terminal with the mouse: header, footer, tree toolbar, Agents panel. 2. The signed-out and not-installed cards say "run claude in a terminal" or "Run this in a terminal".
- **Expected:** A visible "Terminal" button. Ideally an "Open a terminal" link next to the command in the setup card.
- **Actual:** No button for terminals exists (`getByRole('button', {name: /terminal/i})` count 0). The panel opens only with Ctrl+`or from the palette. Once open, it works well: the shell starts in the project folder, and`echo hello-nadia && pwd`prints`/tmp/…/demo-app` (`06/05-terminal-typed.png`).
- **Why it matters for Nadia:** She uses the mouse first and does not know the Ctrl+` shortcut. Onboarding sends her to a terminal the app hides.
- **Suggested fix:** Add a "Terminal" toggle to the header or footer with its shortcut in the tooltip. In ClaudeSetup, add "Open a terminal" next to `claude auth login` and the install commands (it could prefill the command; she still presses Enter).

### NEW-37 minor: The trust card does not help her decide, and the risky button is the highlighted one

- **Area:** ui/src/agents/setup
- **Steps:** 1. Open any project. The card reads "Project settings: not loaded. Claude reads the project's CLAUDE.md but not its .claude/ settings, hooks, agents, commands or skills." 2. Click "Review and trust…". 3. In `teamproj` (hook, env, allow rules, subagent) the list is shown with "Some of it runs commands on your computer." 4. After a simulated pull that changed the hook to `curl … | sh` and added `Bash(rm -rf:*)`, the card shows a good +/− diff.
- **Expected:** One sentence on what trust means for her and when to give it: "Only trust projects whose authors you trust. You can start tasks without trusting; Claude then ignores the project's own Claude settings." The safe choice should not look like the default.
- **Actual:** "not loaded" in bold reads like an error she has to fix. The words are jargon to her (".claude/", "hooks", "skills", "PostToolUse (Edit)"). Nothing says that she can work without trusting. "Trust project" and "Trust the new settings" are the blue-outlined primary buttons, next to a plain "Close" or "Stop trusting" (`12/01-trust-review.png`, `12/03-changed.png`). With "Tools allowed without asking you: Bash(curl:*)", she cannot tell that trusting means those commands will no longer appear in Approvals.
- **Why it matters for Nadia:** She will click the highlighted button to make the "not loaded" warning go away, which is the opposite of what a careful person does with a cloned repository.
- **Suggested fix:** Rename the untrusted state to something neutral ("Project's Claude settings: off (safe default)"). Add the one-line rule above. Give both buttons equal weight. For allow rules, add "these will run without asking you".

### NEW-38 minor: When the agent is done, nothing points her to the review, and "waiting" means two things

- **Area:** ui/src/agents, ui/src/notify, ui/src/inbox
- **Steps:** 1. `edit_allowed` run until the first turn ends. 2. Read the toast, the session status and the header.
- **Expected:** "Claude finished. 1 file changed: Review changes" with a button that opens the inbox on this item.
- **Actual:** The toast says "… finished and waits for you" with only ×. The session status says "Waiting for you". The header button turns into "Reviews (1 waiting)" in the top-right corner (`03/02-after-edit.png`). The session detail never mentions the review. After Interrupt, the toast also says "finished and waits for you" (`13/02-interrupted.png`). In the inbox, "Commit the agent's files" leaves the message empty because the item has no summary (`"summary":null`). "Approve and commit" is then disabled with no reason given (`07b/03-commit.png`).
- **Why it matters for Nadia:** "Agents do the work, you approve" is why she installed Ovra, and the approve step is the one the UI does not lead her to.
- **Suggested fix:** Add a "Review changes" action to the toast and to the session detail when the session has an open review item. Say "Interrupted" rather than "finished". Prefill the commit message from the task title when there is no summary. Explain a disabled "Approve and commit" ("Enter a commit message").

### NEW-39 minor: The transcript shows text written for the model, not for her

- **Area:** ui/src/agents (Transcript), crates/agent-runtime
- **Steps:** 1. `--claude interrupted`: allow the ping, then Interrupt. 2. `--claude subagent`: run the recorded task.
- **Expected:** "You interrupted this command." For a subagent, "Started a helper agent: read README.md".
- **Actual:**
  - Interrupt: "Tool failed", followed by a 9-line block that starts "The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing … Correction to the text above: …", then "Error / Interrupted.".
  - Subagent: "Tool Agent / Use Agent" with no description, then a tool result that includes "agentId: … (internal ID - do not mention to user. Use SendMessage …)" and "Do NOT Read or tail this file via the shell tool".
  - Not installed or crashed: the session row shows "claude exited with exit status: 2: …".
- **Why it matters for Nadia:** Error text, shouting capitals and "do not mention to user" read as something going wrong, even when it is the normal flow.
- **Suggested fix:** Give the interrupt result and the `Agent`/`Task` tool their own summaries in `describeTool`. Fold long tool results by default ("Show tool output"). Map exit failures to a short sentence, with details on demand.

### NEW-40 polish: Small wording items on approval cards and in the Agents panel

- **Area:** ui/src/approvals, ui/src/agents
- **Steps / Actual:**
  - Cards name the session by its whole prompt: "Fix the typo in README.md with the Edit tool. Do not run commands and do not cha… asks to use Edit".
  - "Agent's note: README.md" and "Agent's note: /notes.txt" repeat the file line below them (`03/01-edit-approval.png`, `05/01-approval.png`).
  - "Why it asks: Ovra asks before an agent changes files outside its folder. (hook)": "(hook)" means nothing to her, and the bold headline "Write /notes.txt" does not say "outside the project".
  - Edits show "old string / new string" instead of a small diff. Paths are absolute even inside the project.
  - Enter in the task box adds a new line. Only Ctrl+Enter starts, which the hint does say.
- **Expected:** "Claude (task: Fix the typo…) wants to edit README.md". Put "outside the project folder" into the headline. Show a two-line diff. Use paths relative to the project.
- **Why it matters for Nadia:** These are small, but the approval card is the one place where she must read and understand before clicking.
- **Suggested fix:** As above. Drop the note when it equals the file. Map reason kinds to words ("Ovra's safety rule").

## What worked well

- The not-installed card is clear. "Claude is not installed" comes with install commands for every OS, a guide link and a path field, all without her having to start anything (`04/01-start.png`). The ready state names the version and the executable.
- Approvals cannot be clicked by accident. An Allow click about 100 ms after the card appeared did nothing, and the card answered only after the arm delay. Y/N hints are shown, and "Deny with message…" worked: the agent read "Keep files inside the project." and did not retry (run 05).
- The "settings changed since you trusted them" diff is well done. It shows + and − per entry, "runs code" on hooks, and "Some of what was added runs commands on your computer." (`12/03-changed.png`).
- Reject asks for confirmation and explains what will happen (`03/08-reject-confirm.png`). Her own edit made while an approval waited was correctly marked "Changed outside the agent" (run 07b).
- The inbox opens with Alt+Shift+R, shows a readable side-by-side line diff, and Escape returns focus. The header button counts open reviews.
- Stopping a session with a pending approval clears the card and shows "the session was stopped" on each tool. Nothing stays waiting.
- The terminal, once she finds it, opens in the project folder and works immediately.
