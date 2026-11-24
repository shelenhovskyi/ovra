# Nadia, the newcomer: Round 1 (2026-11-24)

Build: dd5ffce (main) plus the UX harness, headless, Playwright's bundled Chromium (Linux), 1280x800 (one check at 800x600).
Scope: shell layout and status bar, start screen / project list, virtualized file tree, command palette, global shortcuts. Fixtures: `small`, `empty` (stand-in core); one run with `--real-core`.

## Summary

With the stand-in core Nadia can add her project by typing its full path and browse the tree with the mouse. On the real core she fails at the first step: the error says "the project store is not implemented yet (task 1.B)" and gives her nothing else to do. Even when the stand-in core lets her through, she cannot read a file, cannot get back to the project list, and the palette offers only "Show command palette". It feels unfinished rather than in progress, because no screen says what is coming or what she can do now.

## Findings

### NEW-1 blocker: On today's real core, adding a project shows a raw "task 1.B" error, and nothing else on the screen helps her

- **Area:** ui/src/tree (ProjectList), crates/store (message text)
- **Steps:** 1. Start with `--real-core`. 2. Type the full path of an existing folder into the field and click Add. 3. Click Add again. 4. Type `/nonexistent` and press Enter.
- **Expected:** a plain message such as "Projects can't be saved in this build yet". The field should be disabled or say that adding isn't ready yet, so she doesn't keep trying.
- **Actual:** red text `unavailable: the project store is not implemented yet (task 1.B)` appears above "No projects yet." (`31-real-add.png`). The field stays enabled and keeps the path. Clicking Add again changes nothing on screen, because the same text is simply set again. A path that does not exist gets the same error, so she can't tell whether her path was wrong.
- **Why it matters for Nadia:** this is the first thing she does in Ovra. Task numbers and error codes scare her, and the message suggests the app is broken, not that it is still being built.
- **Suggested fix:** in ProjectList, map the `unavailable` code to a friendly sentence and hide the form, or show a "coming soon" note, while the store is unavailable. Keep "task 1.B" out of user-facing text in `crates/store` (it can stay in logs).

### NEW-2 major: Every error shows its raw protocol code ("not_found:", "bad_request:")

- **Area:** ui/src/tree (ProjectList); the source is `CoreError.message` in ui/src/lib/transport.ts
- **Steps:** 1. On the start screen enter `/no/such/folder`, then `~/project`, then the path of `README.md`, then `small`.
- **Expected:** "Folder not found: /no/such/folder". For `~/project` and `small`: "Use the full path, e.g. /home/you/project", or the path gets expanded. For a file: "That's a file. Choose the folder that contains it."
- **Actual:** `not_found: no such folder: ~/project` (`04-try.png`) and `bad_request: …/README.md is not a folder` (`05-try.png`). The text keeps `snake_case` codes in front, and there is no hint about how to fix the path. (The exact wording after the code comes from the stand-in core, but the `code:` prefix and the missing hint come from the UI.)
- **Why it matters for Nadia:** she reads messages literally. "bad_request" sounds like she broke something. `~/project` and relative names are what she types from VS Code habits.
- **Suggested fix:** show `error.message` without the code, or a per-code sentence, and add a one-line hint under the field ("Full path to a folder, e.g. /home/you/my-app"). Expanding `~` belongs in the core's path handling, not in the UI.

### NEW-3 major: Once a project is open there is no way back to the project list or to add another project

- **Area:** ui/src/shell (header) / ui/src/tree (Tree)
- **Steps:** 1. Open `demo-app` from the list. 2. Look for a way to switch project or add one: the header, a click on the project name, a right-click, the palette.
- **Expected:** the project name in the header opens a switcher, or there is a "Close project" / "Open project…" entry in the palette.
- **Actual:** the header has no buttons and its project name can't be clicked (0 buttons). The palette has no project command. The only way back is to reload the page, which also drops the open project and file (`16-after-reload.png`: back on the start screen).
- **Why it matters for Nadia:** she expects "an obvious first step and a way back". Adding the wrong folder (for example the parent folder) leaves her stuck.
- **Suggested fix:** make the header project name a button that returns to the start screen (or opens a list of projects), and register a `projects.switch` / "Open project…" command in the palette.

### NEW-4 major: Clicking a file shows only the word "Viewer" and the path, with no "not ready yet" hint

- **Area:** ui/src/viewers (placeholder), ui/src/editor (placeholder)
- **Steps:** 1. Open `demo-app`. 2. Click `README.md`, double-click it, click `src/main.rs`.
- **Expected:** if viewers aren't ready yet, the main area should say so in plain words, for example "File preview is coming soon. README.md is selected." Before any file is chosen, it should show a hint, for example "Select a file in the tree to view it".
- **Actual:** grey "Viewer" plus a grey path (`11-readme-clicked.png`, `12-expanded.png`). Before she picks a file, the main area of the start screen also says just "Viewer" (`01-first-screen.png`). Double-click does the same thing.
- **Why it matters for Nadia:** her goal is to "find a file, read it". The bare word "Viewer" looks like a bug or a missing render, not a planned feature. She will click again and again and assume Ovra is broken. (The viewer itself being unimplemented is not the finding; the missing explanation is.)
- **Suggested fix:** have the placeholder say what it is and what is coming. Give the main area a real empty state (no project: "Open a project on the left"; project but no file: "Pick a file to view it"). The empty state stays useful after 1.E lands.

### NEW-5 minor: The palette contains only "Show command palette"; Ctrl+P and F1 do nothing

- **Area:** ui/src/palette, ui/src/keys
- **Steps:** 1. With a project open, press Ctrl+P, F1, Ctrl+O, then Ctrl+Shift+P. 2. Type "open", "file", "project", "help".
- **Expected:** Ctrl+P (VS Code "Go to file") or F1 do something, or at least open the palette. The palette lists a few useful commands.
- **Actual:** only Ctrl+Shift+P reacts. The palette lists a single entry, "Show command palette" (`14-palette.png`), and every query returns "No matching commands". Running that entry closes the palette and opens it again.
- **Why it matters for Nadia:** the palette is where she would go to "understand what Ovra can do". Right now it tells her Ovra can do nothing.
- **Suggested fix:** register the commands that already exist as actions: "Open project…" / "Switch project" (see NEW-3), "Reveal file tree", "Toggle view/edit" when it lands. Hide `palette.show` from its own list. Also bind F1 to `palette.show`, as VS Code does. Quick-open on Ctrl+P is planned; until then, leave it unbound rather than surprising her.

### NEW-6 minor: An empty repository never shows "Empty project"; it shows a lone `.git` folder

- **Area:** ui/src/tree (FileTree), crates/fs-index (lists `.git` as an ignored entry)
- **Steps:** 1. Open the `empty` fixture, a fresh `git init`.
- **Expected:** "This folder is empty", or at least no `.git` row, as in VS Code, which hides `.git` by default.
- **Actual:** the tree has one dimmed row, `▸ .git` (`21-empty-project.png`). The "Empty project" message in FileTree can never appear for a git repository. In `small`, `.git` is the first row of every project (`07-try.png`).
- **Why it matters for Nadia:** `.git` is noise she never edits, and opening it by accident shows git internals.
- **Suggested fix:** hide `.git` in the tree by default (in FileTree, or with a flag in the listing), and count only visible rows when deciding to show the empty message.

### NEW-7 minor: The project list shows only folder names, with no path or "last opened" info

- **Area:** ui/src/tree (ProjectList)
- **Steps:** 1. Start with `small` and `empty` seeded (`20-start-two.png`).
- **Expected:** each entry shows its path, shortened (`~/code/demo-app`), as VS Code's "Recent" does, so two folders called `app` can be told apart.
- **Actual:** only `empty` and `demo-app`. The path appears only in a hover tooltip (`title`). There is no label like "Recent", no remove action, and no explanation of what "Add" does compared with clicking a name.
- **Why it matters for Nadia:** she expects "recent projects" with enough context to pick the right one.
- **Suggested fix:** show the path in a second, muted line. Label the field "Open a folder" and the button "Open" rather than "Add"; to her, "project" means a folder she opens.

### NEW-8 minor: No feedback while adding, and old errors stay visible

- **Area:** ui/src/tree (ProjectList)
- **Steps:** 1. Enter a bad path, then submit again (`03-try.png` to `06-try.png`).
- **Expected:** the button shows a busy state while the call runs. The old error clears when she edits the field or submits again.
- **Actual:** the error is never cleared. A retry that fails in the same way looks as if nothing happened. The button is never disabled. On the first screen nothing has focus, so she must click the field first.
- **Why it matters for Nadia:** she can't tell whether her second try was even sent.
- **Suggested fix:** clear `error` on input and on submit, disable the button while the call is pending, and autofocus the field when the list is empty.

### NEW-9 polish: Hint and placeholder text is low-contrast

- **Area:** ui/src/shell, ui/src/tree, ui/src/viewers
- **Steps:** measure `#6f6f7a` (used for "No projects yet.", the "Viewer" placeholder, the tree's empty/loading note) on `#16161a`.
- **Expected:** at least 4.5:1 for body text.
- **Actual:** 3.64:1. The `#9a9aa6` secondary text is fine at 6.48:1.
- **Why it matters for Nadia:** the very messages that should guide her are the hardest to read.
- **Suggested fix:** use `#9a9aa6` (or similar) for guidance text, and keep the darker grey for decoration only.

### NEW-10 polish: Git badges and truncated names explain themselves only partly

- **Area:** ui/src/tree (FileTree)
- **Steps:** 1. Expand `src` and the long folder (`12-expanded.png`). 2. Hover over the rows.
- **Expected:** a tooltip with the full name on truncated rows. Badges readable without already knowing git letter codes.
- **Actual:** badges have tooltips (`modified`, `untracked`), which is good. Folder dots (`•`) say only "modified". Truncated rows such as `very-long-directory-name-that-will…` have no tooltip (0 rows carry a `title`), so she cannot read the full name anywhere, because the sidebar can't be resized.
- **Suggested fix:** put `title={row.path}` on rows and give folder dots the tooltip "contains changes".

## What worked well

- The first screen is calm and focused: one heading, one field, one button. The status bar shows "Connected to ovra 0.1.0 (headless)" right away.
- Adding a valid folder opens it straight away into the tree, so she doesn't have to click again.
- The tree is fast and clear: folders first, ignored files dimmed, git colours like VS Code, and expand/collapse with a single click.
- The palette looks familiar (centered box, shortcut on the right) and closes on Escape or on a click outside, as she expects.
