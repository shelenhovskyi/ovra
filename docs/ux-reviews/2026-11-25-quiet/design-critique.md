# Design critique (visual polish): Quiet

Build: 7311b1e, headless, Chromium 158.0.8127.36, 1440x900 and 1280x720,
dark and light (`prefers-color-scheme` emulated).
Scope: the "Quiet" style (`docs/specs/quiet-style.md`) compared with the chosen
mockup (`a-quiet.html`, renders `a-work.png`, `a-start.png`, `a-approval.png`)
on the start screen, workspace (read-only view, editor, Markdown), Git panel and
diff, Search, palette, Agents (composer, approvals, transcript, task list),
Reviews, More menu and its dialogs (Policies, Network, Servers, Audit log),
folded, swapped and terminal layouts. Fixtures: `small` (seeded).

Method: a single reviewer, no separate detector pass. Checklist: the impeccable
playbooks `critique`, `craft-floor`, `distill`. Measurements come from the live
DOM: computed font sizes, WCAG contrast of each visible text node against its
composited background, visible borders, button heights and radii, scroll
containers. Screenshots are in `/mnt/project-files/ovra/screenshots/quiet/`
and are named `critique-<theme>-<width>-<step>.png`. For example,
`critique-dark-1440-12-approval.png`. Step `16-project` is the Policies dialog.

## Summary

The frame now follows Quiet. Areas are told apart by tone with almost no
hairlines: the DOM audit found only the active-tab underline and CodeMirror's
dotted focus outline. Colours come from tokens: the only hex values outside
`theme.css` are in the server-log ANSI palette. Body text is 14 px and nothing
readable is under 12 px.

The gap to the mockup is in the content, not the frame:

- Code in the default read-only view, in diffs and in search results has no
  syntax colour.
- The Geist fonts are never loaded.
- In light theme, security warnings and the accent buttons drop below 4.5:1.
- At 1280x720 the Agents panel clips its own composer while an approval waits.
- Copy is still long, icons are a mix of glyphs and private SVGs, and radii and
  control heights wander.

Overall it feels calm and much closer to the mockup than before, but it reads
as "Quiet frame around the old panels" rather than one designed surface.

## Findings

### DES-1 major: Agents panel clips the composer and Send at 1280x720 while an approval waits

- **Area:** ui/src/shell (Shell.svelte `.panel` uses `overflow: clip`), ui/src/agents, ui/src/approvals
- **Steps:**
  1. Use a 1280x720 window.
  2. Open demo-app.
  3. Start a task "Fix the typo".
  4. Wait for "Run ls -la".
- **Expected:** The approval card, the Agents header and the reply box all fit.
  When they do not, the panel's own list scrolls (spec: "each panel scrolls on
  its own").
- **Actual:** The agents `<aside>` holds 697 px of content in 648 px and clips
  it. The textarea ends at y=691 and the Send button sits at y=699-731, below
  the panel bottom (692), so it cannot be seen or clicked. "Deny with
  message..." is also cut off. See `critique-dark-1280-composer-clipped.png`
  and `critique-dark-1280-12-approval.png`.

  The same kind of clipping happens at 1440x900 with two tasks: "Trusting it
  starts language servers, which run..." is cut through the middle of a line
  by the composer (`critique-dark-1440-task-list.png`).
- **Why it matters:** Replying to a waiting agent is the main action of the
  panel, and in a common laptop window it disappears without any sign.
- **Suggested fix:**
  - Make the agents column a three-row flex: approvals (`max-height` as now),
    the Agents list or transcript (`flex: 1; min-height: 0; overflow: auto`),
    and the composer (`flex: none`).
  - Nothing in the column may be taller than the column.
  - Add an e2e check at 1280x720 that Send is `toBeInViewport`.

### DES-2 major: Code has no syntax colour in the read-only view, diffs, search results and Markdown code blocks

- **Area:** ui/src/viewers (Lines.svelte, MarkdownView.svelte, PatchView.svelte), ui/src/git (GitDiff.svelte), ui/src/search, ui/src/editor/highlight (the highlighter to reuse)
- **Steps:**
  1. Open `src/lib.rs` from the tree (it opens read-only).
  2. Press Ctrl+E.
  3. Open the Git panel and click `main.rs`.
  4. Search for `fn`.
- **Expected:** The same colours as the mockup and the editor: `--kw`, `--ty`,
  `--str`, `--num`, `--com`, `--fn`, `--attr`.
- **Actual:**
  - Only the CodeMirror editor uses the syntax tokens (`editor/theme.ts`, the
    only file that reads `--kw` and friends).
  - The read-only view, which is how every file opens, is plain monochrome
    text with no line numbers (`critique-dark-1440-03-view-librs.png`).
  - Diffs are monochrome on red and green bands
    (`critique-dark-1440-07-git-diff.png`), and so are search snippets and
    Markdown code blocks (`critique-dark-1440-05-view-readme.png`).
  - Switching View/Edit moves the text by 34 px right and 62 px down: the
    gutter appears and an LSP banner pushes everything down
    (`critique-dark-1440-04-edit-librs.png`).
- **Why it matters:** The coloured code is the single strongest visual of the
  mockup (`a-work.png`). Without it the main area looks like a text dump, and
  reviewing agent diffs is slower.
- **Suggested fix:**
  - Run the existing highlight worker (`editor/highlight/`) for `Lines`,
    `GitDiff`, `PatchView`, Markdown `<pre>` and search snippets. They all emit
    the same `.tok-*` classes and share one stylesheet with `editor/theme.ts`.
  - Give the read-only view the editor's gutter so the two modes line up.

### DES-3 major: Light theme fails 4.5:1 for security warnings, accent buttons, accent links and success text

- **Area:** ui/src/shell/theme.css (light tokens), ui/src/tree (ignored names), ui/src/notify (toast action)
- **Steps:**
  1. Switch the system to light.
  2. Open demo-app.
  3. Start a task.
  4. Read the "No sandbox" lines, the Allow, Start and Trust buttons, the
     toast's "Review", and the tree.
- **Expected:** At least 4.5:1 for text (spec, Accessibility), and above all for
  security facts.
- **Actual** (measured on the composited background):

  | Text                                      | Ratio                 | Where                                                                                                                                  |
  | ----------------------------------------- | --------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
  | `--warn` #a96d0c on `--s1`                | 3.97                  | "No sandbox: the agent can read and change anything your user can.", "Needs approval", "(1 waiting)", "Agent started" in the Audit log |
  | `--warn` on `--warn-soft`                 | 3.69                  | the approval card's "No sandbox: it can reach anything you can.", the Network dialog warnings                                          |
  | `--accent-ink` #fff on `--accent` #0b80a8 | 4.499                 | every primary button: Allow, Open, Add rule, Add server, Start                                                                         |
  | `--accent` on `--s4`                      | 3.43                  | toast "Review"                                                                                                                         |
  | `--accent` on `--accent-soft`             | 3.86                  | palette match highlight                                                                                                                |
  | `--ok` #23885a on `--s1`                  | 4.09                  | git "U" and "M" letters                                                                                                                |
  | `--text-3` for gitignored names           | 2.83 light, 3.66 dark | `node_modules`, `target`, `build.log` (also italic)                                                                                    |
  | `--com` (code comments) on `--s2`         | 2.57 light, 2.76 dark | comments in code                                                                                                                       |

  See `critique-light-1440-12-approval.png` and
  `critique-light-1440-02-workspace-empty.png`.
- **Why it matters:** The spec keeps security facts visible "shorter, never
  hidden". At 3.7:1 they are visible but hard to read, in the theme people use
  in daylight.
- **Suggested fix:**
  - Light `--warn` `#8a5806`: 5.6:1 on `--s1`, 4.9:1 on `--warn-soft`.
  - Light `--accent` `#08769c`: 5.1:1 with white ink. Or keep `--accent` for
    fills and add `--accent-text` for links on raised surfaces.
  - Light `--ok` about `#1d7a50`.
  - Light `--com` about `#6f7884`, dark `--com` about `#7a8491`. Comments are
    read.
  - Gitignored names: `--text-2`, not italic.
  - Add a unit test that computes token pairs against WCAG.

### DES-4 minor: The Geist fonts are named but never shipped, so the app renders in whatever font the system has

- **Area:** ui/src/shell/theme.css, ui (assets)
- **Steps:**
  1. Open the app.
  2. Inspect the fonts in use (CDP `CSS.getPlatformFontsForNode`).
- **Expected:** Geist and Geist Mono, as in the mockup and the `--font` and
  `--mono` tokens.
- **Actual:** `document.fonts` has no faces. The text renders in Inter on this
  machine, and on the owner's Mac it falls to SF Pro and SF Mono, on Windows to
  Segoe UI and Consolas. Spacing, the weight-600 titles and the mono key caps
  all differ from the mockup per platform.
- **Why it matters:** This is the biggest single visual gap after syntax colour.
  Every size in the spec was tuned on Geist.
- **Suggested fix:**
  - Self-host `Geist` and `Geist Mono` as woff2 (400/500/600 and 400) with
    `@font-face` in `theme.css` and `font-display: swap`. Keep the system stack
    as fallback.
  - Geist is SIL OFL 1.1, which is not on the CLAUDE.md permissive list, so
    the owner must confirm it first.

### DES-5 minor: Disabled primary buttons look like pale accent buttons (2.2-2.8:1)

- **Area:** ui/src/agents (Start, Send), ui/src/git (CommitBox: Commit), ui/src/network (Save secret)
- **Steps:**
  1. Open demo-app. Do not type a task, do not stage anything.
  2. Look at Start, Commit, Send and Save secret, in both themes.
- **Expected:** Quiet's rule is one filled primary per area, so a primary that
  cannot be used should not look like one.
- **Actual:**
  - The disabled primary is the accent at reduced opacity. Measured
    2.24:1 (light) and 2.84:1 (dark).
  - In light it is the "pale accent button" that reads as broken
    (`critique-light-1440-07-git-diff.png`: Commit and Start). In dark it is a
    muddy teal (`critique-dark-1440-06-git.png`).
  - The same panel then shows Trust project in full accent next to the faded
    Start, so the strongest button on screen is the risky one.
- **Suggested fix:** A shared disabled style: `background: var(--s3)` and
  `color: var(--text-2)`, no opacity. Or keep the button enabled and say what
  is missing when it is pressed (as approvals already do).

### DES-6 minor: More than one filled primary per area

- **Area:** ui/src/agents/setup (ProjectTrust), ui/src/network, ui/src/git
- **Steps:**
  1. Open demo-app and look at the Agents panel.
  2. Open More > Network and secrets.
- **Expected:** One primary per area (spec, Rules).
- **Actual:**
  - Agents: "Trust project" (filled accent) and "Start" (filled) are on screen
    together.
  - Network dialog: "Allow" and "Save secret" are both filled.
  - Git: a filled Commit sits next to quiet Stage all and Unstage all while
    nothing is staged.

  See `critique-dark-1440-02-workspace-empty.png` and
  `critique-dark-1440-17-network.png`.
- **Why it matters:** "Trust project" lets language servers run project code.
  As the loudest button it invites the click the spec wants to be deliberate.
- **Suggested fix:**
  - Trust project: a quiet button with a leading shield icon, kept visible.
  - Network: Allow (hosts) is the primary and Save secret is quiet, or the
    secret form opens behind "Add secret".
  - Commit: quiet until something is staged.

### DES-7 minor: Agents header wraps "1 of 3" and "New task" onto two lines

- **Area:** ui/src/agents (AgentsPanel.svelte header), ui/src/shell (Shell.svelte `.agents-panel > .controls`, `padding-right: 72px`)
- **Steps:**
  1. Keep the Agents panel at its default 320 px width.
  2. Start a task.
- **Expected:** One row, as in the mockup: "Agents 2 of 3", the slot bars, and
  the buttons on the right.
- **Actual:** "1 of / 3" and "New / task" wrap. The swap and collapse buttons
  are positioned absolutely with a fixed 72 px reservation, so the header has
  about 150 px for four things. This happens at both widths, both themes, and
  both sides (`critique-dark-1440-12-approval.png`,
  `critique-dark-1440-21-swapped.png`).
- **Suggested fix:**
  - `white-space: nowrap` on the count and on buttons.
  - Make "New task" an icon button (plus, tooltip "New task", Mod+N).
  - Put swap and collapse in the header's flex row instead of absolute
    positioning.

### DES-8 minor: After an approval is answered from the keyboard, the empty Approvals band covers the Agents header

- **Area:** ui/src/approvals (Approvals.svelte `.idle .body`, absolutely positioned on `--s4`)
- **Steps:**
  1. Start a task.
  2. Press Alt+Shift+A, then y, then ArrowDown, then y.
- **Expected:** "Nothing waits" is announced, and the Agents header (name,
  slots, "Tasks", Stop) stays visible.
- **Actual:**
  - A raised "Approvals / Nothing waits for your approval." band with a focus
    ring sits over the Agents header and hides "Tasks", Stop and the slots.
  - The Agents panel's swap and collapse buttons draw on top of the band.

  See `critique-dark-1440-14-transcript.png` and
  `critique-dark-1280-14-transcript.png`.
- **Suggested fix:** Keep the band in flow (it pushes the header down) or show
  the confirmation as a short status inside the Agents header. Never overlay
  the header.

### DES-9 minor: Status bar: "Language servers off" floats in the middle, and the mockup's left group is missing

- **Area:** ui/src/shell (Shell.svelte footer `.status { margin-left: auto }`), ui/src/lsp (LanguageServers.svelte `.lsp { margin-left: auto }`)
- **Steps:**
  1. Open any file in Edit mode.
  2. Look at the footer.
- **Expected:** Mockup: branch and "3 changes" on the left; "2 working",
  "Connected" and the account on the right.
- **Actual:** Two `margin-left: auto` items split the free space, so "Language
  servers off" sits at x≈605-735, centred under the editor
  (`critique-dark-1440-04-edit-librs.png`). There is no left group.
- **Suggested fix:**
  - Remove `margin-left: auto` from `.lsp` and wrap the right-hand items in
    one group.
  - The branch and changes counter can come later. Until then, a
    left-aligned empty footer is fine.

### DES-10 minor: Start screen is far from `a-start.png`

- **Area:** ui/src/tree (ProjectList.svelte), ui/src/shell (start layout, Rail)
- **Steps:**
  1. Load the app with no project open (1440x900 and 1280x720).
- **Expected:** Mockup:
  - an "Open a project" title (about 24 px/600) and one short line;
  - Recent rows with an initial tile, the name, a mono path and a relative
    time;
  - one "Open folder" primary with its key caps;
  - a content column about 560 px wide;
  - Claude status on the right;
  - the rail showing its icons.
- **Actual:**
  - The title is "Projects" at 14 px. Recent rows are text only, with no time.
  - The path input stretches to 966 px (1440) and 806 px (1280).
  - The rail is an empty 52 px strip.
  - The right column says only "Open a project to start an agent."
  - The Claude status appears only after a project is open.

  See `critique-dark-1440-01-start.png`.
- **Suggested fix:**
  - `max-width: 560px` on the start column, a 24 px title, and rows with a
    32 px `--s3` initial tile plus `last_opened_ms` as a relative time.
  - Show the Claude status card in the right column on the start screen.
  - Hide the rail entirely when no project is open, or show it disabled.

### DES-11 minor: Icons mix Unicode glyphs, the shared icon set and private SVGs with different strokes

- **Area:** ui/src/tree (FileTree twisty `▸ ▾`, dir dot `•`), ui/src/git (branch "main ▾", "▸ Recent commits", FileSection SVGs), ui/src/editor (Tabs `×` and `●` via `content:`), ui/src/viewers (PatchView `▸ ▾`), ui/src/agents/setup (ProjectTrust `⚠`), ui/src/search (`…` options), ui/src/terminal (`+` text, private SVGs), dialogs (native `<summary>` triangles)
- **Steps:**
  1. Compare the tree chevrons, Git row buttons, tab close, dialog disclosures
     and rail icons in `critique-dark-1440-06-git.png` and
     `critique-dark-1440-17-network.png`.
- **Expected:** Spec: every icon is drawn in `ui/src/shell/icons.ts` with one
  stroke, at 18 px or 15 px.
- **Actual:**
  - Filled 12 px triangles sit next to 1.6-stroke outline icons.
  - Ten components draw their own `<svg>`: Terminals, AgentsPanel,
    Notifications, FileTree, PolicyPanel, AuditLog, NetworkPanel, FileSection,
    Approvals, ServersPanel, Inbox.
  - FileSection uses a 16-unit viewBox with stroke 1.5 at 16 px, so it renders
    at 1.5 px. Rail icons render at 1.2 px and small icons at 1.0 px, which
    makes the Git row's open, stage and discard icons visibly heavier.
- **Suggested fix:**
  - Add `chevron-right`, `chevron-down`, `close`, `dot`, `warning`, `more`,
    `plus` and `external` to `icons.ts` and use `<Icon>` everywhere.
  - Style `summary` with the chevron icon (`list-style: none`) in one shared
    rule.

### DES-12 minor: Browser defaults leak through: dotted editor outline, native selects and checkboxes, scrollbars, selection

- **Area:** ui/src/editor (theme.ts), ui/src/shell (theme.css)
- **Steps:**
  1. In light theme, press Ctrl+E on a file and click in the editor.
  2. Open Policies (Action select) and the Audit log (Show select,
     "Refused only").
  3. Open Git ("Amend last commit").
- **Expected:** Craft floor, "browser surfaces": focus, caret, scrollbars,
  selection and form controls come from the palette.
- **Actual:**
  - CodeMirror's default `.cm-focused { outline: 1px dotted }` draws a dotted
    hairline across the top of the editor
    (`critique-light-1440-editor-dotted-outline.png`).
  - Selects and checkboxes are the browser's grey defaults: only Inbox sets
    `accent-color`.
  - Nothing sets `scrollbar-color` or `::selection`.
- **Suggested fix:**
  - In `editor/theme.ts`, add `"&.cm-focused": { outline: "none" }`. The panel
    already shows focus.
  - In `theme.css`:
    - `:root { accent-color: var(--accent); scrollbar-color: var(--s4) transparent; }`
    - `::selection { background: var(--accent-soft); }`
    - `select { appearance: none; background: var(--s3) url(chevron) ...; }`

### DES-13 minor: Radii and control heights wander off the two tokens

- **Area:** ui/src/shell/theme.css plus the call sites listed
- **Steps:**
  1. List the `border-radius` values in `ui/src/**/*.svelte`.
  2. Measure the button heights on the workspace.
- **Expected:** Spec and mockup: a small set of radii (6 and 8 px tokens, about
  12 px for cards and dialogs) and two control heights.
- **Actual:**
  - Radii in use: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12 and 14 px next to
    `--radius` and `--radius-sm`. Examples: Rail 9 px, TitleBar and MoreMenu
    7 px, Approvals 12/10/5 px, dialogs 14 px, Palette 12/5 px, Search 9/3 px.
  - Buttons measured at 24, 25, 26, 28, 30, 32 and 38 px tall on one screen.
    Trust project is 24 px and Start is 32 px in the same panel.
  - Icon-only buttons inherit the browser's 13.33 px button font.
- **Suggested fix:**
  - Add `--radius-lg: 12px` (cards, dialogs, palette) and `--radius-xs: 4px`
    (key caps, chips), and replace the literals.
  - Two control heights: 28 px (inline and quiet) and 32 px (primary and
    inputs).
  - Add `:where(button, input, select, textarea) { font: inherit; }` in
    `theme.css`.

### DES-14 minor: Too many words for "Few words"

- **Area:** ui/src/git, ui/src/search, ui/src/lsp (editor banner), ui/src/agents, ui/src/shell (MoreMenu), ui/src/network, ui/src/servers, ui/src/policy
- **Steps:**
  1. Read the Git panel, the Search panel, the editor in Edit mode, the
     Agents composer, the More menu and the three dialogs.
- **Expected:** Labels of one to three words. A sentence only where a person
  decides. Explanations behind "Details".
- **Actual (examples):**
  - Git: "Stage changes to commit them, or amend the last commit." and
    "Push and pull: git push, git pull in a terminal."
  - Search: "Down arrow: results. Escape: back."
  - The editor banner: "No fake-lsp here: language servers run the project's
    code, so they start only once you trust the project (Trust project, in the
    Agents panel)." This is two lines on `--accent-soft` across the editor.
  - Composer: "Works in this folder. Asks before risky steps."
  - Every More item has a second descriptive line.
  - Network and secrets has six explanatory sentences before the first field.

  See `critique-dark-1440-06-git.png`, `critique-dark-1440-04-edit-librs.png`,
  `critique-dark-1440-17-network.png` and `critique-dark-1440-16a-more-menu.png`.
- **Suggested fix:**
  - Git: drop both sentences. Show "git push / pull" only as a terminal
    shortcut.
  - Editor banner: a footer chip "Language servers off · Trust…".
  - More: names only, descriptions as tooltips.
  - Dialogs: one line and a "Details" disclosure. Keep the no-sandbox and
    keychain warnings visible, but shorten them to one line each.

### DES-15 minor: Panel headers do not line up and show five always-on buttons

- **Area:** ui/src/shell (Shell.svelte `.head`, `.own-head`), ui/src/tree (FileTree toolbar)
- **Steps:**
  1. Switch between Files, Search and Git.
  2. Compare the header title positions and controls.
- **Expected:** One header height and baseline for every side panel. Secondary
  buttons quiet until hover (mockup: two faint icons).
- **Actual:**
  - Title positions: "Files" at x66/y67, "Search" and "Git" at x70/y63,
    "Agents" at y68. The title jumps 4 px when switching views.
  - Files shows five icon buttons (collapse all, reveal, filter, swap,
    collapse) at 0.6 opacity, so the "chrome on demand" is always half there.
  - The swap and collapse pair is absolutely positioned over the tree
    toolbar.

  See `critique-dark-1440-03-view-librs.png` and
  `critique-dark-1440-06-git.png`.
- **Suggested fix:**
  - One header component (40 px, 18 px left padding, title at 14/600) used by
    Files, Search, Git and Agents. Panel-specific buttons go in a slot.
  - Idle controls at `opacity: 0` (or `--text-3`) until the header is hovered
    or focused, and keep them visible with forced colours.
  - Move "collapse all" and "reveal" into the panel's overflow.

### DES-16 minor: Dialog sizes are inconsistent; Reviews and Audit fill the window for a line or three

- **Area:** ui/src/inbox (Inbox.svelte), ui/src/audit (AuditLog.svelte), ui/src/policy, ui/src/network, ui/src/servers
- **Steps:**
  1. Open Reviews with nothing to review.
  2. Open More > Audit log, Project policies, Network and secrets, Servers.
- **Expected:** One dialog scale, sized to the content, as in the mockup's
  approval card (about 480 px).
- **Actual:**
  - Reviews is 1378x840 for "No agent changes wait for review."
  - Audit log is almost full-window too.
  - Policies, Network and Servers are 760 px wide. Servers is 260 px tall.

  See `critique-dark-1440-15-reviews.png` and `critique-dark-1440-19-audit.png`.
- **Suggested fix:**
  - Widths 560 (forms) and 760 (lists).
  - Full-window only when Reviews shows a diff.
  - Empty states sized to their content.

### DES-17 minor: Server log ANSI colours are hard-coded for a dark background

- **Area:** ui/src/servers (ansi.ts)
- **Steps:**
  1. Read `ui/src/servers/ansi.ts`. It holds 16 hex values from a dark
     palette, including `#ffffff`.
- **Expected:** Tokens only. In light theme, bright white and bright yellow
  must stay readable.
- **Actual:** These are the only hex colours left in components. I did not run
  a dev server with coloured output, so the light-theme result is inferred, not
  seen.
- **Suggested fix:** Map the ANSI colours to tokens: `--danger`, `--ok`,
  `--warn`, `--kw`, `--attr`, `--ty`, `--text`, `--text-2`. Bright white maps
  to `--text`.

### DES-18 polish: Tabs use an accent underline (a line) instead of tone

- **Area:** ui/src/editor (Tabs.svelte), ui/src/terminal (Terminals.svelte tabs)
- **Steps:**
  1. Open two files.
- **Expected:** Mockup: the active tab is an `--s3` pill with `--text`. The
  dirty or agent state is a small accent dot. No line.
- **Actual:**
  - The active tab has a 2 px `--accent` bottom border. Its radius is
    `6px 6px 0 0` with no fill, so the radius is invisible.
  - The terminal tabs copy this pattern.
  - Close and dirty are `×` and `●` glyphs.
- **Suggested fix:**
  - Active tab: `background: var(--s3); border-radius: var(--radius-sm)`, no
    border.
  - Dirty state: the accent dot icon.
  - Close: the close icon, shown on hover and focus.

### DES-19 polish: The approval card reads in a different order from the mockup

- **Area:** ui/src/approvals
- **Steps:**
  1. Start a task.
  2. Look at "Run ls -la" and "Edit README.md".
- **Expected:** Mockup (`a-approval.png`), in this order:
  - who asks;
  - the question as the title, with the command in a mono pill;
  - risk chips (Network: host);
  - "Details";
  - Allow once, Always here, Deny;
  - a footer with sandbox and billing.
- **Actual:**
  - The warn box "No sandbox: it can reach anything you can." comes before the
    title.
  - The command in the title is sans ("Run ls -la").
  - Raw field names in lower case ("command", "description", "old string",
    "new string") sit in an always-open table.

  See `critique-dark-1440-13-approval-edit.png`.
- **Suggested fix:**
  - Title first, with the command in `--mono` on `--s3`.
  - "No sandbox" as a warn chip directly under the title. It stays visible.
  - Field labels in sentence case ("Old text", "New text"). The description
    row goes behind "Details".

### DES-20 polish: Tree, palette and task rows miss the mockup's quieter markers

- **Area:** ui/src/tree, ui/src/palette, ui/src/agents
- **Steps:**
  1. Expand `src`.
  2. Press Alt+P and type "main".
  3. Start two tasks and look at the task list.
- **Expected:** Mockup tree: SVG chevrons, file-type icons, git state as small
  coloured dots. Palette rows: file name in `--text` and folder in `--text-2`.
  Task rows: the full title, with the badge on the same line.
- **Actual:**
  - Tree: git state is a mono letter (M, U) and folders get a "•".
  - Palette: the whole path is in one tone.
  - Task list: the title is truncated ("Fix the typo in the rea...") by a wide
    "Approve" badge, and "No sandbox" sits alone on a second line, aligned
    right.

  See `critique-dark-1440-10-palette-query.png` and
  `critique-dark-1440-task-list.png`.
- **Suggested fix:**
  - Tree: dots with an accessible name; keep the letter in the tooltip.
  - Palette: split name and folder.
  - Task rows: the title takes `flex: 1`, the badges are `flex: none` and size
    to their content, and the "No sandbox" chip goes under the status line on
    the left.

### DES-21 polish: Folding and swapping leave the rail detached from its panel

- **Area:** ui/src/shell (Shell.svelte strips, Rail.svelte)
- **Steps:**
  1. Collapse Files and Agents.
  2. Unfold both, then press "Move Files to the other side".
- **Expected:** A folded panel is clearly the rail's panel. After a swap, the
  rail's selection still points at its panel.
- **Actual:**
  - Folded: the strip shows the same folder icon 48 px to the right of the
    rail's folder icon, so two identical icons sit side by side
    (`critique-dark-1440-20-folded.png`).
  - Swapped: the rail stays on the far left next to Agents, while its pressed
    Files button controls a panel at the far right
    (`critique-dark-1440-21-swapped.png`).
- **Suggested fix:**
  - When the rail's panel folds, drop its strip: the rail already unfolds it.
    Keep the strip only for Agents.
  - When swapped, move the rail to the right edge with the side panel, or show
    a small chevron on the pressed rail item pointing at the panel.

### DES-22 polish: Rail tooltips stay up after a click and cover panel headings

- **Area:** ui/src/shell (Rail.svelte `.tip`)
- **Steps:**
  1. Click the Git or Files icon in the rail.
- **Expected:** The tooltip shows on hover after a short delay, or on keyboard
  focus, and hides on click.
- **Actual:**
  - The "Git" tooltip sits over the "Commit message" label
    (`critique-dark-1440-06-git.png`).
  - The "Files" tooltip covers the Files title
    (`critique-dark-1440-12-approval.png`).
  - The "Terminal" tooltip covers "Ctrl+Enter sends"
    (`critique-dark-1440-22-terminal.png`).
- **Suggested fix:** Show on `:hover` (300 ms delay) and `:focus-visible` only,
  hide on pointerdown, and offset 8 px.

### DES-23 polish: The terminal dock is the same tone as the editor

- **Area:** ui/src/terminal
- **Steps:**
  1. Open a diff.
  2. Press the rail's Terminal button.
- **Expected:** Tone, not lines. The mockup's terminal is a separate `--s1`
  card or bar under the editor.
- **Actual:** The terminal sits on `--s2` directly under the diff, so nothing
  separates the two areas except the tab row
  (`critique-dark-1440-22-terminal.png`).
- **Suggested fix:** Give the dock `--s1` (or a 6 px `--s0` gap and a
  `--radius-lg` card), and give its header the same height as the editor tabs.

## What worked well

- **Tokens:** All UI colour comes from `theme.css` except the ANSI palette.
  The DOM audit found no stray hex, rgba or named colours in styles.
- **Tone, not lines:** Between areas the audit found no visible 1 px borders.
  Rail and title bar on `--s0`, panels on `--s1` and editor on `--s2` read as
  in the mockup.
- **Type:** Body 14 px and secondary 13 px. Nothing readable is under 12 px;
  only key caps and badges use 12 px. Titles use weight 600.
- **The frame:** The title bar, 52 px rail, panel widths (260 and 320) and
  28 px footer match the mockup's proportions. The window never scrolled in
  any step.
- **Focused approvals:** A focused approval dims the rest of the window with
  `--scrim`, close to `a-approval.png`, and the security facts (no sandbox)
  stay visible (`critique-dark-1440-focused-approval.png`).
- **Overlays:** The palette and the More menu use `--s4` with `--card-shadow`
  and look like the mockup's raised surfaces. The focus ring is visible
  everywhere.
