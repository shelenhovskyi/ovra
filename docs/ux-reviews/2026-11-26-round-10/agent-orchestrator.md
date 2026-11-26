# Agent orchestrator: round 10

Build: 7990e2b, headless, Chromium 158.0.8127.36, 1280x800.
Scope: Markdown in the transcript, Jump to latest and the compact task header
(321c8b0, 9fe9770); adding files by drop and paste, with the check for
secret-looking files (c26b952, 7841dbb, 1ea2933); the Focus switch in the
title bar (1c893e0); the Get started checklist (88ad493); the Git panel
redesign with pull and push (9c21d13, ee5d613) and workers' push approvals
(9cbf38d, 56a8467); file icons and coloured git status (ffe1266, ad63ec8);
the simpler project trust card (d645447); panel borders (d544fc3).
Fixtures: `small` (demo-app), `monorepo`, both seeded. Most runs used the
scripted fake agent. Markdown and the sandbox runs used `--claude` with a copy of
`edit_allowed.jsonl` whose final answer was changed to rich Markdown.

## Summary

Andrii can run a "Plan and split" task with three workers next to a queued
direct task, approve everything from Focus and review the results, so the
day's features hold up when several agents run at once. Two
problems cost him the most. First, the transcript stops following the agent as soon
as an approval card opens above it, so he never sees the latest line without
clicking. Second, the git panel lists the sandbox's empty placeholder files
(`CLAUDE.md`, `AGENTS.md`, `.envrc`, ...) as changes and offers to commit
them. Otherwise the panel is calm and readable: the Markdown looks good, the
secret-file check works, and Focus fits several workers well. Most of the
clutter comes from one 320 px column holding approvals, the header, Get
started, the trust card and the transcript at once.

## Findings

### ORCH-1 major: the transcript loses its place every time an approval card opens

- **Area:** ui/src/agents (Transcript.svelte, sticky scroll and Jump to latest)
- **Steps:** 1. Open demo-app, start "Fix the typo in README" (One agent,
  here). 2. Allow each request as it comes. 3. Send seven short replies,
  allowing the `git status` request after each one. Do not touch the scroll
  wheel.
- **Expected:** The transcript stays pinned to the newest message while the person
  has not scrolled up, and sending a reply always brings the view to the
  newest message.
- **Actual:** When the Approvals card opens, the transcript's height drops from
  432 px to 170 px, but its scroll position stays the same. The view is then
  no longer at the bottom, so it stops following. The distance from the
  bottom grows each round: 292, 541, 791 ... 1790 px (from `session.log` of
  `jump2`). After seven rounds the view shows round 2 and the Jump to latest
  button says "18 new". Sending a reply while scrolled up does not move the
  view either: the label goes from "18 new" to "21 new"
  (`52-new-while-up.png`). The new count only appears in `aria-label` and
  `title`. On screen there is just a 28 px chevron with a 6 px dot.
- **Why it matters for Andrii:** he answers approvals all day, so this
  happens on every turn. Each time he must find and click the small chevron
  to see what the agent just did.
- **Suggested fix:** Track "at bottom" as a state that only the person's own
  scroll changes. When the transcript is resized (ResizeObserver) and that
  state is set, scroll back to the bottom. Always scroll to the bottom after
  the person sends a message. Show the count as text on the button
  ("3 new ↓") rather than only in the tooltip.

### ORCH-2 major: the git panel lists the sandbox's empty placeholder files as changes and offers to commit them

- **Area:** ui/src/git (GitPanel, FileSection) and ui/src/tree, with
  crates/sandbox-linux (`FILES_MADE` in `src/plan.rs`) as the source
- **Steps:** 1. Run the UX harness with `--claude` (real driver, sandboxed
  session) on demo-app. 2. Start a task and allow its Edit. 3. While the
  session is still open, open the Git panel.
- **Expected:** The git panel and the tree show only the changes the person or
  the agent made. Files the sandbox makes for itself are hidden or clearly
  marked as temporary.
- **Actual:** `git status` and the panel show `.envrc`, `.mcp.json`,
  `.worktreeinclude`, `AGENTS.md`, `CLAUDE.local.md` and `CLAUDE.md` as
  untracked. The header says "8 changed files", and the primary button reads
  "Commit 8 files" (`80-git-during-session.png`). The tree also gains about
  ten empty folders (`.codex`, `.cursor`, `.idea`, `.vscode`, `.zed`, ...)
  and green "U" badges. Everything disappears when the session ends.
  `plan.rs` documents this ("Empty files show in git status"), but nothing in
  the UI explains it.
- **Why it matters for Andrii:** git status is his main signal. With several
  sessions open the placeholders are always there. One click on "Commit N
  files" puts an empty `CLAUDE.md` and `.envrc` into history.
- **Suggested fix:** Have the core report the placeholders it created (for
  example a `sandbox_placeholder` flag on status entries, or a list in
  `agents.status`). Leave them out of the commit count and the file list,
  with a footnote such as "6 empty files the sandbox holds open are hidden".
  Mark them in the tree the same way. A narrower option: exclude them through
  `.git/info/exclude` while the sandbox runs.

### ORCH-3 major: with the disk estimate in the Agents header, New task and Keep awake are pushed off the screen

- **Area:** ui/src/agents (AgentsPanel.svelte header, 9fe9770)
- **Steps:** 1. Open monorepo and start "Plan and split". 2. Look at the Agents
  header, then try the "+" (New task) button.
- **Expected:** The header fits in the 320 px column. The capacity, the disk
  estimate and the end buttons all stay visible.
- **Actual:** The row's `scrollWidth` is 399 px against a `clientWidth` of
  320 px. "up to 183 MB on di…" is cut off. Keep awake and New task sit at
  x = 1330 in a 1280 px window, so Playwright reports "element is outside of
  the viewport" and the click fails (`11-split-started.png`, `13-mixed.png`).
  Alt+N still works.
- **Why it matters for Andrii:** the moment he runs workers is exactly when he
  wants to start the next task and keep the machine awake. Both buttons
  disappear at that moment.
- **Suggested fix:** Move the disk estimate out of the header, either into the
  capacity tooltip or the task's own row (each worker already shows "up to
  61 MB on disk"), or let it shrink with `min-width: 0` and an ellipsis. Keep
  `.end` from being pushed out with `flex: none` plus `min-width: 0` on the
  middle span. Add a test that the New task button is in the viewport with
  the disk label showing at 320 px.

### ORCH-4 major: a second direct task fails after the person has typed it, and the error does not say what to do

- **Area:** ui/src/agents (task form, `agent-start-error`)
- **Steps:** 1. In demo-app, start "Fix the typo in README" (One agent,
  here). 2. Press New task and type "Bump the version", still "One agent,
  here". 3. Press Start.
- **Expected:** Before Start, the form says that one agent already works in
  the main checkout. It offers "Plan and split" (its own copy) or "Queue
  after the current one", the same way the capacity limit already queues
  ("Waiting, 1st in line").
- **Actual:** The form shows red text "Could not start: another agent already
  works in this project's main checkout". The text stays in the box and the
  form offers no way forward (`04-four.png`). I tried four tasks in a row:
  only the first started.
- **Why it matters for Andrii:** running agents in parallel is his whole
  workflow. The obvious first try, several "One agent, here" tasks, fails
  with an error worded for the core rather than for him.
- **Suggested fix:** When a direct session is live, disable "One agent, here"
  or label it "(busy: Fix the typo...)", and switch the default to "Plan and
  split". If the error still happens, add a button that starts the same text
  as Plan and split, or queue the task behind the running one.

### ORCH-5 minor: Cancel on the secret-file question also drops the safe files from the same paste

- **Area:** ui/src/agents (ContextInput.svelte `copyChecked`, attach.ts)
- **Steps:** 1. In the task box, paste two files at once: `.env` and
  `screenshot.png`. 2. The box asks: ".env may hold secrets. A copy in the
  project can be read by every agent. Copy anyway / Cancel". 3. Press Cancel.
- **Expected:** Cancel skips `.env` and still adds `screenshot.png`, or the
  question names both files and says that Cancel skips both.
- **Actual:** Neither file is added. Only `.env` was named, so the loss of
  `screenshot.png` goes unnoticed (log: chips before and after Cancel are
  both just `crash.log`).
- **Why it matters for Andrii:** he pastes several logs and screenshots at once
  for an agent. Losing the safe ones without notice means the agent works
  without the context he believed he gave it.
- **Suggested fix:** Split the paste. Copy the files that do not look like
  secrets right away and ask only about the rest. Label the buttons "Copy
  .env anyway" and "Skip .env".

### ORCH-6 minor: Focus counts reviews still waiting as "done"

- **Area:** ui/src/focus (FocusView summary line)
- **Steps:** 1. In monorepo, start "Plan and split" and a direct task.
  2. Switch to Focus and allow the three workers' writes.
- **Expected:** The summary reads something like "1 working · 3 to review".
- **Actual:** "1 working · 4 done", while three "Ready to review" cards sit
  directly below it (`15-focus-after-allow.png`). Under "Done" the
  coordinator task and all three workers say "Done", although none of their
  changes was kept.
- **Why it matters for Andrii:** the count is how he decides whether to look
  at all. "Done" suggests there is nothing left to do.
- **Suggested fix:** Count "Ready to review" separately from finished work.
  Don't count the coordinator task, which only produced workers. In the Done
  list, show workers with an open review as "Waiting for review".

### ORCH-7 minor: Get started step 3 repeats the approval card and stays in the way during a multi-agent run

- **Area:** ui/src/agents (FirstSteps.svelte, 88ad493)
- **Steps:** 1. Fresh data folder, open demo-app. 2. Start a task, or a
  Plan and split task with three workers.
- **Expected:** Once a request is visible in the Approvals card, the checklist
  does not repeat it. In a busy run it folds down to one line.
- **Actual:** Step 3 says "Claude asks you something. Open" while the same
  request is already open in the card above. With three workers the text is
  still singular. The card takes about 150 px of the column, between the
  task list and the trust card, until it is hidden (`04-four.png`,
  `61b-task-list.png`).
- **Why it matters for Andrii:** in a 320 px column every line of repeated
  text pushes the workers' rows and the transcript out of sight.
- **Suggested fix:** When the request is already on screen, show "Answer the
  request above" with no button. Use the plural when several agents ask.
  Collapse the checklist to its one-line header once a task has started.

### ORCH-8 minor: the git panel shows file status only as a coloured dot

- **Area:** ui/src/git (FileSection.svelte, 9c21d13 / ad63ec8)
- **Steps:** 1. Open demo-app's Git panel. 2. From outside, as an agent
  would: delete `Cargo.toml`, change `README.md`, add `NEW_FROM_AGENT.md`,
  rename `changes.patch`.
- **Expected:** Each row says what happened in a way that can be scanned, as the
  tree does with "M" and "U" letters next to the colour.
- **Actual:** The panel caught up in 83 ms, which is good. But each row has
  only a 6 px dot: red for deleted, amber for modified, green for untracked.
  The text ", deleted" exists only for screen readers. The rename shows up as
  one deleted and one untracked file. A long name like `NEW_FROM_AGENT.md`
  runs under its dot (`31-git-changes.png`).
- **Why it matters for Andrii:** telling a deleted file from a modified one by
  a red or amber pixel is slow and unreliable at a glance, and the tree a few
  centimetres away uses letters.
- **Suggested fix:** Use the tree's letter badge (M, U, D, R) in the same
  colour, and keep the name's ellipsis from reaching the badge column.

### ORCH-9 minor: the transcript is only 170 px tall while a request waits

- **Area:** ui/src/agents and ui/src/approvals (column layout)
- **Steps:** 1. At 1280x800, start a task in demo-app. 2. Wait for the first
  request.
- **Expected:** Enough of the transcript stays visible to judge the request
  (what the agent said just before).
- **Actual:** From top to bottom the column holds the Approvals card (about
  250 px), the Agents header, task title and status, "No sandbox: the agent
  can change anything you can" (two lines), then a transcript 170 px tall,
  then the reply box. That leaves two or three lines of what the agent said
  (`03-after-Fix.png`, `16-ide-after.png`). The reply box also says "Claude
  is working..." while the status says "Needs approval".
- **Why it matters for Andrii:** he judges requests by their context, and
  with this layout the context is hidden behind the request.
- **Suggested fix:** Show the no-sandbox note once, in the header's status
  line, not as a two-line banner on every task. Shrink the approval card's
  empty gaps. When a request waits, use the reply placeholder "Answer the
  request above, or reply".

### ORCH-10 minor: the project trust card disappears when a task opens

- **Area:** ui/src/agents/setup (ProjectTrust.svelte, d645447)
- **Steps:** 1. Give monorepo a `.claude/settings.json` with
  `allow: ["Bash(*)", "Edit"]` and a hook. 2. Open it and the card says
  "This project has its own Claude settings" with two warnings. 3. Start
  "Plan and split".
- **Expected:** While the decision is open, a short line stays visible near
  the task ("Project settings: not trusted · Review").
- **Actual:** The card is gone while the task is open. Back in the task list
  it sits below three workers and Get started. Opening "Review and trust…"
  expands the details inside the scrolling list, so the card's heading
  scrolls out of view (`62-trust-details.png`). The new wording is clear and
  shorter, which is good.
- **Why it matters for Andrii:** he needs to know whether workers run with
  the project's `Bash(*)` rule. While a task runs, nothing in sight tells
  him.
- **Suggested fix:** Keep one line with the trust state in the task header
  or footer ("Claude · Ready · project settings off"), and have Review open
  the full list at the top of the column.

### ORCH-11 minor (policy): the `.claude` folder and Claude files use the Claude logo as their icon

- **Area:** ui/src/shell/file-icons (`data.ts`), scripts/file_icons.py (ffe1266)
- **Steps:** 1. Open any project that has a `.claude` folder (the trust
  fixture above, or any sandboxed session).
- **Expected:** A plain folder or settings icon. CLAUDE.md in the repository
  says: "No Claude or Anthropic logos."
- **Actual:** The `claude` and `folder-claude` icons are the orange (#e57e5d)
  Claude starburst from Material Icon Theme, shown in the tree, tabs, the
  palette and the git panel (`60-trust-card.png`).
- **Why it matters for Andrii:** it is not a usability problem for him, but
  it breaks a product rule from ADR 0006 and ships in every build.
- **Suggested fix:** Remove `claude` and `folder-claude` from the icon list in
  `scripts/file_icons.py`. Map `.claude`, `CLAUDE.md` and `CLAUDE.local.md`
  to the generic `folder-config` and `markdown` icons, and add a test that no
  icon name contains "claude".

## Not exercised

- Workers' push approvals (9cbf38d, 56a8467): the scripted fake worker never
  calls the push tool, so I could not show the Push card. Reading
  `describe.ts`, the card names the target ("Push to origin/ovra/…"), what it
  builds on, the files and the commit message. Whether these sit behind
  "Details" needs a fake worker step that pushes; please add one to the
  daemon's fake script so the next round can judge it.
- The person's own push and pull: "Send 1 commit" worked against a local
  bare remote ("Sent 1 commit."). Get commits was not offered without
  anything to fetch. A branch with no upstream shows no sync area and no hint;
  publishing is out of v1 (docs/specs/git-panel.md), so this is not a finding.

## What worked well

- Markdown in the transcript: headings, nested lists, inline code, a table,
  a blockquote and links all render cleanly. A link to `README.md` opens the
  file in a tab (`70-md.png`, `72-link.png`).
- The compact task list explains a run in a few lines: "Write the notes ·
  Workers are on it · 0 of 3 done", each worker with its state and disk
  estimate, and "Waiting, 1st in line" for the queued task.
- Focus with four agents: one column, "Waiting on you" at the top, the
  approval card in place, then the review cards with "See changes". Approving
  three workers from there took no navigation.
- The secret-file check catches `.env` and `id_ed25519` from both paste and
  drop, and says why in plain words. Dragging a tree row onto the box adds a
  chip. Pasting into the Reply box works too.
- The Git panel caught external changes in 83 ms, and "1 commit not sent
  yet. They are only on this computer. Send them to origin/main." is the
  clearest explanation of ahead/behind I have seen in an IDE.
- Panel borders: the hairlines between the sidebar, editor and Agents column
  are quiet and make the three areas easy to tell apart.
