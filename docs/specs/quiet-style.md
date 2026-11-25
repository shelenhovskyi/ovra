# Quiet: Ovra's visual style

Status: the owner chose this direction on 2026-11-25 from five mockups. The
reference mockup and its renders are kept outside the repository; this page is
what the code follows.

The design guide in `docs/design/` refines this style (tokens, components,
where features live); where the two differ, the guide wins.

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
  `--font` for text, `--mono` for code and paths, both the system's own fonts
  (`--font`: -apple-system, BlinkMacSystemFont, system-ui, "Segoe UI";
  `--mono`: ui-monospace, "SF Mono", Menlo, Consolas); no bundled fonts.
  Weight 600 for titles, 500 for active items. No text under 12 px.
- **Shape.** Radii `--radius-xs` (chips, key caps), `--radius-sm` (controls),
  `--radius` (panels), `--radius-lg` (cards, dialogs); control heights
  `--control-h-sm` (quiet and inline) and `--control-h` (primary, inputs).
- **Code** shows the syntax colours wherever it appears: editor, viewer,
  diffs, search results, Markdown code blocks.
- **Few words.** Labels are 1-3 words; a sentence only where a person must
  decide something. Explanations sit behind a "Details" disclosure.
  Security facts (approvals, trust, no-sandbox, billing account, risky
  settings) stay visible: shorter, never hidden.
- **One primary action per area**, filled with `--accent` and `--accent-ink`.
  Other buttons are quiet: transparent, `--text-2`, `--s3` on hover.
- **Icons.** Lucide (ISC) as inline SVG, 18 px (16 px small), 1.75 stroke,
  `currentColor`, copied into `ui/src/shell/icons.ts`. Icon-only buttons have
  an accessible name and a tooltip.
- **Chrome on demand.** Panel headers show their secondary buttons (filters)
  muted, at full strength on hover or focus. Folding and swapping panels
  belong to the title bar and the palette, not to panel headers.
- **Accessibility stays.** Visible focus ring (`--accent`), targets at least
  24 px, 4.5:1 contrast for text (`--text-3` is for icons and large text only),
  forced-colours mode keeps working, motion respects
  `prefers-reduced-motion`.

## Layout

- **Title bar.** No separate app header. In the desktop app the window uses an
  overlay title bar next to the traffic lights; empty space drags the window.
  In headless mode the same bar is the top row of the page. Three groups: the
  sidebar toggle (Mod+0), project switcher and branch button; a search field
  (Mod+K opens the palette); Reviews with its count, the Agents toggle
  (Alt+Mod+0) and More. A "1 waiting" chip shows while Agents is folded and a
  request waits. The start screen keeps only the Ovra mark and More.
- **Icon rail** on the edge of its panel's side (the far left unless the
  panels are swapped): Files, Search, Git; Terminal at the bottom (Settings
  joins it with the settings view). Hidden on the start screen.
- **Side panels** (the rail's panel, and Agents) resize by dragging a handle
  from 160 px to 60% of the window, leaving the main area 320 px (arrow keys
  on a focused handle, double-click resets), fold (the rail's panel to
  nothing, Agents to a 48 px strip that shows how many agents work and how
  many requests wait), and swap sides. The layout is remembered. Folded
  Agents unfolds when a new request arrives.
- **The window never scrolls**; each panel scrolls on its own.
- **Agents** is a compact list: one row per task with its state ("2 of 3"
  with slot bars in the header); a row opens its transcript.
