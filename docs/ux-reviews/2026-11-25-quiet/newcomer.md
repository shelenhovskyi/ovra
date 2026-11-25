# Nadia, the newcomer: Quiet style (2026-11-25)

Build: 7311b1e (main), headless (`ui/dist` and `target/debug/ovra-daemon` as built), Playwright Chromium 158.0.8127.36, 1280x800 (also 1024x700 and 800x600).
Scope: the first session in the new "Quiet" style (`docs/specs/quiet-style.md`): start screen, opening a project, the Claude setup and project trust cards, starting a task ("One agent, here" and "Plan and split"), answering approvals, results and the Review inbox, closing a task, the icon rail, resizing, folding and swapping the side panels, the title bar search. Dark and light themes (through `prefers-color-scheme`). Fixtures: `small` (seeded as demo-app, once with a `.claude/settings.json` hook added), `empty` (seeded in one run). The scripted fake agent was used, plus one run with `--claude edit_allowed` for the real setup card.
Reference: the owner's mockup `a-quiet.html` and its renders `a-start.png`, `a-work.png`.

Runs: seven scripts; screenshots are named below by file. Eighteen are saved as `/mnt/project-files/ovra/screenshots/quiet/newcomer-*.png`.

Measurements: visible words and font sizes were counted over every text node on screen. Contrast was computed from the computed colour, effective opacity and the nearest opaque background.

## Summary

Nadia could do the whole first session without help. She opened a project (trailing slash accepted), started a task, allowed two requests, ran a split task, kept a worker's changes in Reviews and closed the task. The new frame is clearly calmer than before: no text below 12 px, 12 px only on key caps and badges, a centred search field, quiet panel controls. In dark mode the workspace with a file open is close to the mockup. The worst problems are in the Agents column, where she spends most of her time. As soon as a task is open its header wraps to "0 of / 3" and "New / task" (NEW-1). Closing a finished split task leaves three orphaned worker rows, flipped from "Done" to "Stopped", one of them without its Review badge (NEW-2). The Review inbox is still the wordiest screen in the app, at 493 visible words (NEW-4). Against the owner's complaint, the frame is fixed, but the content inside the panels (setup and trust cards, approvals, inbox) still reads like before the restyle. The start screen is an empty grey sheet, nothing like the mockup's "Open a project" page (NEW-3). The light theme has too little tone difference and several warning texts under 4.5:1 (NEW-9).

New: 0 blocker, 2 major, 9 minor, 4 polish.

Re-check of earlier rounds: NEW-68 (round 7, no mouse path to the terminal) is **fixed**. The rail has a Terminal button and More lists "Toggle terminal Ctrl+`". NEW-69 (Search tab does not focus the box) is **fixed**: the rail's Search button puts the cursor in "Search in files". NEW-64 is **partly fixed**: before Keep, the inbox now says "lint and tests also changed NOTES.md. Only the version you keep first is copied…". It still opens on the last item (docs) while the list starts with lint.

## Findings

### NEW-1 major: The Agents header wraps to "0 of / 3" and "New / task" as soon as a task is open

- **Area:** ui/src/agents (AgentsPanel.svelte header), ui/src/shell (Shell.svelte: the swap and collapse `.controls` are absolutely placed over the panel's header)
- **Steps:** 1. Open demo-app at 1280x800 (Agents panel at its default width). 2. Type "Fix the typo in the README" and click Start.
- **Expected:** One calm header line as in the mockup: "Agents 2 of 3 ▬▬▬", with New task and the panel controls as icons, shown at full strength on hover.
- **Actual:** "Agents", then "1 of" over "3", the slot bars, "+", then "New" over "task", then the swap and collapse icons. Everything is squeezed into about 300 px and wraps onto two lines (`newcomer-agents-header-wrap-dark.png`, also `15-approval`, `16-edit-approval`, `54-worker-open`). With approvals waiting, the Approvals section sits above it with a fixed height, and the last line of the card ("Deny with message…") is cut in half by the Agents header (`newcomer-approval-dark.png`).
- **Why it matters for Nadia:** This is the panel she watches during every task. A header that breaks into fragments is exactly the "looks primitive" impression the restyle was meant to remove, and "1 of / 3" reads like two separate numbers.
- **Suggested fix:** Make New task an icon-only button (plus icon, tooltip "New task Ctrl+N") or move it to the bottom like the mockup's "+ New agent". Give the panel controls their own reserved space in the header row instead of overlaying it. Use `white-space: nowrap` on the capacity text. Let the Approvals block grow to fit its first card, or scroll inside its own frame, rather than being cut by the next section.

### NEW-2 major: Closing a split task leaves its workers as orphans, switches them to "Stopped", and drops one Review badge

- **Area:** ui/src/agents (AgentsPanel.svelte task list, `close`); the status change may come from crates/core orchestration
- **Steps:** 1. Start "Add notes for the project" with "Plan and split". 2. Allow the three Write NOTES.md requests. 3. Open Reviews and keep docs' changes. Close the inbox. 4. In Agents, click the × on the "Task: Add notes for the project" row.
- **Expected:** Closing the task hides the task and its workers together, or asks first: "Close this task? 2 workers' changes still wait for review; they stay in Reviews." Statuses stay as they were (Done). Every worker that waits for review keeps its Review badge.
- **Actual:** The task row disappears without a question and without Undo. Its three worker rows stay, no longer grouped under anything, and all change from "Done" to "Stopped" (`newcomer-task-closed-dark.png`). After step 3, lint's row lost its Review badge, although the inbox still lists "lint … Waiting for review" and the title bar says "Reviews (2 waiting)" (`newcomer-task-closed-kept-dark.png`, run `o6`). On hover, the row's × covers the Review or No sandbox badge ("No sandb×"). A worker opened from the list shows "Review asked for, 1 changed file" but no Review button, only "Resume".
- **Why it matters for Nadia:** "Close" is how she tidies up after a task. Instead, three new-looking rows appear, and "Stopped" makes her think she killed agents that were still working. A missing badge makes her believe there is nothing left to review.
- **Suggested fix:** Close a task together with its workers (one row group), or keep the workers nested and labelled with their task. Do not rewrite a Done status to Stopped when closing. Derive the Review badge from the same source as the inbox count. Put the × in its own column so it never covers a badge. Add a Review button to a worker's detail view when it has an open review.

### NEW-3 minor: The start screen is an empty sheet that says nothing about what Ovra does

- **Area:** ui/src/shell (Shell.svelte start layout, Rail.svelte), ui/src/tree (project list)
- **Steps:** 1. Start with no projects and look at the first screen (dark and light).
- **Expected:** As in the owner's mockup (`a-start.png`): a heading "Open a project" with one line under it, recent projects as rows with an initial and a short `~/` path, a clear "Open folder" action, and the Claude status next to it. The rail is hidden or shows only what works.
- **Actual:** A 14 px "Projects" label, a full-width text field and "Full path, like /home/you/my-app". Below it, about 600 px of empty panel (`newcomer-start-dark.png`, `newcomer-start-light.png`). The 52 px rail on the left is an empty column. The Agents panel has its swap and collapse buttons and one sentence, "Open a project to start an agent.". The title bar search says "Open a project Alt+P", which is a second way to do the same thing with no hint of how it differs. With recent projects the list shows raw absolute paths in mono, such as `/tmp/ovra-ux-fhdzho/demo-app` (`newcomer-start-recent-dark.png`). With 28 words and one font size, it is the screen that looks most "primitive" next to the mockup.
- **Why it matters for Nadia:** Her goal includes "understand what Ovra can do, without help". The first screen tells her to paste a path and nothing else. She cannot tell that agents, approvals and reviews exist until she has opened a folder.
- **Suggested fix:** Build the mockup's start page: a heading ("Open a project"), recent projects with initials, a `~`-shortened path and "2h ago", and the path field as the second option. Show the Claude ready/sign-in card on the start page (the Agents column is empty anyway). Add one sentence on what happens next ("Give Claude a task; you approve each risky step and review the changes."). Hide the empty rail and the panel controls until a project is open.

### NEW-4 minor: The Review inbox is the wordiest screen: 493 words, explanations always open, titles that wrap

- **Area:** ui/src/inbox (Inbox.svelte, describe.ts)
- **Steps:** 1. Finish a "Plan and split" task with three workers. 2. Click Reviews (3 waiting).
- **Expected:** Following the spec's "few words": item titles of one to three words ("docs", with the task as a group header), one line on what Keep does, and the long rules behind "Details". It opens on the first item.
- **Actual:** 493 visible words and 65 text elements at 13 px. The list titles are "lint (task: Add notes for the project)", and each wraps to two lines in a 250 px column with "Waiting for review" and "1 file, 1 also in another open review" under it. Two always-visible paragraphs (59 words) explain Keep and the shared-file rule. The file badge says "Also changed in another open review" again, and after docs is kept the other two still say "1 also in another open review". It opens on docs, the last item. The diff has an empty gutter before the line number (`newcomer-reviews-dark.png`, `newcomer-reviews-light.png`).
- **Why it matters for Nadia:** Reviews is where she "approves results", the core promise. A wall of 13 px grey text is what the owner meant by "small text nobody wants to read". She will click Keep without reading it, which defeats the warning.
- **Suggested fix:** Group items by task (task name once as a header, worker name as the row title). Replace the two paragraphs with one line ("Keep copies NOTES.md into your project.") and a "Details" disclosure. Show the shared-file rule only on the file it concerns, once. Recount the "also in another open review" counts after a Keep. Open the first waiting item.

### NEW-5 minor: The trust card outshines the task box; "No Claude settings in this project" followed by "Trust project" is a non sequitur

- **Area:** ui/src/agents/setup (ProjectTrust.svelte), ui/src/agents (AgentsPanel.svelte composer)
- **Steps:** 1. Open demo-app (no `.claude` folder). 2. Look at the Agents column. 3. Add `.claude/settings.json` with a hook and click "Show details".
- **Expected:** One primary action per area (spec), and that is Start. For a project without its own settings, trust only matters for language servers, so it is one quiet line, not a card with a filled button. The details text fits and is spelled correctly.
- **Actual:** Two accent buttons compete. "Trust project" is filled and bright, and the empty-prompt "Start" is a dim teal that looks broken rather than "type first" (2.84:1 dark, 2.24:1 light; `newcomer-trust-card-dark.png`). The no-settings variant reads "No Claude settings in this project [Trust project] Trusting it starts language servers, which run the project's code in the sandbox." Nadia cannot tell why she would trust a project that has nothing to trust, or whether she must do it before starting. With a hook, the card has 60 words. "Show details" adds another 50 words, with a missing space ("(build scripts, for example)in the sandbox"), and pushes the hook list under the task box, where it is cut off (`newcomer-trust-details-dark.png`).
- **Why it matters for Nadia:** On her very first screen the brightest thing is a security decision she does not understand. A newcomer clicks the bright button.
- **Suggested fix:** Without project settings, show one quiet line: "Language servers: off · Turn on". Keep the filled Trust button only when the project brings hooks or commands, and keep the hook warning visible as the spec requires. Make the empty-prompt Start look like a normal disabled button (outline or `--s3`) rather than a faded primary. Fix the missing space. Let the card scroll on its own, not under the composer.

### NEW-6 minor: "Change" on a working Claude setup opens "How to fix", a path field and an account editor at once

- **Area:** ui/src/agents/setup (ClaudeSetup.svelte, Accounts.svelte)
- **Steps:** 1. With Claude ready ("Claude 2.1.331 is ready" in the `--claude` run), click Change. 2. Click "Use another Claude account".
- **Expected:** A small menu or popover: the account in use, where usage is billed, "Use another account…", "Set path to claude…".
- **Actual:** The card expands in place. A filled "Check again" button is the primary action, though nothing is wrong. Below it come a "How to fix" disclosure, a "Path to claude" field whose placeholder is cut ("Empty: search PATH and the usual install fol…"), Cancel and Save path, and then the accounts editor with Default, "Claude Code's own folder (~/.claude)", Rename, a Name field filled with "Work" and more fields below. All of it is in the 310 px column above the task box (`newcomer-claude-change-light.png`, `newcomer-claude-change-dark.png`).
- **Why it matters for Nadia:** She clicked Change out of curiosity and got a troubleshooting form. "How to fix" makes her think something is broken.
- **Suggested fix:** When ready, rename the disclosure to "Details" and drop the filled Check again. Put the path field and accounts behind their own small links. Show the billing line ("uses your Claude plan") on the one-line ready card, as the spec requires billing to stay visible.

### NEW-7 minor: Parallel workers' approvals repeat the same warning three times and stack three toasts over the editor

- **Area:** ui/src/approvals (Approvals.svelte), ui/src/notify (Notifications.svelte)
- **Steps:** 1. Start "Add notes for the project" with "Plan and split".
- **Expected:** Three compact cards: who asks ("docs") and what ("Write NOTES.md"). The sandbox fact is stated once for the group, still visible as the spec requires, and one toast says "3 requests need approval".
- **Actual:** 367 visible words. Each card repeats "docs · Add notes for the project · demo-app" (wrapping to two lines), "No sandbox: it can reach anything you can." in a yellow box, and "In the agent's folder: NOTES.md". The second and third cards have both "Show details" and "Details". Three toasts stack in the middle of the editor, each with Review and × (`22-split-approval`). Cards that are not scrolled into view show a dimmed Allow (3.21:1 dark, 2.41:1 light) that looks disabled for no visible reason.
- **Why it matters for Nadia:** "Plan and split" is the feature that most needs a quick read. Repetition trains her to skip the yellow box, which is the one security fact she must see.
- **Suggested fix:** Group the cards by task: the task name and the no-sandbox warning once as the group header, the worker name and action on each card. Merge the toasts into one with a count. Keep one disclosure per card. Explain the dimmed Allow in its tooltip ("Scroll to read this request first").

### NEW-8 minor: After a swap the rail is far from the panel it drives, and a folded panel repeats the rail's icon next to it

- **Area:** ui/src/shell (Rail.svelte, Shell.svelte strips)
- **Steps:** 1. Click "Move Files to the other side". 2. Click Files in the rail. 3. Swap back and click Search in the rail twice, so the second click folds the panel.
- **Expected:** The rail stays next to the panel it controls (or moves with it). A folded panel next to the rail is simply gone, and the rail button shows that it can unfold it.
- **Actual:** After the swap, Files, Search and Git stay on the far left, while their panel is on the far right, about 1000 px away. The highlighted Files button sits next to Agents (`newcomer-swapped-light.png`). A folded panel becomes a 48 px strip with the same Search icon, right next to the rail's Search icon: two columns of icons (`newcomer-folded-strip-dark.png`). The start screen shows swap and collapse buttons for a layout that has nothing to swap.
- **Why it matters for Nadia:** She clicks icons on the left and something changes on the right. Two identical icons side by side look like a bug.
- **Suggested fix:** When the rail's panel is folded and sits next to the rail, fold it to nothing (the rail is the strip). When it is swapped to the right, either move the rail with it or give the strip a label. Hide the panel controls on the start screen.

### NEW-9 minor: Light theme: surfaces are almost the same tone, and warnings, status words and dimmed items fall below 4.5:1

- **Area:** ui/src/shell (theme.css tokens), ui/src/tree (FileTree.svelte uses `--text-3` for text)
- **Steps:** 1. Use the app with the system in light mode. 2. Compare the rail, panels and editor; read warnings and statuses.
- **Expected:** Areas told apart by tone (spec), and 4.5:1 for all text.
- **Actual:** `--s0`/`--s1` is 1.046:1 and `--s1`/`--s2` is 1.055:1 (dark is the same, 1.044 and 1.054, but dark panels read better). In light the rail, the side panels and the editor merge into one pale sheet (`newcomer-swapped-light.png`, `newcomer-start-light.png`). `--warn` (#a96d0c) is 3.97:1 on `--s1` and 4.19:1 on `--s2`, used at 13-14 px for "Waiting for review", "(1 waiting)", "Needs approval" and "No sandbox: the agent can read and change…". `--accent` as text is 4.15:1 on `--s1` (the toast's "Review" link measured 3.43:1). Ignored entries in the tree (node_modules, target, build.log) and the ▸ twisties use `--text-3` for 14 px text: 3.66:1 dark, 2.83:1 light, although the theme comment says `--text-3` is for icons and large text only.
- **Why it matters for Nadia:** "No sandbox" and "Waiting for review" are the words she must not miss. The pale light theme also makes the frame look unfinished.
- **Suggested fix:** Darken light `--warn` to about #8a5800 and light `--accent` for text to about #08739a (or add `--accent-text` and `--warn-text`). Widen the light surface steps (for example `--s0` #e9edf1, `--s2` #ffffff). Use `--text-2` plus italics for ignored tree entries.

### NEW-10 minor: A finished task still offers Stop, and the transcript does not show what changed

- **Area:** ui/src/agents (AgentsPanel.svelte bar, Transcript.svelte)
- **Steps:** 1. Start "Fix the typo in the README" (one agent, here). 2. Allow both requests and wait for "Done".
- **Expected:** "Done" offers Close (and Reply). The result is one click away: "Edited README.md · Show change".
- **Actual:** The bar shows "Stop" and "Close" next to a task whose status is "Done" (`17-after-allow`). The transcript lists "Command Run ls -la / Tool output (1 line)" and "File change Edit README.md / Tool output (1 line)" with the output folded, and offers no way to open the changed file or its diff. In this mode there is no Review, so "see the result" means finding README.md in the tree herself.
- **Why it matters for Nadia:** "Agents do the work, you approve" ends with her not knowing what was done. "Stop" on a finished task makes her wonder whether it is still running.
- **Suggested fix:** Hide Stop when the session is idle after a finished turn. Make a File change entry a link that opens the file's diff (the git diff view already exists), and add a one-line "Changed: README.md" summary under Done.

### NEW-11 minor: The file tree still looks like the old UI: text triangles, letter badges, no file icons, header icons that crowd the title

- **Area:** ui/src/tree (FileTree.svelte), ui/src/shell (icons.ts)
- **Steps:** 1. Open demo-app. 2. Drag the Files panel to its minimum, 180 px.
- **Expected:** As in `a-work.png`: drawn chevrons, file-type icons, a small coloured dot for changes, and the tree's buttons muted until hover.
- **Actual:** Folders use the text glyph "▸" at 12 px (3.66:1). Files have no icon. Changes are a 12 px "•" or a green "U". The header has five 26 px icons (Collapse all, Reveal, Changed only, Move, Collapse). At 180 px they touch the word "Files" ("Files ^ ◎ ≡ ⇄ ▯", `newcomer-panel-180-dark.png`). The Collapse all, Reveal and Changed only icons are not muted like the panel controls.
- **Why it matters for Nadia:** The tree is the part she uses most. Next to the new rail icons, text glyphs make the app look patched rather than redesigned.
- **Suggested fix:** Draw the chevron and a few file-type icons in `icons.ts` (folder, file, code, markdown, config). Use a dot for changes with the letter in the tooltip. Mute all header icons until hover, and hide Reveal and Changed only below about 220 px.

### NEW-12 polish: The rail lacks the Reviews and settings entries the spec and mockup put at its bottom

- **Area:** ui/src/shell (Rail.svelte)
- **Steps:** Open a project and look at the rail.
- **Actual:** Files, Search and Git at the top, and Terminal alone at the bottom. Reviews is a text button in the title bar ("Reviews (3 waiting)") with no badge on the rail. There is no settings entry: settings-like pages (policies, network, servers) are in More. On the start screen the rail is an empty column.
- **Suggested fix:** Add Reviews with a count badge at the bottom of the rail, as the spec says, and either keep the title bar button or drop it. Add a settings entry when one exists. Hide the rail when no project is open.

### NEW-13 polish: The title bar search is "Go to file Alt+P", not the mockup's "Search or run"; Ctrl+K does nothing

- **Area:** ui/src/shell (TitleBar.svelte), ui/src/keys
- **Steps:** 1. Click the search field. 2. Press Ctrl+K (shown in the mockup, and named in the spec as "Mod+K opens the palette").
- **Actual:** The field opens the file finder only (`newcomer-search-palette-dark.png`, which works well: "readme" finds README.md at the top with highlights). Commands need Ctrl+Shift+P, which the field never mentions. Ctrl+K does nothing in the browser tab. The label switches to "Open a project" when no project is open.
- **Suggested fix:** Either follow the spec (one field for files and commands, a `>` prefix for commands, with a placeholder like "Search files or type > for commands") or update the spec to say Mod+P. For Nadia, Ctrl+P (VS Code) is the better default in the desktop app.

### NEW-14 polish: Path errors repeat the hint and do not accept `~`

- **Area:** ui/src/tree (project list)
- **Steps:** Type `~/nope/` and click Open.
- **Actual:** The red message "Use the full path to a folder, for example /home/you/my-app." sits under the grey hint "Full path, like /home/you/my-app", saying the same thing twice (`02-wrong-path`). A trailing slash on a real path is accepted, which is good.
- **Suggested fix:** Expand `~` (the core knows the home folder), and when the folder is missing say "No folder at /home/you/nope". Hide the hint while an error shows.

### NEW-15 polish: The app does not ship the mockup's font

- **Area:** ui/src/shell (theme.css)
- **Steps:** Compare `a-work.png` with any screenshot.
- **Actual:** `--font` starts with "Geist", but no `@font-face` or font file is in `ui/`, so every machine falls back to its system font. Weights and widths differ from the mockup, and macOS, Windows and Linux will each look different.
- **Suggested fix:** Bundle Geist and Geist Mono (SIL OFL; this needs the owner's OK under the licence rules, since OFL is not on the permissive list) or settle on the system font in the spec.

## Owner's complaint: "too much small text, looks primitive"

**What is fixed.** No text is under 12 px, and 12 px is used only for key caps, badges and the tree's twisties (counted on every screen). Body text is 14 px and secondary text 13 px. There are no hairlines, the panel controls are muted, the search field is centred, and the rail has drawn icons. With a file open in dark mode, the workspace (`30-file`) is close to `a-work.png` in rhythm and tone.

**Where it is still wordy** (visible words on screen, the rest of the window included):

| Screen                           | Words | Mockup equivalent      |
| -------------------------------- | ----- | ---------------------- |
| Start, no projects               | 28    | about 40, but laid out |
| Workspace, project open, no task | 156   | about 120 incl. code   |
| One approval waiting             | 195   | —                      |
| Three worker approvals           | 367   | —                      |
| Review inbox                     | 493   | —                      |

The inbox (NEW-4), the split approvals (NEW-7) and the trust and setup cards (NEW-5, NEW-6) carry most of the words. They are the parts the restyle did not touch.

**Where it is cramped:** the Agents header (NEW-1), the approval card cut by the Agents section, task rows where "No sandbox" wraps to its own line under "Review" (`newcomer-task-closed-dark.png`), and the tree header at 180 px (NEW-11).

**Where it is low-contrast:** light-theme warnings and accent text, `--text-3` used for tree text, and the dim Start and Send buttons (NEW-9, NEW-5).

**Against the mockup:** the mockup's Agents list is one row per task with a status line and a single pill ("Approve", "Review"). In the app each row has a dot, a truncated name ("Task: Add notes for th…"), a status and one or two pills, and the pills wrap. The mockup's start page has a heading, recent projects and a Claude card, while the app shows an empty panel. The mockup's tree has icons, while the app uses glyphs. The mockup has "+ New agent" at the bottom of the Agents column, while the app squeezes it into the header.

## Scores (1-10)

| Dimension                         | Dark  | Light   | Note                                                           |
| --------------------------------- | ----- | ------- | -------------------------------------------------------------- |
| Visibility of system status       | 7     | 6       | Good toasts and counts; closed-task statuses wrong (NEW-2)     |
| Match with her words              | 6     | 6       | "Trust project" without settings, "How to fix" when ready      |
| User control and freedom          | 6     | 6       | Close without question or undo; swap and fold work and persist |
| Consistency                       | 6     | 6       | Rail far from its panel after swap; two primaries per area     |
| Error prevention                  | 7     | 7       | Armed Allow with "Ready in a moment — read the request first." |
| Recognition rather than recall    | 7     | 7       | Rail tooltips, shortcuts shown; search field files only        |
| Flexibility and efficiency        | 7     | 7       | Resize, fold, swap, keyboard all fine                          |
| Minimalism (owner's complaint)    | 5     | 5       | Frame quiet; inbox, approvals, cards wordy                     |
| Error recovery                    | 6     | 6       | Path error repeats hint, no `~`                                |
| Help for a first-timer            | 4     | 4       | Start screen explains nothing (NEW-3)                          |
| Visual quality against the mockup | 6     | 4       | Dark close in the editor area; light flat, Agents cramped      |
| **Overall**                       | **6** | **5.5** | Calmer and readable; Agents column and inbox still "primitive" |

## What worked well

- Opening a project is fast (161 ms from Open to the tree), and a trailing slash is accepted. The project switcher lists recent projects with their paths, plus "Open a folder…" and "Close project", which is the clear way back that earlier rounds asked for (`51-switcher-two`).
- The empty editor offers four next steps with shortcuts: Give Claude a task, Find a file, Search in files, Open a terminal.
- Approvals are safe and explained: an early click shows "Ready in a moment — read the request first.", the edit card shows old and new text side by side ("teh" → "the"), and "No sandbox" is on every card and session.
- The rail works as Nadia expects from VS Code: tooltips on hover, a second click folds the panel, and Search puts the cursor in the box. The resize handle shows a col-resize cursor, stops at 180 and 460 px, and resets on double click. Folding Agents leaves a small strip, and the layout survives a reload.
- The window never scrolls at 1024x700 or 800x600. At 800 px the panels stack sensibly (`47-800`).
- The palette's fuzzy file search is clear and readable, with matched letters highlighted.
- In dark mode, with a file open, the workspace has the calm, tone-based look of the mockup.
