# Nadia, the newcomer: round 10

Build: 7990e2b (branch `ux-round`), headless, Chromium 158.0.8127.36, 1280x800 (one run at 1280x560), light and dark.
Scope: the git panel redesign with Get/Send commits, the last Get started step, panel hairlines and the panel-toned dock, Markdown in the transcript, Jump to latest, the compact task header, drop and paste into task boxes, file icons and coloured git status, the Focus toggle, the simplified trust card. Fixtures: `small` (seeded), `monorepo`. Fake agent. Scripts in `/tmp/ux-newcomer/s1.ts` .. `s10.ts`, shots next to them (`/tmp/ux-newcomer/<script>/`).

## Summary

Nadia can open her project, read files, start a task, allow its requests and commit, and most of today's work reads well: the git panel says "Everything is saved" / "2 changed files" / "Commit 2 files" in plain words, agent Markdown renders as real lists, and drop/paste gives a clear chip. The worst problems come right after her first task: a second task is refused with "another agent already works in this project's main checkout" while the panel says 0 of 3 agents and the task says Done, and Get started keeps saying "Claude is working" for a task that finished. The git panel's new "Get commits" line almost never appears because nothing checks the remote, and git status uses three different codes in the tree, the git panel and the palette.

## Findings

### NEW-1 major: a second task is refused because a finished one still holds the folder

- **Area:** crates/core (`src/agents/requests.rs`, `prepare`), ui/src/agents
- **Steps:** 1. Open demo-app. 2. Type "Summarize what's new" and press Start; it ends as Done (no requests). 3. Click "< Tasks", type "Checkpoint please" in Describe a task, press Start.
- **Expected:** The second task starts. If one task at a time is the rule in the main folder, the box says so before she types, in plain words, with the fix ("Close the task 'Summarize…' first").
- **Actual:** Red text under the box: "Could not start: another agent already works in this project's main checkout" (`s4/22-checkpoint.png`). At the same time the header says "Agents 0 of 3" and the only task row says "Done". Nothing working is visible, and "main checkout" is a word she does not know.
- **Why it matters for Nadia:** Her second action in Ovra fails, and the message contradicts the screen. She has no idea that "Close" on the finished task is the way out.
- **Suggested fix:** Do not count a session that is Done (not running, nothing pending) as the folder's writer, or close it automatically when a new direct task starts. If a task really blocks, name it and offer the action: "'Summarize what's new' still has this folder. Close it to start a new task" with a Close button.

### NEW-2 major: Get started says "Claude is working" after the task is done, and never finishes

- **Area:** ui/src/agents (`FirstSteps.svelte`)
- **Steps:** 1. Open demo-app (Get started 1 of 3). 2. Start "Summarize what's new"; the agent answers and the task is Done with no request and no changes to review. 3. Click "< Tasks".
- **Expected:** Step 3 says what to do now, for example "Done. Read Claude's answer" with an Open button, and ticks itself off once she has read it.
- **Actual:** "3 Check Claude's work. Claude is working. Its questions and changes show up here." (`s4/21-list.png`). The task row right above it says Done. The step is only ticked by an answered request or a review, so a question-only task leaves the checklist at 2 of 3 forever.
- **Why it matters for Nadia:** The checklist is the one place that tells her what to do next, and it is wrong at the moment she is most unsure. She waits for work that has already ended.
- **Suggested fix:** Base the third line on the task's state: running → "Claude is working…"; Done with changes → "Review"; Done without changes → "Read the answer" (Open), and count opening a finished task as checking the work.

### NEW-3 major: "Get N commits" never appears on its own; the panel does not look at the remote

- **Area:** ui/src/git (model), crates/fs-index (git watching)
- **Steps:** 1. Give demo-app a remote with one commit that is not local, and one local commit not pushed. 2. Open the Git view. 3. Run `git fetch` in a terminal and wait 2.5 s.
- **Expected:** The panel finds out by itself (on open, on focus, every few minutes, or with a "Check for new commits" item) that the remote has 1 new commit, and shows "Get 1 commit".
- **Actual:** Before the fetch the panel only shows "1 commit not sent yet · Send 1 commit" (`s5/29-git-nofetch.png`), which pushes onto a remote that has moved on. After the fetch the panel still shows ahead 1, behind 0: the log has no `fs.git_changed` and no new `git.status`. Only a page reload shows "1 new commit on origin/main · Get 1 commit" (`s5/30-git-sync.png`).
- **Why it matters for Nadia:** "Get commits" was added because "nothing is clear", but she will only see it if she already knows to fetch in a terminal. Without it she tries Send, which fails on a remote that has moved on.
- **Suggested fix:** Fetch the upstream through the Forge when the Git view opens and on window focus, throttled, and show "Checked 2 min ago" plus a quiet "Check now" in the More menu. Also make the watcher fire `fs.git_changed` when `.git/refs/remotes/**` or `FETCH_HEAD` change.

### NEW-4 minor: git status is shown in three different ways

- **Area:** ui/src/git (`FileSection.svelte`), ui/src/palette, ui/src/tree (shared `shell/file-icons`)
- **Steps:** 1. Expand `src` in Files. 2. Open the Git view. 3. Press Ctrl+K and type `main.rs`.
- **Expected:** One code everywhere: the same colour on the name and the same short word or letter.
- **Actual:** In the tree, `main.rs` is amber (rgb 127,83,0) with an "M" and `new_module.rs` is green with a "U" (`s2/07-tree.png`). In the git panel, both names are plain grey (rgb 83,93,107), and the only sign is a 6 px brown or green dot whose tooltip says "M: modified" / "U: untracked" (`s2/03-git.png`). In the palette, `src/main.rs` has no status at all (s10 log: `PAL src/main.rs=rgb(23, 26, 32)`).
- **Why it matters for Nadia:** She cannot learn one code. A dot without a word or legend tells her nothing, and "untracked" is git jargon.
- **Suggested fix:** Use the tree's coloured name plus letter in the git panel and the palette too. In the git panel, where there is room, write the word ("changed", "new", "deleted") instead of a letter or a dot.

### NEW-5 minor: git words come back in the diff and the sync card

- **Area:** ui/src/git
- **Steps:** 1. In the Git view, click `main.rs`. 2. With a remote ahead, read the sync card.
- **Expected:** The same plain words as the panel: "Changes since your last commit", "1 new commit on GitHub" or "on the shared copy".
- **Actual:** The diff header says "src/main.rs Unstaged changes" with "Lines 1 to 3" above the code (`s2/05-git-diff.png`), but the panel hides staging on purpose. The sync card says "1 new commit on origin/main" (`s5/30-git-sync.png`), and the row tooltips say "untracked".
- **Why it matters for Nadia:** The redesign is plain until she clicks a file. Then "Unstaged" makes her think she missed a step.
- **Suggested fix:** When staging is hidden, label the diff "Changes since last commit" (keep "Staged" / "Not staged" for staging mode). Say "the remote (origin/main)" or the forge's host name instead of the bare ref, and rename "untracked" to "new file".

### NEW-6 minor: in Focus, the "Focus" pill looks like the button she pressed but does nothing

- **Area:** ui/src/focus (`FocusBar.svelte`)
- **Steps:** 1. Click "Focus" in the title bar (top right, a pill). 2. Try to leave Focus by clicking the same pill again.
- **Expected:** The same control in the same place turns Focus off, or at least the way back sits where Focus was.
- **Actual:** In Focus, a same-looking "Focus" pill sits at the top left. It is a `<span>`, and clicking it does nothing (my script's click hit nothing). The way back is "Open IDE" at the top right (`s3/18-focus.png`). "IDE" appears nowhere else in the UI, and the tooltip on the way in, "Only the agents' work", does not say what Focus hides or how to get back.
- **Why it matters for Nadia:** She feels trapped for a moment, and "IDE" is a word she has to translate.
- **Suggested fix:** Put a pressed "Focus" toggle where it was in the title bar (`aria-pressed=true`), so the same click leaves. Rename "Open IDE" to "Back to files" or "Exit Focus". Change the tooltip to "Focus: hide files and code, show only tasks (Ctrl+Shift+.)".

### NEW-7 minor: in a short window, the transcript collapses and Jump to latest covers the reply box

- **Area:** ui/src/agents (`Transcript.svelte`, panel layout)
- **Steps:** 1. Set the window to 1280x560 (a laptop at 125-150% zoom). 2. Start a task with a few lines ("Fix the typo" plus a short list). 3. The first request shows.
- **Expected:** The request, the task header and some of the transcript stay readable, and the reply box is clear.
- **Actual:** The transcript area is about 0 px tall. The "No sandbox" line is cut off, and a line of her own message ("- check the readme") shows through between the header and the reply box. The round Jump to latest chevron sits on top of the "Claude is working…" field (`s8/50-small-start.png`).
- **Why it matters for Nadia:** Text looks broken and overlaps in the area where she is asked to decide.
- **Suggested fix:** Give the transcript a minimum height (about 96 px) and let the approvals card scroll instead. Anchor the jump button inside the transcript's own box, never over the composer, and hide it when the transcript is shorter than the button.

### NEW-8 minor: a paste adds a strange `.ovra-attachments` folder at the top of her project

- **Area:** ui/src/tree (ignored styling), ui/src/agents (`ContextInput.svelte`)
- **Steps:** 1. Click Describe a task. 2. Paste a screenshot (Ctrl+V).
- **Expected:** A chip "image.png", and if a folder must be created, it looks like other ignored folders and she is told once why it exists.
- **Actual:** The chip appears (good), and `.ovra-attachments` appears as the first row of the Files tree in normal (not italic, not dimmed) text, above her own folders. `node_modules` and `target` are italic as ignored (`s6/36-drop-env.png`). Nothing explains the folder. The chip's tooltip is the bare path `.ovra-attachments/image.png`.
- **Why it matters for Nadia:** A new folder in her project that she did not create looks like something went wrong, or like something she has to commit.
- **Suggested fix:** Show `.ovra-attachments` as ignored, like the other ignored entries (it is git-ignored by its own `.gitignore`). The first time a copy is made, add a one-line note under the chip: "Copied into the project so the agent can read it (not committed)."

### NEW-9 minor: "Interrupt", "Stop" and "Close" in the task header are three similar words

- **Area:** ui/src/agents (`AgentsPanel.svelte`, compact header)
- **Steps:** 1. Start a task. 2. Read the header row: "< Tasks … Interrupt Stop". 3. When done, the row reads "< Tasks … Close".
- **Expected:** One obvious "Stop" while it works, and an end-of-task action that says what it does.
- **Actual:** "Interrupt" (tooltip "Stops this step; you can reply") and "Stop" (tooltip "Ends the session") sit next to each other as equal plain text (`s3/13-started.png`). After Done, "Close" could mean the panel, the transcript or the task. As NEW-1 shows, it also releases the folder, which no word suggests.
- **Why it matters for Nadia:** She does not know which one is safe to press. "Interrupt" vs "Stop" is a distinction only an expert makes.
- **Suggested fix:** Show one "Stop" with the safe behaviour (interrupt; she can still reply), and put "End task" in its overflow. After Done, label the button "End task" or "Archive" with a tooltip "Frees this folder for a new task".

### NEW-10 polish: finished steps are struck through, so "Claude is ready" reads as "not ready"

- **Area:** ui/src/agents (`FirstSteps.svelte`)
- **Steps:** Open any project; look at Get started (`s2/02-project.png`).
- **Expected:** Done steps read as done: a check and calm text.
- **Actual:** "~~Claude is ready~~" and "~~Type a task and press Start~~" are crossed out in grey. A crossed-out "Claude is ready" looks like a cancelled or false statement.
- **Suggested fix:** Drop `text-decoration: line-through` and keep the check icon plus `--text-2`.

### NEW-11 polish: icon-only controls whose pictures do not say what they do

- **Area:** ui/src/agents (keep awake), ui/src/git (`AgentActions.svelte`), ui/src/terminal
- **Steps:** Look at the Agents header, the Git header and the terminal dock.
- **Expected:** Icons that are common in other apps, or a word.
- **Actual:** A coffee cup in the Agents header means "Keep the computer awake while agents work". A sparkle in the Git header opens "Suggest commits / Summarize what's new", and nothing says these start an agent that uses her Claude plan, or that "what's new" makes no sense without a remote. A speaker in the terminal dock means "Read terminal output to a screen reader", which she reads as "sound on/off" (`s6/37-terminal.png`).
- **Suggested fix:** Label the sparkle "Ask agent ▾", and end its items with "(starts an agent)", or show the account line in the menu. Hide "Summarize what's new" when there is no upstream. Move the screen-reader toggle into the dock's menu or settings. Keep the coffee cup, but let its tooltip say "Keep awake: off".

### NEW-12 polish: the trust card still opens with jargon and ends in a long paragraph

- **Area:** ui/src/agents/setup (`ProjectTrust.svelte`, `texts.ts`)
- **Steps:** 1. Open a project with `.claude/settings.json` (one `Bash` rule, one hook, one env var). 2. Click "Review and trust…" and scroll to the button.
- **Expected:** Short lines in her words. The Trust button comes right after the lists.
- **Actual:** The lead says the card "starts language servers". Above Trust project there is a 6-line paragraph: "…They run the project's code (build scripts, for example) in the sandbox, which keeps them to the project and their cache, without network…" (`s7/42-trust-btn.png`). Group names like "Hooks: commands that run on agent events" and raw entries like `PostToolUse (Edit): npx prettier --write .` are fine for security, but "language servers" is not a newcomer word. The amber warnings themselves are clear, and "Claude works without it" is a good new line.
- **Suggested fix:** Say "code help (autocomplete, errors) for this project" instead of "language servers". Cut the bottom paragraph to one line plus a Details link: "Also turns on code help, which runs the project's build scripts in the sandbox."

### NEW-13 polish: a heavy accent frame around the whole Files tree after opening a project

- **Area:** ui/src/tree (focus styling), ui/src/shell (focus on open)
- **Steps:** Click a recent project with the mouse.
- **Expected:** A calm first screen; the focus ring shows on one row when she uses the keyboard.
- **Actual:** A 2 px accent outline goes around the entire Files panel (`s2/02-project.png`, `s7/40-trust.png`). It is the loudest thing on screen, and it vanishes after her first click. Also, a 28 px empty footer runs along the bottom of every screen, including the start screen, whose only content ("Connected") is not visible.
- **Suggested fix:** When focus moves to the tree because a project opened, put it on the first row (or use `:focus-visible` on the row, not the tree container). Hide the footer until it has something to show, such as language or line endings for an open file.

## What worked well

- The git panel in its normal state is plain: branch, "2 changed files", "Commit message: What did you change? For example: Fix the login button", "Commit 2 files". Staging and amend are tucked into "…" (`s2/03-git.png`, `s2/04-git-more.png`). "1 commit not sent yet · They are only on this computer" is exactly the right sentence.
- When the remote is ahead and she has local commits, the card says "Get the new commits first, then send yours" and hides Send. That prevents a confusing rejected push.
- Agent Markdown renders as a real bulleted list, and her own message stays literal (`s4/20-markdown.png`).
- Drop and paste: "Drop to add to the message" while dragging, a clean `image.png` chip after paste, and a dropped `.env` is stopped with "may hold secrets… Copy anyway / Cancel" (`s6/35-dragover.png`, `s6/36-drop-env.png`).
- File icons make the tree, tabs and palette easier to scan, and ignored entries are italic. The hairlines and the panel-toned terminal dock separate areas calmly in both themes without heavy borders (`s6/37-terminal.png`, `s6/38-terminal-dark.png`).
- The trusted state folds to one tidy line: "Project's own settings are on ⚠2 Details" (`s7/43-trusted.png`).
- The Focus view is calm and readable: "Waiting on you / Nothing waits for you / Done 1", with one task box at the bottom.
