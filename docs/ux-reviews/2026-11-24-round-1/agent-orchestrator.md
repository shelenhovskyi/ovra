# Agent orchestrator (Andrii): Round 1 (2026-11-24)

Build: dd5ffce on `main` plus the UX harness (2ed3ba4), headless, Chromium 158.0.8127.36, 1280x800.
Scope: shell layout and status bar (`ui/src/shell`), start screen and file tree with git status and
live updates (`ui/src/tree`), command palette (`ui/src/palette`), global shortcuts (`ui/src/keys`).
Viewer/Editor placeholders (1.E/1.F) and the stub project store (1.B) are not findings by themselves.
Fixtures: `small`, `monorepo` (stand-in core, `fs.changed`/`fs.git_changed` from Node `fs.watch`).

## Summary

Andrii can watch agents work on one project: new and modified files show up with the right badge
about 85-90 ms after the write (stand-in core), and commits clear the badges in about 160 ms. But he
cannot trust what he sees: when the core connection drops the status bar keeps saying "Connected"
while the tree silently freezes, deleted files are invisible, and any rename, regenerated folder or
mouse-wheel scroll throws away his selection and keyboard focus. Projects cannot be switched without
reloading the page, which also forgets the open project, so the overall feel is "a nice live tree
for one folder", not yet a place to supervise several agents.

## Findings

### ORCH-1 blocker: losing the core connection is not shown; the tree freezes silently and requests hang forever

- **Area:** ui/src/lib/transport.ts, ui/src/shell (status bar)
- **Steps:** 1. Open `demo-app`, expand `src`. 2. Close the WebSocket (`window.__ws.close()`
  through a wrapped `WebSocket`, standing in for a daemon crash or sleep/wake). 3. Create
  `src/while_offline.rs` on disk. 4. Click the `docs` folder. 5. Wait 3 s.
- **Expected:** the status bar changes to "Disconnected, reconnecting…" (with a retry and a
  machine-readable state), the tree is marked stale, the click fails with a message, and on
  reconnect the tree reloads what it has open.
- **Actual:** status still reads "Connected to ovra 0.1.0 (headless)" after 3 s
  (`30-after-close.png`); the new file never appears; `docs` shows the loading "…" forever
  (`31-expand-while-offline.png`). Cause: `connectWebSocket` only calls `failAll` on close; frames
  sent after the close are pushed to `queue` and never sent or rejected, so the promise never
  settles; the status bar only reflects the one `info.get` made at start; there is no reconnect.
  The only recovery is a page reload, which also drops the open project (ORCH-4).
- **Why it matters for Andrii:** the tree is his "what did the agents change right now" signal. A
  frozen tree that claims to be connected makes him approve or reject work based on stale state.
- **Suggested fix:** give the transport a connection state (`connecting | open | closed`) that the
  shell shows and exposes as `data-state` on `[data-testid=status]`; reject calls made while
  closed; reconnect with backoff; on reconnect emit a "resync" so the tree runs `refreshAll()`.
  The bad-token case already shows "Core unavailable: connection to core closed"
  (`33-bad-token.png`); it should say the token was rejected and how to get a new URL.

### ORCH-2 major: deleted files disappear without a trace; renames look like brand-new files

- **Area:** crates/fs-index (listing), protocol `fs` (`crates/core/src/protocol/fs.rs`,
  `ui/src/lib/protocol/fs.ts`), ui/src/tree
- **Steps:** 1. Open `demo-app`, expand `src/util`. 2. An agent deletes the tracked
  `src/util/strings.rs`. 3. An agent renames the tracked `src/lib.rs` to `src/core.rs` (plain `mv`).
- **Expected:** `strings.rs` stays in the tree as a struck-through row with a `D` badge until the
  deletion is committed; the rename shows as `core.rs` `R` (or `lib.rs` `D` + `core.rs` `U`).
- **Actual:** `strings.rs` vanishes; the only hint is a red dot on `util` and `src`
  (`11-deleted-tracked.png`). The rename shows only `core.rs U`; `lib.rs` is gone
  (`12-renamed-selected.png`). The red "deleted" colour also wins over everything else on `src`,
  so a folder with one deletion and many edits just looks "deleted". The real core lists from disk
  the same way (`listing.rs` iterates `read_dir`), so this is not a stand-in artifact; the `D`
  badge in `GIT_LETTER` can never appear on a file.
- **Why it matters for Andrii:** "the agent deleted the migration" is exactly the change he must
  catch, and today it is the one change the tree cannot show.
- **Suggested fix:** small protocol task first: let `fs.list_dir` include tracked-but-missing
  entries (`kind: "file"`, `git: "deleted"`, a `missing: true` flag). Tree renders them dimmed and
  struck through. Optionally add counts per status to folder rows (title or `aria-description`).

### ORCH-3 major: selection and keyboard focus are lost on rename, regeneration and plain scrolling

- **Area:** ui/src/tree (FileTree.svelte, model.ts)
- **Steps:** a) Select `src/lib.rs`; an agent renames it. b) Expand `src/util`, select
  `strings.rs`; an agent runs `rm -rf src/util` and recreates it 250 ms later (codegen, branch
  switch, `git stash`). c) In `monorepo`, expand `generated`, click `file-00000.json`, scroll the
  tree with the mouse wheel by 3000 px, press ArrowDown.
- **Expected:** a) and b) selection moves to the renamed file or the nearest sibling/parent, the
  recreated folder keeps its expanded state; c) ArrowDown moves the selection from where it was.
  In every case focus stays on the tree.
- **Actual:** a) selection becomes null and `document.activeElement` becomes `BODY`; ArrowDown
  does nothing. b) `util` comes back collapsed, selection null, focus on `BODY`
  (`50-after-regen.png`; `forget()` drops `expanded` for vanished folders). c) after the wheel
  scroll focus is on `BODY` and ArrowDown, PageDown, End and ArrowUp do nothing in the tree
  (selection stays null). Cause: a click focuses the row element itself (`tabindex="-1"`); when
  virtualization or a refresh removes that element, focus falls to the body.
- **Why it matters for Andrii:** agents rename and regenerate files constantly; each time he has
  to find his place again with the mouse and click back into the tree.
- **Suggested fix:** keep DOM focus on the `role="tree"` viewport and point at the current row with
  `aria-activedescendant` (focus the viewport on row mousedown). Keep `expanded` for paths that
  vanish and come back (forget them only on collapse or after a timeout). When the selected path
  disappears, select the next sibling or the parent instead of nothing.

### ORCH-4 major: no way to switch projects, and a reload forgets the open project

- **Area:** ui/src/shell, ui/src/tree (ProjectList), ui/src/palette / ui/src/keys (commands)
- **Steps:** 1. Open `demo-app`. 2. Try to get back to the project list: Escape, click "Ovra",
  click the project name in the header, palette "project", browser Back. 3. Reload the page.
- **Expected:** a visible project switcher (header button or palette command "Open project…" /
  "Close project"), and after a reload the last project reopens.
- **Actual:** nothing in the UI leaves a project; the palette has a single command ("Show command
  palette"), "project" gives "No matching commands" (`04-palette-project.png`); browser Back leaves
  the app (`about:blank`). A reload shows "No project" and the start screen again, with all
  expanded folders lost (`03-after-reload.png`). The URL never changes, so an agent or a script
  cannot open the UI straight at a project either. `projects.remove` exists in the protocol but
  has no UI.
- **Why it matters for Andrii:** he moves between the main repo and agent worktrees all day, and
  ORCH-1's only recovery path (reload) puts him back at the start screen.
- **Suggested fix:** register `project.open` / `project.close` commands (palette, `Mod+O`-style
  shortcut) and make the header project name a button that opens the list. Restore the last
  project on load (from `projects.list` order or a `?project=<id>` URL parameter, which also gives
  scripts a deep link). Keep expanded folders per project in session storage.

### ORCH-5 major: the UI is hard for agents and scripts to drive reliably

- **Area:** ui/src/tree, ui/src/shell, ui/src/palette
- **Steps:** take an ARIA snapshot of the tree and the start screen (see `session.log` of run 1)
  and try to locate "the modified file" or "the connection state" without CSS classes.
- **Expected:** stable, plain accessible names and data attributes for the state an agent reads.
- **Actual:**
  - Tree item names include drawing glyphs and badge letters: `treeitem "▾ src •"`,
    `"main.rs M"`, `"▸ .git"`. `getByRole("treeitem", { name: "src" })` fails; scripts need regexes.
  - Git status is only a CSS class (`git-modified`) and a `title` on the badge; ignored state is
    only `opacity`. Neither is in the accessible name, description or a `data-*` attribute.
  - Connection state is free text in `[data-testid=status]` (and wrong after a disconnect, ORCH-1).
  - Only five test ids exist: `sidebar`, `project-list`, `main`, `status`, `file-tree`. Project
    buttons have the folder name only; two projects called `demo-app` (a repo and a copy or
    worktree) are two identical `button "demo-app"` (`40-two-same-name.png`); the path is only a
    `title` tooltip.
  - The start screen sits under an `h2 "File tree"` heading followed by `h2 "Projects"`.
  - Good: rows carry `data-path`, which is stable and was enough to script everything here.
- **Why it matters for Andrii:** he wants agents (and his own scripts) to drive Ovra; every one of
  these forces brittle selectors that break when visuals change.
- **Suggested fix:** mark the twisty and badge `aria-hidden="true"` and put the status into
  `aria-description` ("modified", "ignored"); add `data-git` and `data-ignored` to rows and
  `data-state` to the status bar; give project buttons an accessible description with the root
  path and show the parent folder when names collide; render the "File tree" heading only with
  the tree.

### ORCH-6 minor: every git change reloads every directory ever loaded, and expanded rows flash "…"

- **Area:** ui/src/tree (model.ts `refreshAll`, `fetch`)
- **Steps:** 1. In `monorepo`, expand 15 packages and their `src` folders. 2. Create 3 files in
  `packages/pkg-007/src`. 3. Append to `packages/pkg-000/src/module-1.ts` ten times in 1.5 s.
- **Expected:** a change reloads the affected directories and refreshes git badges cheaply; rows
  that are already shown do not flicker.
- **Actual:** step 2 sent 32 `fs.list_dir` calls, step 3 sent 320; the `…` loading hint was
  inserted 21 times on expanded folders during step 3. `refreshAll()` refetches every entry in
  `dirs`, including collapsed folders, and `fetch()` marks each as loading. With the real core
  `fs.git_changed` fires only when the status set changes, but each new untracked file an agent
  creates does that. (Frame counts are from the stand-in, which sends `git_changed` on every
  change; the fan-out per event is the UI's.)
- **Why it matters for Andrii:** with five to ten agents creating files the tree keeps blinking
  and the request rate grows with how much he has browsed, not with what changed.
- **Suggested fix:** on `fs.git_changed` refetch only expanded directories (drop cached listings of
  collapsed ones, or mark them stale and reload on expand); show the loading hint only for a first
  load, not for a refresh; longer term, a protocol event carrying the changed statuses would avoid
  the refetch entirely (protocol task).

### ORCH-7 minor: the row under the user's eyes jumps when agents add files above it

- **Area:** ui/src/tree (FileTree.svelte)
- **Steps:** 1. In `monorepo`, scroll to `packages/pkg-091/src/module-3.ts` and select it. 2. An
  agent adds three files to the expanded `packages/pkg-007/src` (above the viewport).
- **Expected:** the selected row stays at the same screen position (scroll anchored to it).
- **Actual:** `scrollTop` stays 17439 while the row moves from y=395 to y=461
  (`20-before.png`, `21-after-insert-above.png`); with constant churn the view keeps sliding.
- **Why it matters for Andrii:** he reads a folder while agents write elsewhere; the list moving
  under the pointer leads to clicking the wrong file.
- **Suggested fix:** in `rebuild()` consumers, when rows are inserted or removed above the
  first visible row, adjust `scrollTop` by the row delta (anchor on the selected or first visible
  path).

### ORCH-8 minor: the open file path goes stale when the file is renamed or deleted

- **Area:** ui/src/shell (workspace.svelte.ts), ui/src/tree
- **Steps:** 1. Click `src/lib.rs` (main area shows `src/lib.rs`). 2. An agent renames it to
  `src/core.rs`.
- **Expected:** the main area follows the rename or says the file is gone.
- **Actual:** the main area still shows `src/lib.rs` (`12-renamed-selected.png`). The viewer is a
  placeholder, but nothing in the workspace reacts to `fs.changed` for `openPath`, so 1.E/1.F
  will inherit it.
- **Why it matters for Andrii:** reviewing a file that no longer exists on disk is reviewing the
  wrong thing.
- **Suggested fix:** decide in the workspace (one small shared-state task) who owns "open file
  vanished": the tree already sees the event and can call `workspace.openFile(null)` or a new
  `workspace.fileGone(path)`, and viewers show a "deleted on disk" state.

### ORCH-9 minor: ignored folders never update live with the real core

- **Area:** crates/fs-index (watch.rs), ui/src/tree
- **Steps (from code; the stand-in watches everything):** 1. Expand `target/` or `dist/` (shown
  dimmed). 2. An agent builds and writes new files there.
- **Expected:** either the expanded ignored folder updates, or the UI says it is not watched.
- **Actual:** `Worker::watch_tree` skips ignored directories, so no `fs.changed` arrives for their
  contents; the expanded folder shows its first listing until something else triggers a refresh.
- **Why it matters for Andrii:** checking build output an agent produced is a common review step.
- **Suggested fix:** start a non-recursive watch on an ignored directory while the UI has it
  expanded (needs an `fs.watch_dir`-style call or watching on `fs.list_dir`), or mark ignored
  expanded folders "not live" with a manual refresh.

## Will today's layout and state fit agents and the inbox?

- `Workspace` holds one `project` and one project-relative `openPath`. Phase 3 puts each agent in
  its own worktree (a different root). Opening an agent's file or diff needs `{ project, path }`,
  not a bare path; change `openPath` to carry the project id before 1.E/1.F build on it.
- `FileTree` already takes `project` as a prop and keys on it, so a second tree for an agent's
  worktree (or a "changed files only" filter) can be mounted without changes. Good.
- The shell grid is fixed to sidebar + main + one status line, with no slot for an agents list,
  terminals (2.B), approvals (2.E) or the inbox (3.D). Define named regions now (left activity
  bar, bottom panel, right panel) so these tasks do not all edit `Shell.svelte`.
- The status bar shows only the core connection. It will need room for per-agent state and
  pending approvals, and should show the current git branch (agents switch branches; nothing in
  the UI shows that a checkout happened).
- Commands are global and the palette has a single entry; agents/inbox will need commands scoped
  to the focused panel (approve, reject, next diff). The registry shape fits, but a `when`/scope
  field will be needed (a hot spot change to `keys/commands.ts`).

## What worked well

- Live updates are fast and correct for create, modify, `git mv` (`R`), commit and branch
  checkout: 85-90 ms with the stand-in's 50 ms debounce, 17 ms for a checkout, 160 ms for commit
  to clear badges. Selection survives changes elsewhere in the tree.
- Folder status roll-up (`•` on `src`) makes "where did the agent touch things" visible at a glance.
- Virtualization holds: 1000+ rows keep about 50 tree items in the DOM.
- `data-path` on every row is a stable, precise hook for automation.
- Clear error for a bad project path: "not_found: no such folder: /nope/agent-worktree".
