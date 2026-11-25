# Ovra design guide

Status: draft, 2026-11-25. It refines the chosen "Quiet" style
(`docs/specs/quiet-style.md`) toward the feel of a well-made Mac app: minimal,
calm and obvious, with every useful feature still one step away. Quiet stays
the base; where this guide and Quiet differ, this guide wins and Quiet will be
updated to match.

It comes from four parallel research passes (macOS HIG, progressive
disclosure, agent chat and tasks, onboarding and empty states) checked
against the Quiet critiques in `docs/ux-reviews/2026-11-25-quiet/`. The full
research notes and the interactive mockup live outside the repository, in the
project's shared files (`research/apple-design/`).

## Pages

- [tokens.md](tokens.md): type, spacing, sizes, radii, colour additions,
  motion, icons.
- [components.md](components.md): the window frame, rail, panels, rows, tabs,
  buttons, menus, palette, the Agents column, approvals, composer, settings,
  empty states.
- [patterns.md](patterns.md): where a feature lives, first run, states and
  copy, keyboard.

## Principles

1. **The frame is the toolbar; the body is content.** The title bar holds
   three groups at most: navigation (leading), "Search or run" (centre),
   window actions (trailing). A panel header holds a title and one action.
   Layout commands (swap, collapse) belong to the frame, the palette and the
   menu bar, not to every header.
2. **Every command has a home; only frequent ones get pixels.** Every feature
   is in the command palette with its shortcut. Daily navigation goes on the
   rail or the title bar; per-item actions go on hover and in context menus;
   setup goes in Settings; logs and lists open as tabs. Two levels at most.
3. **Safety facts are said once, never hidden.** "No sandbox", trust
   warnings, approvals, the billing account and the API-key warning stay
   visible where the decision is made: a chip or one line in `--warn`, once
   per group, with the explanation behind "Details".
4. **One primary per area.** One accent-filled button per area; everything
   else is quiet. Accent marks focus, selection and that one action.
5. **Weight and tone before size.** Hierarchy comes from weight (600 titles)
   and tone (`--text-2`) first, size last. A headline is body size in bold.
6. **Status in place, not in alerts.** State lives on the row or header it
   belongs to, in specific words ("Editing src/sync.ts", not "Working").
   Only an approval interrupts, and only once per burst.
7. **Launch into work; ask at the point of need.** Restore the last project.
   The Claude check is the only up-front step; trust, accounts, git, servers
   and network wait until they matter. No tours: empty states and one-time
   hint lines teach.
8. **Motion confirms, never performs.** Short fades and one spring for things
   that appear or move. Hover, typing and list navigation get none. Reduced
   motion turns it off.
9. **Native behaviour beats native looks.** Arrow cursor on controls,
   unselectable chrome text, delayed tooltips, context menus, Cmd+, for
   Settings, standard shortcuts, the system font.

## Decisions recorded here

Taken as the reasonable default (reversible; see each page):

- System font stack (SF Pro / SF Mono on a Mac) instead of Geist. No bytes,
  no licence question, the most native choice.
- One icon family: Lucide (ISC), paths copied into `ui/src/shell/icons.ts`,
  with its licence notice.
- Reviews lives in the title bar only (count badge); the rail bottom holds
  Terminal and Settings.
- Settings is one main-area view (Mod+,), so headless mode behaves the same.
  Policies, Network and secrets and Servers definitions become its panes.
  This changes where they appear, not what they enforce.
- Audit log and Reviews open as main-area views, not modals; running dev
  servers become a tab in the bottom dock.
- Approval buttons follow macOS order: Deny, then Allow on the trailing
  side. Y and N answer; Return does not approve. Esc leaves it waiting.
- Reviews says "Keep changes" and "Discard…" (confirmed by "Discard the
  changes"), never Approve or Reject (owner, 2026-11-25). The protocol keeps
  `review.approve` and `review.reject`.
- "Always allow `npm test` here" (owner, 2026-11-25): a quiet action under
  the answers, with the same wait and checks as Allow once and no key. It
  writes the narrowest `auto` rule the core offers (`docs/specs/security.md`,
  "Always allow"), marked "From an approval" in Settings > Rules. Its code
  still needs a human security review.

Open for the owner:

- Window translucency (Tauri `windowEffects`) for the title bar and rail
  (postponed by the owner, 2026-11-25).
- Streaming replies need a CLI flag and a new protocol event; that lands as
  its own small task first.
