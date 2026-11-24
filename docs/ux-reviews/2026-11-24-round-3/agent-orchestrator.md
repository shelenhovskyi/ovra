# Agent orchestrator (Andrii): Round 3 (2026-11-24), regression

Build: 9512ea6 on `main`, real core, headless, Playwright Chromium, 1280x800.
Scope: re-check of every ORCH finding from rounds 1 and 2, with priority on the five unsaved-text
loss paths and core disconnect and recovery; changes since 485e5f2 (drafts and conflicts in
`ui/src/editor`, connection status and reconnect in `ui/src/lib/transport.ts`, project switcher,
browser-safe shortcuts, Go to file over `fs.list_files`, virtualized diffs, visible-rows tree
refresh, deleted and renamed files in listings). Fixtures: `small`, `monorepo`. 17 scripted runs
(r01-r17). Agents are simulated by writing, deleting, renaming, `git mv` and regenerating files in
the fixture from the script. The connection was dropped by routing the page's WebSocket in the
script and refusing new sockets until "the core is back".

## Summary

Andrii no longer loses typed text in any of the five paths from round 2: reload, closing a dirty
tab, the tab limit, an outside write to a dirty background tab, and Save over a changed or deleted
file all keep the text or ask first. Saving while the core is gone fails with a message and keeps
the text. The status bar shows a lost connection within 11 ms and reconnects within 0.75 s. The
connection work is still unsafe in one place: **expanding a folder while disconnected freezes the
page for good** (ORCH-20). After a reconnect, nothing re-reads what the agents did during the
outage (ORCH-21). If the core is not up when the page loads, the start screen stays empty
(ORCH-22). The tree, Go to file, the project switcher and big diffs now keep up with agents well.
Overall it feels like a tool he can review agent work in. He still has to reload by hand after
any outage, which costs him the open project (ORCH-4).

## Regression table

Status counts: 8 fixed, 10 partly fixed, 1 still open, 0 regressed.

| ID      | Round | Old severity | Status       | Evidence (this build)                                                                                                                                                                                                                                                                                                                                                                                                                   |
| ------- | ----- | ------------ | ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ORCH-1  | 1     | blocker      | partly fixed | r09: status shows "Connection to core lost, reconnecting…" 11 ms after the drop. Ctrl+S offline shows "connection to core closed" and keeps the ● and the text. Reconnect takes 749 ms; the next Save wrote the text. Still broken: expanding a folder offline freezes the page (ORCH-20), nothing resyncs (ORCH-21), the start screen stays empty (ORCH-22), and `[data-testid=status]` has no `data-state` (only class `error`).      |
| ORCH-2  | 1     | major        | fixed        | r12: `rm src/lib.rs` leaves `lib.rs [git=deleted]`, struck through, titled "src/lib.rs (deleted)". `git mv Cargo.toml Cargo2.toml` gives "Cargo2.toml (renamed, from Cargo.toml)". A plain `mv` shows D + U, which is honest. Small leftover: a folder whose only file was deleted collapses (r12/r13); once it was left expanded with no rows (r05 `33-renamed-dirty.png`).                                                            |
| ORCH-3  | 1     | major        | partly fixed | Focus stays on `role=tree` with `aria-activedescendant` after renames, deletes and wheel scrolls (r12, r14: wheel -3000 px, then ArrowDown moves from `pkg-182` to `pkg-182/dist`). Still open: a regenerated `gen/` comes back collapsed, and the selection jumps to an unrelated row by index (`gen/b.rs` selected, then `src/util` after the regeneration, r12 3c). Deleting the selected `lib.rs` moves the selection to `main.rs`. |
| ORCH-4  | 1     | major        | partly fixed | r13: palette "Switch project" lists both projects with paths and switches. "Close project" works. Reload still lands on "No project" (r02, r13: `after reload header=Ovra No project`). The header name is plain text, not a control. There is no deep link.                                                                                                                                                                            |
| ORCH-5  | 1     | major        | partly fixed | r12 ARIA: `treeitem "src" [expanded]: src contains deleted files`, `treeitem "lib.rs": lib.rs deleted`. Rows have `data-git` and `data-ignored`. Project buttons are named "demo-app /tmp/.../demo-app". Still open: the status has no machine-readable state, and the header project is a `text` node.                                                                                                                                 |
| ORCH-6  | 1     | minor        | fixed        | r14, monorepo with 15 packages and their `src` expanded: creating 3 files sent 4 `fs.list_dir` (round 1: 32); 10 appends sent 14 (round 1: 320). The loading hint was inserted 0 times.                                                                                                                                                                                                                                                 |
| ORCH-7  | 1     | minor        | fixed        | r14: `packages/pkg-182` selected; 3 files added in expanded `pkg-003/src` above it; row y 336 -> 336, scrollTop 21268 -> 21268.                                                                                                                                                                                                                                                                                                         |
| ORCH-8  | 1     | minor        | partly fixed | The viewer says "src/util/strings.rs no longer exists." (r13 `80-deleted-folder-expanded.png`). The tab does not follow renames; see ORCH-19.                                                                                                                                                                                                                                                                                           |
| ORCH-9  | 1     | minor        | partly fixed | r14: a new file in expanded ignored `pkg-000/dist` is not shown after 1.5 s. It appears after a window `focus` event. Nothing tells him the folder is not live.                                                                                                                                                                                                                                                                         |
| ORCH-10 | 2     | blocker      | fixed        | r03: dirty `main.rs` in the background; the agent writes it. The text is kept. In edit mode the banner reads "src/main.rs changed on disk while you have unsaved changes. Overwrite with my version / Discard my changes and reload" (`13-bg-agent-edit.png`). Same after switching projects and back (r15). The visibility gap is ORCH-24.                                                                                             |
| ORCH-11 | 2     | blocker      | fixed        | r02: reload 105 ms after typing (shorter than the 300 ms draft timer) still kept both notes. `localStorage` has `ovra.draft.<id>:src/main.rs` and `:src/lib.rs`, and reopening restores both tabs with ● and the text. No dialog appears; drafts are flushed on `beforeunload` instead. The project itself is not reopened (ORCH-4).                                                                                                    |
| ORCH-12 | 2     | major        | fixed        | r03: "Discard my changes and reload", then Ctrl+Z twice: the document stays clean with the agent's text, and Ctrl+S wrote nothing over it. With a clean buffer and an agent write of `/* agent v3 */`, Ctrl+Z does nothing.                                                                                                                                                                                                             |
| ORCH-13 | 2     | major        | fixed        | r05: a dirty `strings.rs` deleted on disk shows "was deleted on disk. Save as a new file / Close without saving" 109 ms later. Ctrl+S does not recreate it (`exists=false`). The same holds after `mv lib.rs renamed.rs`.                                                                                                                                                                                                               |
| ORCH-14 | 2     | major        | fixed        | r03: Alt+W and the tab's close button on a dirty tab both open the `alertdialog` "Save changes to main.rs before closing? Save / Don't save / Cancel". Escape and Cancel keep the tab.                                                                                                                                                                                                                                                  |
| ORCH-15 | 2     | major        | partly fixed | r05: the conflict banner appears 89 ms after the outside write, without Save. There is still no Compare, and nothing says who changed the file or when.                                                                                                                                                                                                                                                                                 |
| ORCH-16 | 2     | major        | partly fixed | The viewer bar has "Open in default app", and so do the binary and too-large messages. Still missing: a palette command and shortcut, and a control in the editor bar. On failure the message drops the reason (`catch {}`). The button is also shown for a deleted file (r13 `80-...png`).                                                                                                                                             |
| ORCH-17 | 2     | major        | partly fixed | r05: no longer jumps to the top, but the cursor keeps its character offset. After the agent inserted one line at the top, the active line went from `f105` to `f104`, scrollTop jumped 1395 -> 1945, and typing produced `pub fn// mine-17 f104() {}` (`32-cursor-after-agent.png`). Console: "Measure loop restarted more than 5 times".                                                                                               |
| ORCH-18 | 2     | minor        | partly fixed | The viewer of a dirty file shows the unsaved text with "Showing unsaved changes from the editor." (`05-tab-main.rs.png`), and the tree highlights the active tab's file. A tree click still drops to view mode (r02: `editorPath:null` after the click). The viewer has no Edit or Save control, and Ctrl+S does nothing there (ORCH-23).                                                                                               |
| ORCH-19 | 2     | minor        | still open   | r05/r13: tabs of deleted files (`strings.rs`, renamed `lib.rs`) look like any other tab. A rename shows "deleted on disk" with no offer to follow `renamed.rs`.                                                                                                                                                                                                                                                                         |
| path 3  | 2 sum | blocker      | fixed        | Tab limit, r04: 26 files opened, 20 tabs left; the two dirty tabs (`main.rs`, `lib.rs`) were kept, and the oldest clean tabs (README, f01-f06) closed.                                                                                                                                                                                                                                                                                  |

## New findings

### ORCH-20 blocker: expanding a folder while the core is disconnected freezes the page for good

- **Area:** ui/src/tree (`model.ts`, `pump()` / `due()` / `fetchFirst()`)
- **Steps (r08b, r06):** 1. Open `demo-app`. 2. Lose the connection (socket closed; new sockets
  refused, as with a core restart or laptop sleep). 3. Click the collapsed `docs` folder. 4. Bring
  the core back.
- **Expected:** the folder shows "could not load (disconnected)" or stays loading. When the
  connection returns, it loads.
- **Actual:** the renderer stops responding. `page.evaluate` times out 1.2 s after the click, the
  click itself never completes, and a screenshot cannot be taken. With the core reachable again,
  the page was still frozen 10 s later. It made no reconnect attempt (`refused=0` in the route log),
  so the page never recovers. Without the click, the same outage behaves well (r07: status
  updates every 250 ms, reconnect attempts at 0.25/0.5/1/2 s). Cause (from code): while closed,
  `call()` rejects at once. `fetch()` swallows the error, the folder stays expanded and not loaded,
  and `due()` keeps returning it. The `for (; batch.length > 0; batch = this.due())` loop in
  `pump()` therefore spins on microtasks only and never yields to timers. While connected, a
  listing that fails with anything but `not_found` (permission denied, I/O error) will loop the
  same way, one request per round trip. That part is from reading the code; I could not produce
  such an error as root.
- **Why it matters for Andrii:** clicking around the tree is the first thing he does when
  something looks stale, which is exactly when the core is gone. The tab has to be killed. Text
  typed in the last 300 ms before the freeze has not reached drafts yet.
- **Suggested fix:** when a listing fails, record the failure per folder: show it on the row and
  leave the folder out of `due()` until the next `open` status or `fs.changed`. Never re-pump in
  the same pass after a failed fetch. Add a unit test with a transport whose `call` rejects
  synchronously.

### ORCH-21 major: after a reconnect nothing re-reads what changed during the outage

- **Area:** ui/src/tree, ui/src/editor (`Buffers`), ui/src/shell; `ui/src/lib/transport.ts`
- **Steps (r10, r09):** 1. Open `demo-app` with `src` expanded, `README.md` and a dirty `main.rs`
  open. 2. Drop the connection. 3. The agent writes `main.rs`, creates `src/while_offline.rs`,
  appends to `README.md` and deletes `src/lib.rs`. 4. The core comes back; wait 3 s.
- **Expected:** on reconnect the UI does what it does for `info.resync`: re-list the shown
  folders, re-check open buffers and viewers, and clear errors that were only about the outage.
- **Actual:** 3 s after "Connected", the tree has no `while_offline.rs`, and `lib.rs` is still
  listed as clean (`while_offline=0 lib.rs=1 git(lib)=null`, `50-after-reconnect.png`). The dirty
  `main.rs` shows no conflict until Save (Save then correctly gets the conflict). The "connection
  to core closed" banner from an offline Save stays on screen after reconnecting (r09). The
  README viewer only caught up because clicking its tab re-read the file. The only frames after
  reconnect are `info.get` and the core's `info.ready`. The core sends `info.resync` only to a
  subscriber that lagged, not to a new socket.
- **Why it matters for Andrii:** "what did the agents do while the core was down" is the first
  question after an outage. The tree answers it wrongly and gives no sign that it is stale.
- **Suggested fix:** when the transport goes from `closed` to `open`, deliver a local
  `info.resync` to listeners (or treat `info.ready` after a reconnect as one). This reuses the
  existing tree, buffer and viewer handlers. Clear `buffer.error` when it was a connection error.
  Add a unit test on `connectSocket` with the fake socket.

### ORCH-22 major: the start screen stays empty when the core comes up after the page, and a bad token is not explained

- **Area:** ui/src/tree (`ProjectList.svelte`), ui/src/shell (status text)
- **Steps (r11, r10):** a) Load the page while the core refuses connections (core still
  starting), then let it accept. b) Open the URL with `token=nope`, and again with no token.
- **Expected:** a) when the connection opens, the recent projects load. b) the status says the
  token was rejected and how to get a fresh URL, and stops retrying.
- **Actual:** a) the status says "Connected to ovra 0.1.0 (headless)", but the start screen still
  shows "connection to core closed" under the folder field, the field has a red invalid border,
  and the Recent list is gone (`60-core-late.png`). Only a reload helps. b) "Core unavailable:
  could not connect, retrying…" stays forever, with the same red field and message under it
  (`51-bad-token.png`). The console shows the reason ("HTTP Authentication failed"), but the UI
  never does. Retries continue every 10 s.
- **Why it matters for Andrii:** he restarts the daemon and reopens bookmarked URLs (with old
  tokens) all the time. The UI says "Connected" next to an empty project list, or "retrying" for a
  URL that can never work.
- **Suggested fix:** reload `projects.list` on every `open` status and clear its error. Show
  connection errors in the status bar, not as a field validation error. For a socket closed
  before it ever opened several times in a row, say "The core rejected this URL (token). Copy the
  current OVRA_URL from the daemon output." If the server can signal the auth failure (HTTP 401
  is not visible to `WebSocket`), the daemon could serve a small `/auth-check` for the UI to probe.

### ORCH-23 minor: Ctrl+S on a dirty file shown in the viewer does nothing

- **Area:** ui/src/editor (`editor.save` is registered by `Editor.svelte` only), ui/src/viewers
- **Steps (r16):** 1. Edit `src/main.rs`. 2. Press Ctrl+E to view, or click any file in the
  tree and come back with the tab (view mode, ORCH-18). 3. Press Ctrl+S.
- **Expected:** the file is saved, as the tab's ● and the notice "Showing unsaved changes from the
  editor." suggest is possible.
- **Actual:** nothing is saved (disk unchanged), and the key event is not prevented
  (`defaultPrevented=false`), so in a real browser Chrome opens "Save page as". The viewer has no
  Save button.
- **Why it matters for Andrii:** the tree puts him in view mode all the time; he presses Ctrl+S,
  sees the browser's dialog or nothing, and the file stays unsaved.
- **Suggested fix:** register `editor.save` in `editor/commands.ts` (any mode) for the active tab's
  buffer. In the viewer's unsaved notice add "Save" and "Edit" buttons.

### ORCH-24 minor: a dirty tab whose file changed on disk shows no sign of it until edit mode

- **Area:** ui/src/editor (`Tabs.svelte`), ui/src/viewers (unsaved preview)
- **Steps (r03, r15):** 1. Edit `main.rs`, open another file. 2. The agent writes `main.rs`
  (or does so while another project is open). 3. Look at the tab, then click it.
- **Expected:** the tab is marked "changed on disk" and the viewer says "The file changed on disk
  since you started editing", with the conflict actions.
- **Actual:** the tab shows only ●. The viewer shows "Showing unsaved changes from the editor."
  with the old base (`12-bg-agent-write-tab.png`, `95-back-after-switch.png`). The conflict
  appears only after Ctrl+E.
- **Why it matters for Andrii:** he reviews in view mode. He would see the agent's change only if
  he happened to enter edit mode.
- **Suggested fix:** expose `buffer.conflict` and `buffer.deleted` to the tab (icon plus accessible
  name "main.rs, unsaved changes, changed on disk"). Show the same banner in the viewer's unsaved
  preview.

### ORCH-25 minor: unsaved tabs scroll out of sight and nothing counts them

- **Area:** ui/src/editor (`Tabs.svelte`), ui/src/shell (status bar)
- **Steps (r04):** 1. Edit `main.rs` and `lib.rs`. 2. Open 25 more files from the tree.
- **Expected:** a count of unsaved files somewhere always visible ("2 unsaved"), and a way to
  list them.
- **Actual:** the tab limit correctly keeps both dirty tabs, but they sit at the left of a strip
  that is 1548 px wide in a 1020 px area, scrolled away (`20-tab-limit.png`). The clean tabs that
  were closed go without a word. Nothing else shows that unsaved text exists, so after a long
  review he cannot tell if anything is pending.
- **Suggested fix:** show "N unsaved" in the status bar (a button that cycles through them) and add
  a palette command "Show unsaved files". Optionally pin dirty tabs visibly.

### ORCH-26 minor: two browser windows share drafts, and the last one to write wins

- **Area:** ui/src/editor (`drafts.svelte.ts`)
- **Steps (r15):** 1. Window A edits `main.rs`. 2. Window B (a second tab, or a script driving its
  own page) opens the same project and edits `main.rs`. 3. Window A types `// TAB1-MORE`.
  4. Window B reloads.
- **Expected:** each window's unsaved text survives, or the second window is told the file has
  unsaved text in another window.
- **Actual:** window B opens A's draft as if it were its own unsaved text. After B's reload the
  stored draft is `...// SWITCH-TEXT// FROM-TAB-2`, and A's `// TAB1-MORE` is gone from storage.
  It survives only in A's memory until A reloads, and A's reload then overwrites B's text.
- **Why it matters for Andrii:** he wants scripts and agents to drive Ovra next to him; a second
  page is the natural way to do that.
- **Suggested fix:** listen to the `storage` event and treat a foreign draft change like an
  outside change (conflict), or key drafts by window as well and offer both on restore.

### ORCH-27 polish: small rough edges around outside changes

- **Area:** ui/src/viewers, ui/src/editor
- **Steps and actual:** "Open in default app" is offered for a file that no longer exists (r13).
  An outside write to the open editor logs "Measure loop restarted more than 5 times" (r05).
  Connection errors read in lowercase core wording ("connection to core closed") inside the editor
  banner (r09 `41-save-offline.png`).
- **Suggested fix:** hide the button when the file is gone; restore the scroll position after the
  layout settles (`requestMeasure`); word the offline save error as "Not saved: the core is
  disconnected. Your text is kept; save again when it reconnects."

## What worked well

- None of the five round-2 paths lost text. Drafts survive a reload that happens 105 ms after
  typing, and both dirty tabs come back with ● (r02). The close dialog is a proper `alertdialog`,
  and Escape cancels it.
- Conflicts show up without Save: 89 ms for a modified file, 109 ms for a deleted one. Save never
  recreates a deleted file or writes over a changed one, including after an outage, where the
  first Save after reconnect correctly got the conflict (r10).
- The disconnect is visible within 11 ms, reconnect backs off as designed (0.25, 0.5, 1, 2 s), and
  an offline Save fails at once with the text kept (r07, r09).
- The tree can be trusted much more: deleted files are struck through with D, renames show where
  they came from, focus survives every change, and a git change costs a handful of listings
  instead of hundreds (r12, r14).
- Accessible names are clean ("lib.rs", description "deleted"), and `data-git`, `data-ignored`
  and `data-path` make the tree easy to script.
- Go to file picks up a file an agent created 400 ms earlier (r13). Alt+P and Ctrl+Shift+P work
  in the browser, and the project switcher lists names with paths.
- A 20,000-line, 200-file patch renders in 298 ms with 615 DOM nodes. Its scroll position
  (20000) survives an agent appending to it (r16).
