# Quiet: Ovra's visual style

Status: the owner chose this direction on 2026-11-25 from five mockups. The
reference mockup and its renders are kept outside the repository; this page is
what the code follows.

## Why

The owner's first real use: the UI looked primitive and overloaded with small
text that nobody wants to read. Quiet answers with fewer words, bigger text,
tone instead of lines, and chrome that appears only when needed.

## Rules

- **Tokens only.** Colours come from `ui/src/shell/theme.css` (`--s0`..`--s4`,
  `--text`, `--text-2`, `--text-3`, `--accent`, `--warn`, `--ok`, `--danger`,
  their `-soft` fills, syntax `--kw` and friends). No hex values in components.
  Dark and light themes follow the system through the same tokens.
- **Tone, not lines.** Areas are told apart by surface tone: rail and title bar
  on `--s0`, side panels on `--s1`, the editor on `--s2`, hover and inputs on
  `--s3`, menus and tooltips on `--s4` with `--card-shadow`. No 1 px borders
  between areas; a border only where a control needs an edge to be found.
- **Type.** 14 px body, 13 px secondary, 12 px only for key caps and badges.
  `--font` for text, `--mono` for code and paths. Weight 600 for titles, 500
  for active items. No text under 12 px.
- **Few words.** Labels are 1-3 words; a sentence only where a person must
  decide something. Explanations sit behind a "Details" disclosure.
  Security facts (approvals, trust, no-sandbox, billing account, risky
  settings) stay visible: shorter, never hidden.
- **One primary action per area**, filled with `--accent` and `--accent-ink`.
  Other buttons are quiet: transparent, `--text-2`, `--s3` on hover.
- **Icons.** Inline SVG, 18 px (15 px small), 1.6 stroke, `currentColor`,
  drawn in `ui/src/shell/icons.ts`. Icon-only buttons have an accessible name
  and a tooltip.
- **Chrome on demand.** Panel headers show their secondary buttons (swap,
  collapse, filters) muted, at full strength on hover or focus.
- **Accessibility stays.** Visible focus ring (`--accent`), targets at least
  24 px, 4.5:1 contrast for text (`--text-3` is for icons and large text only),
  forced-colours mode keeps working, motion respects
  `prefers-reduced-motion`.

## Layout

- **Title bar.** No separate app header. In the desktop app the window uses an
  overlay title bar: project switcher, branch, a search field (Mod+K opens the
  palette), Reviews and More sit next to the traffic lights; empty space drags
  the window. In headless mode the same bar is the top row of the page.
- **Icon rail** on the far left: Files, Search, Git, Terminal; Reviews and
  settings at the bottom; badges for counts.
- **Side panels** (the rail's panel, and Agents) resize by dragging a handle
  between 180 and 460 px (arrow keys on a focused handle, double-click resets),
  collapse to a 48 px strip with one icon (the Agents strip shows how many
  agents work), and swap sides. The layout is remembered.
- **The window never scrolls**; each panel scrolls on its own.
- **Agents** is a compact list: one row per task with its state ("2 of 3"
  with slot bars in the header); a row opens its transcript.
