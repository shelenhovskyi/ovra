# Alignment audit (2026-11-26)

The owner's macOS screenshots showed controls off their line: the Get started
step numbers sat low, the keep-awake cup sat high in a grey box, Search's
icon buttons and Markdown task boxes sat high. Pixel measurements of those
screenshots and a WebKit run (the engine of the macOS app) gave four causes:

| Cause                                                                                                                                                                   | Where                                               | Fix                                                                 |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- | ------------------------------------------------------------------- |
| A component used classes its parent scopes (`.quiet`, `.icon-only`), so the browser styled the button: a 2 px outset border, a grey fill, the icon on the text baseline | `power/KeepAwakeButton.svelte`                      | The button styles itself                                            |
| An icon-only button left to block layout: the SVG is a letter on the text line and sits 1.5-3 px above the middle                                                       | Search's Replace and filter buttons, any future one | `theme.css`: `:where(button:has(> svg.icon:only-child))` centres it |
| Text in a box shorter than its line (20 px line in an 18 px circle): the digit sits low                                                                                 | `agents/FirstSteps.svelte`                          | `line-height: 1` in the mark                                        |
| A checkbox in a line of text sits on the baseline                                                                                                                       | Markdown task lists                                 | `theme.css`: `vertical-align: -0.2em`                               |

Run it again with WebKit (install once with
`pnpm --filter @ovra/e2e exec playwright install webkit`):

    just ux tests/e2e/ux/alignment.ts --browser webkit --seed small

It writes 2x dark screenshots of each screen and `findings.json`. On `main`
before the fixes it found 38 findings (browser-styled 8, icon-on-baseline 12,
icon-off-middle 12, text-taller-than-box 6); after, none.

Not found by the audit: the system font (SF Pro) puts lowercase text about
1 px below the middle of its line on macOS. That is how the font is drawn; no
CSS was changed for it. Linux WebKit uses another font, so a last check on a
Mac is still needed.
