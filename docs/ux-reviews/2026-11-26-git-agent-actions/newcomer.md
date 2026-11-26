# Nadia, the newcomer: git agent actions

Build: 8a385a3, headless (scripted fake agent), Playwright Chromium, 1280x800.
Scope: the git panel's agent actions (`docs/specs/git-agent-actions.md`): the
sparkles button "Agent actions" on the branch row, Suggest commits (edit,
Leave out, Commit all, Dismiss, changed since suggested), Write commit
message (with Undo), Summarize what's new, the error line, and the palette
commands. Fixtures: `small`, changed with real git inside the scripts
(README.md edited, `new.txt` added, a `--no-ff` merge for `ORIG_HEAD`).

## Summary

Nadia found the sparkles button by hovering and got useful results: a plan
she could edit and commit, a drafted message she could undo, and a short
"What's new" card. Each action takes one click. Three problems stand out.
First, Commit all on a plan marked "Your changes moved on" commits right away,
with no second question, so it commits file contents she never saw in the
plan. Second, every action leaves a "Stopped" agent in the Agents column and
ticks "Type a task and press Start" in Get started, so the column fills with
things she did not start and does not need. Third, the error line wraps one
word per line next to its buttons. The panel already has a commit box, so the
plan card adds a second commit button and the sidebar gets crowded. That
matches the owner's worry that the UI is overloaded.

## Findings

### NEW-1 major: Commit all on a "changed since suggested" plan commits without asking

- **Area:** ui/src/git (`AssistCard.svelte`, `assist.svelte.ts` `commitAll`)
- **Steps:** 1. Change README.md and add new.txt. 2. Sparkles > Suggest
  commits. 3. Edit README.md again and touch `src/lib.rs` outside Ovra.
  4. The card shows "Your changes moved on since this was suggested."
  (`newcomer-stale-plan.png`). 5. Press Commit all.
- **Expected:** what the spec says: the card asks again before it commits,
  for example "Your files changed after the agent looked at them. Commit
  anyway? / Suggest again".
- **Actual:** both commits are made at once ("Made 2 commits."). The README
  commit contains the newer text that neither Nadia nor the agent saw.
- **Why it matters for Nadia:** she reads messages literally. A yellow
  sentence with no action attached looks like a note, not a warning. She
  presses the blue button and gets a commit she did not review.
- **Suggested fix:** when `assist.stale` is true, the first press of Commit
  all should only change the button to "Commit anyway" and show "Suggest
  again" next to it. Or replace Commit all with those two buttons outright.
  Add a component test for this.

### NEW-2 major: every action leaves a "Stopped" agent behind and ticks a Get started step

- **Area:** ui/src/git (session lifecycle), ui/src/shell (Agents column, Get
  started)
- **Steps:** 1. Use Suggest commits three times, then Write commit message
  once and Summarize what's new once. 2. Look at the Agents column.
- **Expected:** a quick helper inside the git panel stays inside the git
  panel. At most, the running action shows while it works.
- **Actual:** five rows such as "Suggest commits for 4 chang... / Stopped"
  pile up above Get started. "Stopped" reads like a failure even when the
  action worked. After the first git action, Get started also ticks "Type a
  task and press Start", although Nadia never typed a task. If she opens one
  of these rows, she sees the raw prompt with diffs, the JSON answer,
  "No sandbox: Claude can change anything you can" right above "Research
  changes no files.", and "Save as doc" / "Turn into task" buttons that make
  no sense for a commit suggestion.
- **Why it matters for Nadia:** the Agents column is where she learns what
  Ovra does. Here it fills with half-finished-looking chores that she did not
  start as tasks, and the onboarding tells her she did something she did not
  do. This is the main thing that makes the feature feel heavy.
- **Suggested fix:** keep these sessions out of the main Agents list, or put
  them in one collapsed "Git helper (5)" group. Show their state as "Done",
  not "Stopped", once the panel has its answer. Do not count them toward
  Get started step 2. The spec only needs the transcript and billing to be
  reachable, so a "Show in Agents" link on the card is enough.

### NEW-3 major: the error line wraps one word per line

- **Area:** ui/src/git (`AssistCard.svelte`, `.line`)
- **Steps:** 1. Make sure the changed files are not README.md or new.txt (the
  `small` fixture: `src/main.rs`, `src/new_module.rs`). 2. Sparkles >
  Suggest commits.
- **Expected:** a short red sentence across the panel's width, with Try
  again and Dismiss below it.
- **Actual:** the text "The agent's answer could not be used. The answer
  names a file that has no changes: README.md" takes about 9 lines of 1 to 3
  words in a narrow column, because the buttons share its row
  (`newcomer-error-line.png`). Try again usually repeats the same answer, and
  the message does not say what Nadia can do.
- **Why it matters for Nadia:** a broken-looking block of red text reads as
  "something is wrong with my project". She also cannot tell whether README.md
  is her mistake.
- **Suggested fix:** wrap `.line.error` so the buttons go under the text, as
  the plan card already does. Reword it to "The agent suggested a file you
  didn't change (README.md). Try again, or commit by hand below." When the
  action is not available, the panel already says "Explain conflicts does not
  apply now.", so use the same plain style here.

### NEW-4 minor: two commit areas at once, and the plan card does not read as commits

- **Area:** ui/src/git (`AssistCard.svelte`, `GitPanel.svelte`)
- **Steps:** 1. Get a 2-commit plan (`newcomer-plan-card.png`).
- **Expected:** one obvious place to commit. Each suggested commit reads as
  a unit: its message (the first line bold, the rest folded, as the spec
  says), its files, and a way to drop it.
- **Actual:** the card's Commit all sits right above the normal "Commit
  message" box and its own Commit button. The two-commit card is about 270 px
  tall, which pushes the file list half off screen. Messages are monospace
  textareas with no bold subject. "Leave out" is indented under the file
  name, so it reads as "leave out this file", not "drop this commit". When
  Nadia wrote a two-line message, the one-row-high box later showed only the
  body ("The old intro was too long.") and hid the subject. "2 files stay
  uncommitted" does not say which files.
- **Why it matters for Nadia:** she has to work out which button commits
  what. "Leave out" sounds like it removes the file from git.
- **Suggested fix:** hide the commit box while a plan is shown (it comes back
  on Dismiss or after committing). Render each commit as bold subject + files
  - a small "Remove" (×) on the subject row. Open the editor only on click,
    sized to its content, starting at the top. Name the files that stay
    uncommitted in the tooltip, or as a list when there are three or fewer.

### NEW-5 minor: "Summarize what's new" shows on a repository with no pull or merge

- **Area:** ui/src/git (`Assist.available`)
- **Steps:** 1. Open a fresh clone or the `small` fixture, which has a single
  commit and no `ORIG_HEAD`. 2. Open the sparkles menu.
- **Expected:** per the spec, the item shows only when `ORIG_HEAD` exists.
- **Actual:** the item is always there when HEAD exists. Pressing it shows,
  in the error line, "There are no new commits since the last pull, merge or
  rebase." with a "Try again" that cannot help.
- **Why it matters for Nadia:** she tries every menu item. One that always
  answers "nothing to do" in red teaches her the menu is unreliable, and it
  adds an extra item to a menu that should be short.
- **Suggested fix:** hide the item unless `git.pulled` (or the status)
  reports an `ORIG_HEAD`. If it has to stay, show the answer as neutral text
  without Try again.

### NEW-6 minor: Leave out on the last commit silently throws the whole plan away

- **Area:** ui/src/git (`assist.svelte.ts` `drop`)
- **Steps:** 1. Get a 2-commit plan. 2. Leave out commit 2, then commit 1.
- **Expected:** the card stays, saying "No commits left" with Dismiss, or
  Leave out can be undone.
- **Actual:** the card disappears with no message. The only way back is to
  run the agent again.
- **Suggested fix:** keep an Undo for the last Leave out, or keep an empty
  card that says "No commits left." with Dismiss.

### NEW-7 polish: the palette lists actions that do not apply

- **Area:** ui/src/keys / ui/src/git (command registration)
- **Steps:** Ctrl+Shift+P, type "git:" in a repository with no conflicts.
- **Actual:** 16 "Git:" commands, including "Git: explain conflicts". Running
  it gives no visible feedback. The notice "Explain conflicts does not apply
  now." goes only to the hidden live region, as far as Nadia can see.
- **Suggested fix:** register the four commands only while
  `Assist.available` includes them, or show the notice visibly.

## What worked well

- The menu is short and shows only what applies, mostly (Write commit
  message appears only with staged files). Every item has a one-sentence
  tooltip in plain words.
- Results appear in place within 0.3 s with the fake agent, and focus goes to
  the card.
- Write commit message over her own text: "An agent replaced your message.
  Undo" brought back "my own words" exactly.
- An edited multi-line message was committed exactly as typed, subject and
  body. A left-out commit's file stayed as a change. Nothing was pushed or
  staged behind her back.
- Wording is mostly free of jargon ("Suggesting commits…", "What's new",
  "Made 2 commits.").
