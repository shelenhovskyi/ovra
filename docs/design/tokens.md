# Tokens

All values live in `ui/src/shell/theme.css`. Components use tokens only; no
hex values, no stray sizes. Existing Quiet names stay; this page adds to them.

## Type

The Quiet sizes stay (14 body, 13 secondary, 12 for key caps and badges).
Roles get names so every area uses the same pairs.

| Token          | Size / line | Weight | Use                                                  |
| -------------- | ----------- | ------ | ---------------------------------------------------- |
| `--t-caption`  | 12 / 16     | 500    | key caps, badges, status bar, section labels         |
| `--t-small`    | 13 / 18     | 400    | secondary text, menu items, tooltips, chrome buttons |
| `--t-body`     | 14 / 20     | 400    | rows, transcript, approvals, forms                   |
| `--t-headline` | 14 / 20     | 600    | panel titles, task titles                            |
| `--t-title3`   | 15 / 20     | 600    | card titles ("Run npm test")                         |
| `--t-title2`   | 17 / 22     | 600    | sheet and settings pane titles                       |
| `--t-title1`   | 22 / 28     | 600    | start screen heading only                            |

- `body` line height is an exact `20px`, so rows land on the 4 px grid.
- `letter-spacing: -0.01em` from 14 px, `-0.02em` from 17 px (WebKit's
  `system-ui` applies SF's own tracking; this covers other faces).
- `font-variant-numeric: tabular-nums` on every number that updates or lines
  up: "2 of 3", badges, line numbers, diff stats, durations, costs, times.
- Section labels (Staged, Changes, Recent) are `--t-caption` at 600 in
  `--text-2`, sentence case. Never uppercase.

Fonts:

```css
--font: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI Variable Text",
  "Segoe UI", Roboto, "Noto Sans", sans-serif;
--mono: ui-monospace, "SF Mono", Menlo, "Cascadia Mono", Consolas,
  "DejaVu Sans Mono", monospace;
```

## Spacing and sizes

```css
--sp-1: 4px; --sp-2: 8px; --sp-3: 12px; --sp-4: 16px; --sp-5: 20px; --sp-6: 24px;
--ctl-sm: 24px;  /* header icon buttons, chips, small segmented */
--ctl:    28px;  /* buttons, fields, selects, list rows */
--ctl-lg: 32px;  /* title-bar search, composer button, sheet buttons */
--row:    28px;  /* tree, git, search rows (macOS medium sidebar row) */
--icon-sm: 16px; --icon: 18px;
```

- Panel content padding 16 px; dialogs and settings panes 20 px.
- 8 px between related controls, 16-20 px between groups.
- Every target is at least 24 px (WCAG 2.5.8).

## Radii (concentric)

```css
--r-xs: 4px;    /* kbd, badges, inline code, row selection */
--r-sm: 6px;    /* 24 px controls, menu item highlight, chips */
--r-md: 8px;    /* 28-32 px controls, search field, rail buttons, tabs */
--r-lg: 12px;   /* cards: approval, composer, toasts, menus */
--r-xl: 16px;   /* palette, sheets */
--r-pill: 999px;
```

Rule: inner radius = outer radius minus inset, never below 4. A menu of 12
with 6 px padding gets 6 px highlights; the palette of 16 with an 8 px inset
gets 8 px rows. These replace `--radius`, `--radius-sm` and the stray values.

## Colour additions

| Token              | Dark                    | Light                   | Use                                                                       |
| ------------------ | ----------------------- | ----------------------- | ------------------------------------------------------------------------- |
| `--sel`            | `#262a32`               | `#dfe3e8`               | selection while the list is not focused                                   |
| `--sel-focus`      | `rgba(95,191,220,.22)`  | `rgba(8,118,156,.14)`   | selection while the list is focused                                       |
| `--separator`      | `rgba(255,255,255,.08)` | `rgba(0,0,0,.10)`       | control edges; area lines only in increased contrast                      |
| `--menu`           | `rgba(43,48,57,.86)`    | `rgba(255,255,255,.88)` | menus, palette, tooltips over `backdrop-filter: blur(24px) saturate(1.6)` |
| `--selection-text` | `rgba(95,191,220,.32)`  | `rgba(8,118,156,.22)`   | `::selection`                                                             |
| `--danger-soft`    | `rgba(236,123,123,.13)` | `rgba(201,60,60,.10)`   | danger chips, failed cards                                                |

Light-theme fixes (warnings and primaries were under 4.5:1):

```diff
- --warn: #a96d0c;   + --warn: #8a5806;
- --accent: #0b80a8; + --accent: #08769c;
- --ok: #23885a;     + --ok: #1d7a50;
- --com: #98a0aa;    + --com: #6f7884;   /* dark: #59636f -> #7a8491 */
- --s2: #fbfcfd;     + --s2: #ffffff;    /* editor = text background */
- --s0: #eef1f4;     + --s0: #e8ebef;    /* chrome one clear step below panels */
- --s4: #dde1e7;     + --s4: #ffffff;    /* light menus are white with a shadow */
```

Media queries:

- `prefers-contrast: more`: `--text-2` moves toward `--text`, `--text-3`
  takes `--text-2`, and `--separator` lines appear between areas.
- `prefers-reduced-transparency: reduce` and forced colours: `--menu` is the
  opaque `--s4`, no blur.

## Motion

```css
--dur-1: 100ms;  /* press colour */
--dur-2: 160ms;  /* tooltip and menu fade, chevron rotate */
--dur-3: 240ms;  /* panel fold, tab insert, toast */
--dur-4: 320ms;  /* palette and sheet entry */
--ease-out: cubic-bezier(0.22, 1, 0.36, 1);
--ease-in-out: cubic-bezier(0.65, 0, 0.35, 1);
--spring: linear(0, .049, .157, .285, .411, .526, .624, .706, .772, .825, .867,
  .899, .924, .943, .957, .968, .976, .983, .987, .99, .993, .995, .997, 1);
```

- Entries use `--ease-out` or `--spring`; exits take 70 % of the entry time,
  opacity only. Menus open at once and fade out over `--dur-2`.
- `prefers-reduced-motion: reduce`: all durations 0 except opacity fades of
  100 ms or less; loops (activity line, thinking dots, cursor) stand still.
- Loops in the Agents column: activity line 1.8 s, thinking dots 1.4 s with
  160 ms stagger, cursor blink 1.1 s `steps(2)`, live step text throttled to
  600 ms with a 120 ms crossfade.

## Icons

- **Lucide** (ISC). Copy the needed path strings into `ui/src/shell/icons.ts`
  (no runtime dependency) and add the ISC notice to the third-party notices.
  One family only: no Unicode glyphs (▸ ✕ ⇄) and no private drawings.
- 24 px grid, `stroke-width: 1.75`, round caps and joins, `currentColor`.
  16 px beside text, 18 px alone (rail, title bar). Gap to a label: 6 px at
  16, 8 px at 18.
- Mapping: Files `folder`, Search `search`, Git `git-branch`, Terminal
  `square-terminal`, Reviews `inbox`, Settings `settings`, More `ellipsis`,
  sidebar toggles `panel-left` / `panel-right`, swap `arrow-left-right`, new
  task `plus`, approval `shield-check`, no sandbox `shield-alert`, network
  `globe`, servers `server`, audit `scroll-text`, browser `app-window`,
  disclosure `chevron-right`.
- Meaning comes from the status dot or the word, never from icon tint alone.
  The one coloured icon is the warning shield in `--warn`.
