# Components

Sizes and colours refer to [tokens.md](tokens.md).

## Window frame

```
[● ● ●]  [◧] demo-app ▾  ⎇ main ▾      [ Search or run      ⌘K ]      [Inbox 2] [◨] [⋯]
 leading: sidebar toggle, project, branch   centre: one field        trailing: Reviews, Agents toggle, More
```

- Title bar 44 px on `--s0`. Traffic lights centred on the bar; leading
  padding leaves about 20 px after the zoom button. In full screen the
  padding drops to 16 px.
- Three groups, no more. Items are borderless `--ctl` circles (`--r-control`),
  `--s3` on hover, `--s4` pressed. Empty space drags the window.
- The branch is a button that opens the branch menu.
- The centre field is 32 px, `--s1`, reads "Search or run" with a Mod+K key
  cap. It is the only field in the bar.
- Reviews shows a count badge only when something waits. A waiting approval
  adds a `--warn` "1 waiting" chip next to it while the Agents column is
  hidden.
- With no project open (start screen) the title bar shows only the Ovra mark
  and More; the rail is hidden.

## Rail

- 48 px wide on `--s0`. Top: Files, Search, Git. Bottom: Terminal, Settings.
- Buttons 36 px, `--r-md`, icon 18 px in `--text-2`. Active: `--s2` fill,
  icon in `--accent`. Badges 16 px, `--t-caption`, tabular numbers.
- Tooltips after 600 ms, then instant while moving along the rail for 1 s;
  hidden on press.

## Side panels

- Header 40 px: title in `--t-headline`, at most one visible action. Other
  actions are muted until hover or focus, and below 220 px they go into a
  "…" menu. Collapse is the title-bar toggles (Mod+0, Alt+Mod+0); swap is in
  the palette, the View menu and the header's context menu.
- Resize from 160 px to 60% of the window (the main area keeps 320 px),
  collapse to a 48 px strip; the hit area of a split is 8 px; a 1 px `--separator`
  line on the side facing the main area marks the edge, and on hover, focus or drag it
  turns into a 2 px `--accent` line with a grip. The bottom dock (terminals, servers,
  problems) is `--s1` under the same hairline, so it never reads as part of the file.
- Rows: `--row` 32 px with a 4 px inset so the selection reads as a pill
  (`--r-xs`). `--sel-focus` while the list has focus, `--sel` otherwise.
  Indent 16 px per level; chevron 12 px rotating over `--dur-2`; 16 px
  monochrome file icon; git status as a 6 px dot (`--warn` changed, `--ok`
  new) with the letter in the tooltip and accessible name.

### Hover slot on rows

- A trailing slot of fixed width (2 x 24 px + 4 px) reserved in the layout,
  so showing it never moves or covers text. Badges sit to its left.
- Shown on hover, on `:focus-within` and always on the selected row; always
  shown under `@media (hover: none)`.
- At most two actions, most frequent first. Destructive actions never go
  here; they go last in the context menu.
- A row's next step for the person (Approve, Review) is a visible pill, never
  a hover action.

## Tabs

- 32 px. The selected tab takes `--s2` and joins the editor; others sit on
  `--s1`. No accent underline.
- Close shows on hover and on the selected tab. An unsaved file shows a 6 px
  dot in the close slot, which turns into the close button on hover.

## Buttons

| Kind                   | Look                                                                                           |
| ---------------------- | ---------------------------------------------------------------------------------------------- |
| Primary (one per area) | `--accent` fill, `--accent-ink`, `--ctl` (`--ctl-lg` in sheets and the composer), capsule, 500 |
| Quiet                  | transparent, `--text-2`, `--s3` on hover                                                       |
| Bordered quiet         | `--s3` fill; where a button must be found without hover (Deny)                                 |
| Disabled               | `--s3` fill with `--text-2`; no opacity fade                                                   |
| Danger                 | quiet with `--danger` text; filled only in a confirm sheet                                     |

- `cursor: default` on buttons, rows, tabs and segmented controls; the hand
  only on real links.
- `user-select: none` on chrome; transcript, code and command text stay
  selectable.

## Menus, context menus, tooltips

- Surface `--menu` with blur, `--card-shadow`, `--r-lg`, 6 px padding.
- Items 26 px, `--t-small`, highlight `--r-sm` in `--accent` with
  `--accent-ink`. Shortcuts right-aligned in `--text-2` as glyphs in Apple
  order (⌃⌥⇧⌘). One line per item; descriptions go in tooltips.
- Context menus: at most 3 groups and about 10 items. Open/navigate first,
  then change, then destructive last in `--danger` with an ellipsis when it
  asks first. Items that do not apply are hidden. Every item is also a
  palette command or a row key. Open with right click, Shift+F10, the Menu
  key, or a "…" button.
- Tooltips: `--t-small`, padding 4/8, `--r-sm`, after 600 ms; never cover
  the element's own label.

## Command palette ("Search or run")

- Opens from the title-bar field, Mod+K (Mod+P and Mod+Shift+P still work).
- 640 px wide, at 18 % of the window height, `--r-xl`, `--menu` surface.
  Rows 36 px at `--r-md`; the selected row is accent-filled.
- Empty query: Recent (5 files and commands), Suggested (context: "Review 2
  waiting", "Go to approvals", "New task"), Projects when none is open.
- Typing: Files, Commands, later Symbols, ranked together. A leading `>`
  limits to commands.
- Command row: category in `--text-2` (Git, Agents, View, Settings), title as
  verb + object in sentence case, shortcut cap at the trailing edge.
- Keywords match synonyms: "preferences" finds Settings, "proxy" and "token"
  find Network, "permissions" finds Rules, "history" finds Audit log.
- Entry: scale 0.98 to 1 with a fade over `--dur-4` `--spring`; exit: 120 ms
  fade.

## Agents column

One flex column; only the body scrolls, so nothing is ever clipped.

```
HEADER     40 px      Agents  2 of 3 ▬▬▭ · 1 queued                 [+]
ATTENTION  max 50 %   approval cards (own scroll), or nothing
BODY       flex 1     task list, or task view (header + transcript)
COMPOSER   56-200 px  the one input
FOOTER     20 px      Claude · Personal plan            (warn: API key)
```

- Header never wraps. "2 of 3" with three 14x4 px slot bars; "· 1 queued"
  when there is a queue (narrow: "+1"). New task is a "+" icon button.
- No setup card once Claude is ready: the footer line carries it, with the
  billing account always visible. A problem (signed out, not found, settings
  changed) appears as one card at the top of the column.

### Task rows

```
◐  Add offline queue to sync                         4m
   Editing src/sync.ts
   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━  activity line
●  Fix flaky login test                        [Approve]
   Wants to run npm test
✓  Update README                                [Review]
   3 files changed · $0.42
○  Add notes for the project                     Queued
   Starts when an agent is free
```

- Grid `22px 1fr auto`, two lines, min 56 px, `--r-md` 10 px padding, 2 px
  gap, no borders. Hover `--s2`; the open row `--s2` with a 500 title.
- Every state has a mark and a word: queued ring, working spinner, waiting
  `--warn` dot + Approve pill, review `--ok` check + Review pill, done check,
  stopped square + Resume, failed `--danger` circle + Retry.
- Order: needs you, working, queued, then the rest by recency.
- Close (x) has its own 24 px column on hover; never over a pill. Closing a
  task with open reviews asks first.
- "No sandbox" is one `--warn` line under the header, not a chip on every
  row.
- Plan and split: the task row opens into a tree (coordinator "Plan ✓ 3
  workers", then one 32 px row per worker) with a determinate "2 of 3 done"
  bar. Stop and Close act on the whole group.

### Task view

- Sticky header: back "Tasks", title `--t-title3`, status line `--t-small`
  (mark, state, elapsed, cost, account). One action by state: Stop while
  working (Interrupt in "…" and Esc Esc), Review when done with changes,
  Resume when stopped, Retry when failed. "Details" opens account, sandbox,
  tokens, branch and worktree; the no-sandbox line stays outside it.
- Transcript blocks 12 px apart. The person's message is a right-aligned
  `--s3` bubble; Claude's text is plain on the panel with Markdown.
- Step cards, collapsed by default, 28 px: status glyph, verb (past tense
  when done, present while running), object in mono, meta right-aligned
  (duration, +/- counts, exit code), chevron. The result merges into its call:
  one card per call. Consecutive reads and searches fold into "Read 6 files,
  searched 2 times". Edits, commands, failures and approvals always get their
  own card.
- Turn end is a thin `--text-3` divider ("1m 12s · $0.08"), not a row.
- Thinking row: three pulsing dots and a live verb in a reserved 28 px row.
- Auto-scroll follows only within 60 px of the bottom; scrolling up detaches
  it and shows a "Jump to latest" pill.
- Done with changes: an outcome card listing changed files with +/- counts,
  "Review changes" (primary) and quiet links.

## Approval card

```
● docs · Add notes for the project                         0:12
Run  npm install left-pad
[No sandbox] [Network: registry.npmjs.org]
"Installs the dependency the tests need."
› Details
                                     [Deny  N]  [Allow once  Y]
Deny with a message…
```

- `--s2`, `--r-lg`, padding 16/20, a 3 px `--warn` leading bar; the accent
  ring only for keyboard focus.
- Reading order: who (worker, task, project when it differs; waiting time),
  what (verb + mono pill; edits show a 6-line diff right here), risk chips
  (at most three: `--danger-soft` outside project, deletes, force push;
  `--warn-soft` no sandbox, network host, secrets), the agent's reason when it
  adds something, Details (full input with sentence-case labels and the rule
  that caused the ask).
- Buttons: Deny (bordered quiet), Allow once (primary, trailing). Keys Y and
  N; Shift+N for a message; Esc leaves it waiting. Return does not approve.
  Buttons stay disabled while arming, with a 2 px fill showing the wait.
- After an answer the card collapses (180 ms) into a receipt line in the
  transcript: "Allowed once: npm test".
- Grouped by task with shared facts once; identical requests from several
  workers stack into one card.

## Composer

```
Give Claude a task
┌──────────────────────────────────────────────────────────┐
│ Describe a change, or ask about the code                 │
│ [@ src/sync.ts ×]                                        │
│ [+]  One agent, here ▾      Personal ▾        [Start ⌘↵] │
└──────────────────────────────────────────────────────────┘
Works in this folder, without a sandbox. Asks before risky steps.
```

- One input for new tasks ("Start") and replies ("Send").
- `--s2`, `--r-lg`, focus ring only when focused. Autogrow 2-8 rows.
- Mode is a menu button; each option shows its consequence. The chosen
  mode's hint stays visible under the box and carries the sandbox fact.
- Account picker only with two or more accounts. A signed-out account
  disables Start with the reason ("Personal is signed out. Use Work").
- "Plan and split" without git shows "Needs git · Set up git" before Start.
- Context chips 24 px, `--r-sm`, mono paths. "/" opens a command popover.
- Mod+Enter sends; keys match on `KeyboardEvent.code`.

## Settings

- A window over the app, laid out like macOS System Settings (owner,
  2026-11-28): Mod+, or the rail gear; reopens on the last pane. 880 x 640
  at most, `--r-lg`, `--shadow`, over `--scrim`. Escape or a click outside
  closes it and focus goes back; Tab stays inside. Opening a file closes it.
- Left column on `--s1`: close button, a search field (focused on open;
  matches titles, lines and keywords; Return opens the first match), then the
  list in two groups. **This project**: Trust, Rules, Network, Keys,
  Servers, Schedules. **Ovra**: Editor, Agents, Setup check, Notifications,
  Phone, Remote machines, Keyboard, Accessibility, About. Each row has its
  icon on a 20 px tile of a `--tint-*` token; the chosen row is `--accent`.
- Fields that belong together sit on one inset card (`--s1`, `--r-md`)
  under a small heading. Opt-in power features fold under "Extra features".
- Pane: title (`--t-title2`, a noun), one line on what it does, one column
  of fields at most 480 px, one primary. Explanations behind one disclosure
  per section. Security warnings at the top of their pane, never folded.

## Empty states

One component (`EmptyState`): headline `--t-headline`, one line `--t-small`
in `--text-2`, one action, an optional key cap. Centred in the main area,
top-left in side panels. Loading names the thing ("Loading changes…"),
never an empty message. Copy for each panel is in [patterns.md](patterns.md).
