# Patterns

## Where a feature lives

Answer in order and stop at the first yes.

1. **Can the agent do more than the person expects, or is money spent?**
   (no sandbox, trust, a rule that skips approval, billing account, API key,
   network without proxy) Always visible where the decision is made, short:
   a chip or one `--warn` line, once per group. The explanation goes behind
   "Details". Never only on hover, in a menu or in Settings.
2. **Needed to finish the current step?** (Allow, Start, Send, Keep, Commit)
   Always visible; one primary per area.
3. **Daily navigation?** Rail or title bar. Budgets: rail 3 top + 2 bottom;
   title bar 3 groups. A new item replaces one.
4. **Acts on one item, daily or weekly?** Hover slot (2 at most) + context
   menu + row key.
5. **Acts on one item, rarely?** Context menu + palette.
6. **Set once, seldom touched?** Settings, with a link where the need comes
   up ("Always allow…" on an approval opens Rules).
7. **Something to read: a log, a history, a long list?** A main-area tab or
   view, never a modal.
8. **A rare global action?** Palette only (and the macOS menu bar).

Always: every command is in the palette with its shortcut; two levels at
most; one route per secondary thing; modals only for short decisions
(discard?, close with open reviews?, trust a project with hooks?).

### Current features, placed

| Feature                                        | Lives in                                                                                                                   |
| ---------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Files, Search, Git                             | Rail (top)                                                                                                                 |
| Terminal, Settings                             | Rail (bottom)                                                                                                              |
| Reviews                                        | Title bar with count; opens as a main-area view grouped by task                                                            |
| Go to file, all commands                       | Title-bar field "Search or run"                                                                                            |
| Branch switch                                  | Title-bar branch button; also the Git panel                                                                                |
| Stage / unstage, open file                     | Hover slot on Git rows; Discard last in the context menu                                                                   |
| Amend                                          | The Commit button's menu                                                                                                   |
| Blame, LSP actions, open in external app       | Editor, tree and tab context menus + palette                                                                               |
| Diff bar                                       | "Open file" only; stage hunk on hunk hover; close with the tab                                                             |
| Stop / Interrupt                               | Task header while running only; Interrupt in "…"                                                                           |
| No sandbox                                     | One line under the Agents header and a chip on each approval group                                                         |
| Billing account                                | Composer footer line; `--warn` when an API key bills                                                                       |
| Trust                                          | No card without project settings; a card with warnings when there are hooks or broad rules; full lists in Settings > Trust |
| Claude path, accounts                          | Settings > Claude; the account picker only with 2+ accounts                                                                |
| Policies, Network, secrets, server definitions | Settings panes                                                                                                             |
| Running dev servers and logs                   | A "Servers" tab in the bottom dock; a status-bar dot while one runs                                                        |
| Audit log                                      | Main-area tab; "Show in audit log" in task menus                                                                           |
| Agent browser                                  | Opens as a tab when an agent opens a page; waiting navigations are approvals                                               |
| More menu                                      | Settings…, Audit log, Agent browser, Keyboard shortcuts (names only)                                                       |
| Notifications, terminal screen-reader mode     | Settings panes + palette                                                                                                   |
| Connection state                               | Status bar only when disconnected                                                                                          |
| Toasts                                         | One per kind with a count                                                                                                  |

## Menu bar (desktop app, macOS)

Built from the command registry: each item runs a command id, so the menu,
palette, More and shortcuts share one source. Disabled items stay visible.

| Menu         | Items                                                                                                                  |
| ------------ | ---------------------------------------------------------------------------------------------------------------------- |
| Ovra         | About Ovra · Settings… ⌘, · Hide · Quit                                                                                |
| File         | New Task ⌘N · Open Folder… ⌘O · Open Recent · Save ⌘S · Close Tab ⌘W                                                   |
| Edit         | Undo, Redo, Cut, Copy, Paste, Select All · Find ⌘F · Find in Files ⇧⌘F                                                 |
| View         | Search or Run… ⌘K · Files ⇧⌘E · Git ⌃⇧G · Show/Hide Sidebar ⌘0 · Show/Hide Agents ⌥⌘0 · Swap Side Panels · Terminal ⌃` |
| Agents       | New Task · Go to Approvals ⇧⌘A · Reviews ⇧⌘R · Agent Browser ⇧⌘B · Servers ⌥⌘S · Audit Log ⌥⌘L                         |
| Window, Help | system items · Keyboard Shortcuts ⌘/                                                                                   |

Shortcut fixes: Servers moves off ⇧⌘S (Save As) to ⌥⌘S; Git moves off ⇧⌘G
(Find Previous) to ⌃⇧G; add ⌘K and ⌘,.

## First run

One path, three moments: Claude is ready; Claude did something I allowed; I
kept the result. Everything else is just in time.

| Step           | What the person sees                                                                                                                                                                               | Primary                          |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| Start screen   | "Open a project" · "Give Claude a task in a folder. You approve each risky step and review every change." · recent projects (initial, `~` path, "2h ago") · drop zone · Claude status on the right | Open folder… ⌘O                  |
| Claude check   | Ready: one line "Claude is ready · Uses your Claude plan". Problems: one card each (see below). Re-checks when the window regains focus                                                            | Copy command (for problems)      |
| Project open   | Editor empty state with the project name; the Agents column is the checklist: Claude ✓, Project settings (only if any), Give Claude a task                                                         | Start ⌘↵                         |
| Trust          | Only when the project brings `.claude/` settings: warning lines stay on a quiet card, "Review and trust…" opens the list, Trust is at its bottom                                                   | (quiet; Start stays the primary) |
| First approval | One-time line: "Allow lets Claude do this one step. Deny stops it and Claude tries another way."                                                                                                   | Allow once                       |
| First result   | "Done · Changed README.md · Show changes"; split tasks get a Review pill                                                                                                                           | Review changes                   |

- Returning people never see the start screen: the last project reopens.
- The checklist shows only while a project has no tasks, has no title or
  count, and folds into the composer footer when the first task starts. It
  comes back only as a problem card.
- Hints are shown once, then live under Help > How Ovra works.
- Start is disabled with a reason before a known failure (signed out, no git
  for Plan and split), instead of failing after the press.

### Claude check copy

| State          | Heading                   | Line                                                                                               | Action                                           |
| -------------- | ------------------------- | -------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| Ready, plan    | Claude is ready           | Uses your Claude plan.                                                                             | … menu: Use another account…, Set path…, Details |
| Ready, API key | Claude is ready           | Billed to the API key in ANTHROPIC_API_KEY, at API prices, not your Claude plan. (`--warn`)        | How to use my plan                               |
| Not installed  | Install Claude Code       | Ovra runs Claude Code from your computer. Install it, then sign in once in a terminal.             | Copy command                                     |
| Signed out     | Sign in to Claude         | Run `claude auth login` in a terminal, then come back. Signing in to the Claude app doesn't count. | Copy command                                     |
| Wrong path     | Claude isn't at this path | Nothing at `~/.local/bin/claude-work`.                                                             | Find automatically                               |
| Too old        | Update Claude Code        | Ovra needs 2.1 or newer; you have 2.0.9.                                                           | Copy command                                     |
| Broken         | Claude didn't respond     | First line of the error; the rest behind Details.                                                  | Try again                                        |

Ovra never installs Claude or signs in for the person (ADR 0006); it only
shows commands to copy.

## Empty states

| Panel / case              | Headline             | Line                                                                             | Action                                                  |
| ------------------------- | -------------------- | -------------------------------------------------------------------------------- | ------------------------------------------------------- |
| Editor, no file           | {project name}       | Open a file, or give Claude a task.                                              | New task ⌘N · Find a file ⌘P · Search ⇧⌘F · Terminal ⌃` |
| Files, empty folder       | This folder is empty | Ask Claude to set up a project here, or add files.                               | Give Claude a task                                      |
| Files, changed only, none | No changed files     |                                                                                  | Show all files                                          |
| Search, idle              | Search in files      | Searches every file except ignored ones.                                         | (field focused)                                         |
| Search, no results        | No results for "{q}" | Check the spelling, or turn off Match case.                                      | Clear options (if any are on)                           |
| Git, not a repository     | Not a git repository | Git keeps a history, so you can review and undo changes.                         | Set up git                                              |
| Git, clean                | No changes           | Changes you or Claude make appear here.                                          |                                                         |
| Terminal, none            | No terminals         | Opens a shell in this project's folder.                                          | New terminal                                            |
| Agents, no tasks          | (the checklist)      |                                                                                  | Start                                                   |
| Approvals, none           | (takes no space)     |                                                                                  |                                                         |
| Reviews, none             | Nothing to review    | Changes from Plan and split wait here until you keep or discard them.            |                                                         |
| Servers, none             | No dev servers       | Add the command that starts your app, so agents can open it.                     | Add server                                              |
| Network, none             | No extra hosts       | Sandboxed agents reach only listed hosts; other requests are refused and logged. | Add host                                                |
| Rules, none               | No rules yet         | Anything your Claude settings don't allow is asked about.                        | Add rule                                                |
| Audit, none               | Nothing recorded yet | Agent starts, approvals and tool calls appear here.                              |                                                         |
| Browser, none             | No pages open        | Agents open pages here; you can open one too.                                    | Open address…                                           |

## States and copy

| State              | Row line 2                   | Header / action |
| ------------------ | ---------------------------- | --------------- |
| Queued             | Starts when an agent is free | Queued          |
| Working            | Editing src/sync.ts          | Stop            |
| Waiting for you    | Wants to run npm test        | Approve pill    |
| Waiting for review | 3 files changed · $0.42      | Review pill     |
| Done               | Done · $0.18                 | Close           |
| Stopped            | Stopped                      | Resume          |
| Failed             | Stopped: usage limit reached | Retry           |

Writing rules: sentence case, verbs on buttons, labels of 1-3 words, no
"please", "successfully", "oops" or exclamation marks, no "we". Name things
as the person named them (task title, worker label); never show an id.
Buttons say the result ("Allow once", "Keep changes"), never OK or Yes.
Errors say what happened, what to do and give one button; raw text goes
behind Details.

## Surfacing an approval

| Where the person is        | What they see                                                                |
| -------------------------- | ---------------------------------------------------------------------------- |
| Agents column visible      | The card in the attention area and the row's Approve pill; focus never moves |
| Agents column folded       | A `--warn` count badge on the strip                                          |
| In a modal or another view | A 32 px banner "1 approval waits · Show"; never hidden behind it             |
| Window in the background   | One system notification per burst ("3 requests need approval")               |
| Several projects           | Requests are labelled with their project                                     |

No in-app toast when the card is already visible.

## Keyboard

- Every shortcut matches `KeyboardEvent.code`, so non-Latin layouts work.
- Tab moves between areas (title bar, rail, panel, main, Agents); arrows move
  within one (roving tabindex). F6 and Shift+F6 cycle areas.
- Lists and trees use a row highlight, not a ring; fields and buttons use the
  focus ring.
- Task list: Up/Down, Enter opens, Delete closes (asks with open reviews),
  `a` jumps to the row's approval. Approval: Y, N, Shift+N, Esc.
