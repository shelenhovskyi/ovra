# Agent orchestrator (Andrii): Round 2 (2026-11-24)

Build: f427218 on `main` (485e5f2 plus the patch, binary and too-large fixture files), real core,
headless, Chromium 158.0.8127.36, 1280x800.
Scope: viewers (markdown, patch, text, binary, too large) refreshing on outside changes; editor with
tabs (`ui/src/editor`), conflict detection through `expected_mtime_ms`; projects (switching,
per-project state restore); open-external (`open.external`, task 1.G). Special focus: unsaved
changes. Fixtures: `small`, `monorepo`. Ten scripted runs; agents are simulated by writing,
deleting, renaming and `git checkout`-ing files in the fixture from the script.

## Summary

Reading what agents change works well: markdown, patch and text viewers redraw 72-147 ms after an
outside write, keep the scroll position, and say when the open file was deleted. Editing next to
agents is not safe yet: unsaved text lives only in page memory and is thrown away without a word
when the file changes on disk while its tab is in the background, when the page reloads (the only
way to recover from a lost core connection, ORCH-1), and Undo plus Save, or Save after an agent
deleted the file, overwrites the agent's work without the conflict check. Per-project state comes
back when a project is reopened, but there is still no way to switch projects, and open-external
works in the core but has no control in the UI.

## Findings

### ORCH-10 blocker: unsaved edits are silently discarded when the file changes on disk while its tab is in the background

- **Area:** ui/src/editor (`session.svelte.ts`, `load()` and the `unsaved` stash)
- **Steps:** 1. Open `demo-app`, click `src/main.rs`, press Ctrl+E, type `// NOTE-B unsaved` at the
  end. 2. Click `src/lib.rs` in the tree (the `main.rs` tab keeps its ● dot; its close button
  says "Close main.rs (unsaved changes)"). 3. An agent writes `src/main.rs`. 4. Click the `main.rs`
  tab, press Ctrl+E.
- **Expected:** the editor keeps my text and shows the conflict ("main.rs changed on disk while
  you had unsaved changes": compare / keep mine / take theirs). Nothing I typed disappears without
  my say-so.
- **Actual:** the editor shows the agent's version, the ● is gone, no alert (`dirty=0 alerts=0`),
  and Ctrl+Z cannot bring the text back (`30-B-stash-lost.png`, run r4). Frames, run r4:
  `14:10:52.970 AGENT write src/main.rs`, `14:10:53.018 <- fs.changed ["src/main.rs"]`,
  `14:10:53.531 -> fs.read_file src/main.rs`, `14:10:53.541 <- mtime_ms 1795529452964`; the tab
  label goes from "Close main.rs (unsaved changes)" to "Close main.rs". No frame ever carried the
  unsaved text. Cause: `load()` keeps the stash only if `stash.mtime === file.mtime_ms`, otherwise
  drops it and clears `dirty`. The same happens after closing a dirty tab (ORCH-14) and
  reopening it after an agent touched the file.
- **Why it matters for Andrii:** agents touch the file he is annotating all the time; this is
  exactly when he switches tabs to look at something else. He finds out later, if ever.
- **Suggested fix:** never drop a stash. When the mtime differs, open the editor with the stashed
  text, keep `dirty`, and set `conflict = true` with the disk version kept for comparison (the
  same state Save already reaches). Mark the tab as "changed on disk" too, so he sees it before
  switching back. A unit test in `session.test.ts` with the fake core covers it.

### ORCH-11 blocker: reloading the page (or the core going away) loses every unsaved edit without a warning

- **Area:** ui/src/editor (stash), ui/src/shell (page lifecycle); with ORCH-1 in
  `ui/src/lib/transport.ts`
- **Steps:** 1. Edit `src/main.rs` (unsaved), switch to `src/lib.rs`, edit it too
  (`// NOTE-D before reload`), both tabs show ●. 2. Reload the page (F5, or the only recovery from
  a lost connection, see ORCH-1 below).
- **Expected:** the browser asks "Leave site? Changes you made may not be saved" (`beforeunload`)
  while anything is dirty; better, unsaved text is kept (session storage or the project state)
  and offered back when the project reopens.
- **Actual:** no dialog (the script's `dialog` listener saw none, and nothing in `ui/src`
  registers `beforeunload`). After the reload the start screen shows "No project"
  (`33-D-after-reload.png`); reopening `demo-app` brings back the tabs but both files show the
  disk text; neither note is on disk. Frames after the reload are only `projects.list` and
  `info.get` (run r4, 14:10:56.748). The stash is a module-level `Map` in
  `session.svelte.ts`, so it dies with the page. Run r8: with the daemon killed, Ctrl+S sends
  nothing and shows nothing (`60-daemon-down-save.png`), status still says "Connected"; the
  ● stays, but the only way out is a reload, which then loses the text.
- **Why it matters for Andrii:** the coordinator's report is right: text typed in Ovra is
  silently lost after a reload. For him a reload is routine (core restart, laptop wake, ORCH-1).
- **Suggested fix:** register `beforeunload` while `dirtyFiles` is non-empty (one line in the
  shell or editor). Persist the stash per project in `sessionStorage` keyed by project and path
  (text, saved text, mtime) and restore it in `load()`, going through the ORCH-10 conflict path
  when the mtime differs. Fix ORCH-1 so a reload stops being the recovery path.

### ORCH-12 major: Undo walks back past an outside change, and Save then overwrites the agent's version with no conflict

- **Area:** ui/src/editor (`session.svelte.ts`, `replace()`)
- **Steps (run r2):** a) Edit `src/main.rs`, an agent writes it, Ctrl+S shows the conflict
  banner, click "Reload from disk", press Ctrl+Z, Ctrl+S. b) With a clean buffer, an agent writes
  the file (the editor shows the new text), press Ctrl+Z once (to undo one's own last typo, for
  example), Ctrl+S.
- **Expected:** the outside change is not part of my undo history, or undoing it is treated as an
  edit against the new disk version and is clearly visible; Save never replaces an agent's version
  without me seeing that I am doing it.
- **Actual:** a) Undo restores my text over the agent's, the ● comes back, and Save writes it:
  `14:09:12.344 -> fs.write_file ... expected_mtime_ms:1795529350012` (the agent's mtime taken by
  the reload), `<- fs.written`; disk now has my old text, the agent's `println!("agent version")`
  is gone, no banner. b) after `AGENT write` of `/* agent v3 */`, one Ctrl+Z brings back the
  pre-agent file with ● (`14-undo-past-agent.png`); saving it would overwrite v3 the same way.
  Cause: `replace()` dispatches the disk text as an ordinary undoable transaction and moves
  `mtime` forward, so the conflict check passes.
- **Why it matters for Andrii:** one Ctrl+Z reverts an agent's work on disk; he will not notice
  because the editor looks like his own state.
- **Suggested fix:** dispatch outside reloads with `addToHistory: false` (CodeMirror
  `Transaction.addToHistory.of(false)`) or reset history on reload; or keep the history but keep
  the old base `mtime` until the user accepts the new version. Test: reload then undo leaves the
  doc clean.

### ORCH-13 major: saving after an agent deleted or renamed the file recreates it silently

- **Area:** crates/fs-index (`files.rs`, `write`), ui/src/editor
- **Steps (run r7):** 1. Edit `src/lib.rs` (unsaved). 2. An agent deletes it. 3. Ctrl+S.
  4. Type more; an agent renames `src/lib.rs` to `src/renamed.rs`; Ctrl+S.
- **Expected:** after step 2 the editor says "deleted on disk" (keep editing / close); Save
  answers with a conflict like for a modified file, and "Save anyway" recreates it on purpose.
  After a rename, the editor offers to follow the file to its new name.
- **Actual:** nothing changes on screen after the delete (`52-dirty-deleted.png`, `alerts=0`).
  Save succeeds: `14:12:15.160 -> fs.write_file src/lib.rs`, `<- fs.written`; the deleted file is
  back. After the rename the save recreates `src/lib.rs` next to `src/renamed.rs`, so the project
  now has both. `write` checks `expected_mtime_ms` only when the target exists; a missing target
  is written without a conflict.
- **Why it matters for Andrii:** "the agent deleted that file" is a decision he reviews; Ovra
  undoes it behind his back and the next build has a duplicate module.
- **Suggested fix:** in `files::write`, treat `expected_mtime_ms: Some(_)` with a missing target as
  `conflict` ("deleted on disk"); `None` keeps creating files. Same in `fakes::MemoryFs`. In the
  editor, handle `fs.changed` for the open path when it is dirty: read it, and on `not_found` show
  the deleted state instead of waiting for Save.

### ORCH-14 major: closing a tab with unsaved changes neither asks nor shows where the text went

- **Area:** ui/src/editor (`Tabs.svelte`, `commands.ts` `editor.close_tab`)
- **Steps (run r4):** 1. Edit `src/main.rs`, switch to view (Ctrl+E). 2. Click the ●/× of the
  `main.rs` tab; or, in the editor, press Ctrl+W.
- **Expected:** "Save changes to main.rs? Save / Don't save / Cancel", as every editor does.
- **Actual:** the tab closes at once, no dialog. The text is kept in the hidden in-memory stash:
  reopening `src/main.rs` and pressing Ctrl+E shows it again with ● (`C reopen ... dirty=1`). With
  the tab closed, nothing in the UI says that `main.rs` has unsaved text, and that text is then
  lost by ORCH-10 (agent writes the file) or ORCH-11 (reload).
- **Why it matters for Andrii:** he closes tabs in bulk at the end of a review; work he forgot
  about vanishes later with no trace.
- **Suggested fix:** confirm on closing a dirty tab (Save / Discard / Cancel); Discard really
  discards. Until there is a dialog, at least keep a dirty tab open.

### ORCH-15 major: while the buffer is dirty, an outside change is invisible until Save, and the banner can only destroy one side

- **Area:** ui/src/editor (`session.svelte.ts`, `Editor.svelte`)
- **Steps (run r2):** 1. Edit `src/main.rs`. 2. An agent writes it (`fs.changed` arrives at once).
  3. Wait. 4. Ctrl+S.
- **Expected:** a notice as soon as the file changes ("main.rs changed on disk"), with "Compare"
  (the viewers already have a diff renderer, `@codemirror/merge` is planned in 1.E) next to
  "Overwrite" and "Reload".
- **Actual:** after step 2 nothing changes for 800 ms and beyond (`11-dirty-agent-wrote.png`,
  `ALERTS 0`); `session` ignores `fs.changed` while dirty. Step 4 gives `<- conflict: src/main.rs
  changed on disk` and the banner "The file changed on disk. Overwrite / Reload from disk"
  (`12-save-conflict.png`). Neither shows what the agent wrote; "Overwrite" (frame
  `expected_mtime_ms: null` at 14:09:14.549) throws the agent's change away, "Reload from disk"
  throws mine away (recoverable only by Undo, which leads to ORCH-12). The banner does not say who
  or when.
- **Why it matters for Andrii:** he must judge the agent's version before choosing; today he has
  to choose blind or open the file elsewhere (with no open-external control, ORCH-16).
- **Suggested fix:** on `fs.changed` for a dirty open file, read it and set `conflict` right away
  (role `status`, not only on Save). Add "Compare" showing disk vs. buffer; keep "Overwrite" and
  "Reload" but make "Reload" keep the buffer recoverable (ORCH-12 fix).

### ORCH-16 major: open-external has no control anywhere in the UI

- **Area:** ui/src/editor or ui/src/viewers (a command and a button), ui/src/tree (context menu)
- **Steps:** look for "Open in external editor" in the palette (queries `open`, `external`), on
  right-click of a tree row, in the editor bar, and on the "too large" and "binary" messages.
- **Expected:** a command (palette and a shortcut) and a button next to "This file is too large
  to show" / "Binary file ... cannot be shown", which are the cases where he needs it most.
- **Actual:** the palette has six commands (Close tab, Next tab, Previous tab, Save file, Show
  command palette, Toggle editing) and none opens externally; no context menu (`menu` count 0).
  The core side works: called over the WebSocket from a script with `VISUAL` set to a recording
  script, `open.external src/main.rs line 2` returned `ok` and the editor received the absolute
  path, a path with spaces worked, `../../etc/passwd` gave `forbidden: path ... leaves the
  project`, a missing file gave `not_found` (run r6). Only a folder gives a misleading
  `not_found: no such file: .../src`.
- **Why it matters for Andrii:** large generated files and binaries are common agent output; the
  viewer tells him it cannot show them and offers nothing.
- **Suggested fix:** register `open.external` as a command ("Open in external editor", Mod+Shift+O
  or similar) using the open path and the editor's cursor line; add a button on the too-large and
  binary messages. Core: say "is a folder" for directories.

### ORCH-17 major: the editor jumps to the top and moves the cursor to line 1 when an agent changes the open file

- **Area:** ui/src/editor (`session.svelte.ts`, `replace()`)
- **Steps (run r7):** 1. Open a 200-line `src/lib.rs` in edit mode, click line 6, press ArrowDown
  100 times (cursor on `pub fn f105()`, scrollTop 1373). 2. An agent inserts one line at the top.
  3. Type `// mine`.
- **Expected:** cursor and scroll stay on `f105` (shifted by one line), as the markdown viewer
  already manages (scrollTop 4000 stayed 4000 over ten appends).
- **Actual:** scrollTop becomes 9, the selection is gone, and the typed text lands on line 1: the
  next save wrote `// mine // header added by agent\npub fn f0()...` (frame 14:12:15.160).
  `replace()` swaps the whole document in one change, so CodeMirror maps the cursor to 0.
- **Why it matters for Andrii:** he reads the file the agent is still writing; every write throws
  him to the top, and anything he types goes to the wrong place.
- **Suggested fix:** apply a minimal diff (common prefix and suffix) instead of replacing the
  whole document, or restore the selection's line/column and scroll after `replace()`. Mark the
  outside update briefly ("updated on disk") so he knows why the text moved.

### ORCH-18 minor: a file opened from the tree leaves edit mode, and the viewer hides that the file has unsaved changes

- **Area:** ui/src/editor (`show`), ui/src/viewers, ui/src/tree
- **Steps (runs r3, r4):** 1. Edit `src/main.rs`. 2. Click `src/lib.rs` in the tree. 3. Click the
  `main.rs` tab.
- **Expected:** a file chosen while editing opens in the editor, or at least the viewer shows
  "Unsaved changes are not shown here - Edit (Ctrl+E)" for a dirty file. The tree highlights the
  active tab's file.
- **Actual:** the tree opens files in view mode (`openFile(path)` defaults to `view`), so step 3
  shows the disk text of `main.rs` in the plain viewer with only the ● on the tab
  (`22-back-to-main.png`, `31-E-view-of-dirty.png`); the tree keeps `lib.rs` highlighted.
  Ctrl+Tab between tabs keeps edit mode and the text (run r9), which is the better behaviour.
- **Why it matters for Andrii:** he cannot tell whether the text he sees is what is on disk or
  what he will save.
- **Suggested fix:** keep the current mode when opening from the tree (as `show()` does for
  tabs); in view mode of a dirty file, show a one-line notice with an "Edit" button; sync the tree
  selection with the active tab.

### ORCH-19 minor: a deleted or renamed open file leaves a tab that looks normal

- **Area:** ui/src/editor (`Tabs.svelte`, `open-files.svelte.ts`), ui/src/viewers
- **Steps (runs r1, r9):** 1. Open `src/lib.rs`. 2. An agent renames it to `src/core.rs`
  (or deletes it, also while Ovra is closed).
- **Expected:** the tab follows the rename, or is marked deleted (struck through) with "Close".
- **Actual:** the viewer says "src/lib.rs no longer exists." within 143 ms (good, round 1's
  ORCH-8 is half fixed), but the tab `lib.rs` looks like any other (`07-renamed.png`), and the
  stored `open_files` keep the dead path, so it comes back after reopening the project. The tree
  shows `core.rs U` and nothing for `lib.rs` (ORCH-2).
- **Suggested fix:** mark tabs whose file is missing; offer "Open core.rs" when a rename is
  detectable (same directory, one `fs.changed` batch with one removal and one addition).

## Round 1 re-check

| ID     | Severity | Status                    | Notes (real core, this build)                                                                                                                                                                                                                                                                                                            |
| ------ | -------- | ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ORCH-1 | blocker  | **still open, now worse** | Killing the daemon (run r8): status keeps "Connected to ovra 0.1.0 (headless)"; clicking `docs` shows "…" forever; Ctrl+S in the editor sends nothing and shows nothing. Recovery is a reload, which loses unsaved text (ORCH-11). `transport.ts` is unchanged.                                                                          |
| ORCH-2 | major    | still open                | Deleting tracked `src/lib.rs` removes its row; `mv lib.rs core.rs` shows only `core.rs U` (run r1).                                                                                                                                                                                                                                      |
| ORCH-3 | major    | still open                | Select `src/lib.rs`, agent renames it: `document.activeElement` is `BODY`, no selected row, ArrowDown does nothing (run r8).                                                                                                                                                                                                             |
| ORCH-4 | major    | **partly fixed**          | Reopening a project restores tabs, the active file and expanded folders (`docs`, `src`) from `projects.get_state` (run r5). Still: no way back to the project list or to another project (header name, Escape, palette `project`/`switch`: nothing); a reload shows "No project"; edit mode is not restored; no URL deep link.           |
| ORCH-5 | major    | still open                | Tree names are still `"▾ src •"`, `"main.rs M"`; no `data-git`/`aria-description`; status has no `data-state`. New test ids help: `tabs`, `editor-path`, `editor-dirty`, `markdown-view`, `patch-view`, `text-view`; tabs use `role="tab"` with `aria-selected`, and close buttons say "(unsaved changes)", which is useful for scripts. |
| ORCH-8 | minor    | changed                   | The viewer now says "<path> no longer exists."; renames are still not followed (ORCH-19).                                                                                                                                                                                                                                                |

## Will today's state fit agents and the inbox?

- Unsaved text is per page and in memory. Phase 3 direct mode (ADR 0005) marks "files changed
  outside the agent (you in the editor ...)" and never rolls them back; that only works if Ovra
  writes what the user typed and never writes stale text over an agent (ORCH-12, ORCH-13). The
  editor needs an explicit "my buffer vs. disk" state before the inbox can show it.
- The conflict banner is the natural place for "this file is owned by agent X right now"; give it
  a component with room for a source and a compare action rather than one line of text.
- `workspace.openPath` is still a bare path; agent worktrees will need `{ project, path }`
  (round 1 note, unchanged).

## What worked well

- Viewers follow outside writes quickly: README 98 ms, patch 119 ms, text 147 ms, an atomic
  write-and-rename 72 ms, `git checkout` correct; the markdown viewer kept scrollTop 4000 over ten
  appends in one second (`50-md-after-appends.png`).
- A clean editor buffer follows outside writes, and the conflict check catches a modified file on
  Save (`<- conflict: src/main.rs changed on disk`); repeated Ctrl+S does not overwrite.
- Ctrl+Tab / Ctrl+Shift+Tab keep edit mode and the unsaved text; Ctrl+W closes the tab instead of
  the browser tab.
- Binary and too-large files say so plainly in both modes.
- Per-project restore of tabs, active file and expanded folders is quick and correct, and the
  partial `projects.set_state` writes do not clobber each other.
- `open.external` in the core is careful: path checks, terminal editors skipped, spaces handled.
