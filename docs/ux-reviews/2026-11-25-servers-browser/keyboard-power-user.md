# Oleh, the keyboard power user: servers and agent browser

Build: 7cf1845, headless (`--fake-agent --fake-servers --fake-browser`), Chromium 158.0.8127.36, 1280x800.
Scope: managed dev servers dialog, agent browser pane (take-over, pick mode, navigation ask, pick chips). Fixtures: `small` (seeded), with a `.ovra/servers.json` defining `web` (`ready`), `api` (`fail`) and `worker` (unknown preset, exits 1).

## Summary

Oleh can open both features from the keyboard (Ctrl+Shift+S, Ctrl+Shift+B, both in the palette with their shortcuts, both work when the key is a Cyrillic letter), start, stop and restart servers, read logs and take over a page, and the take-over correctly swallows every IDE shortcut until Shift+Escape. The worst problems: picking an element cannot be done without a mouse at all, focus falls to `<body>` after Start, Stop, Allow and chip removal (and in the Servers dialog that makes Escape stop working), and Shift+Escape leaves a Shift key held down in the page. Overall it feels close; the gaps are all focus management and one missing keyboard path.

## Findings

### KEY-1 major: element picking needs a mouse; there is no keyboard way to pick

- **Area:** ui/src/browser
- **Steps:** 1. Start a "Browse the page" session, Ctrl+Shift+B, open `localhost:5173`, Allow. 2. Focus the tree, press Alt+Shift+P (pick mode turns on, focus moves to the page view, good). 3. Press ArrowUp, Enter, Space.
- **Expected:** with pick mode on, the element under some starting point (the page's focused element, or `body`) is highlighted; ArrowUp/ArrowDown walk parent/child, and Enter (Shift+Enter for more) picks it, like click and Shift+click.
- **Actual:** nothing is highlighted (`browser-hover-box` count 0), ArrowUp does nothing (`Picker.parent()` returns early without a pointer position), Enter and Space do nothing, no chip appears (`b05-pick-kbd.png`). The mode text tells him to "point at an element", which is a mouse task. Only Escape works.
- **Why it matters for Oleh:** picking is the reason the pane exists for him ("this button is wrong"); now it is the one step where he has to reach for the mouse.
- **Suggested fix:** in `PageView.onKeyDown`, while picking, start from a default point (centre of the frame, or the hit test of the page's `document.activeElement`) on the first arrow key, add Enter = pick and Shift+Enter = pick and stay, and say "Enter picks" in the mode text. Optionally Tab/Shift+Tab to step through siblings.

### KEY-2 major: Servers dialog loses focus after Start and Stop, then Escape no longer closes it

- **Area:** ui/src/servers
- **Steps:** 1. Ctrl+Shift+S (focus on the "Servers" heading, good). 2. Tab to "Start web", Enter. 3. Wait for "ready". 4. Press Escape.
- **Expected:** focus stays in the dialog (on the new server row's "Stop web", or on the next enabled button), and Escape closes the dialog.
- **Actual:** "Start web" becomes disabled and `document.activeElement` is `BODY`. Escape does nothing (dialog still visible) because the Escape and Tab-trap handler is on the dialog element and the key now starts at `<body>`. The same happens after Enter on "Stop web" (focus `BODY`). Ctrl+Shift+S still closes it, by accident of the global handler.
- **Why it matters for Oleh:** "Esc closes anything" is his first reflex; starting a server is the dialog's main action and right after it his reflex silently fails.
- **Suggested fix:** after `servers.start` resolves, focus the started server's "Stop" button (or the next enabled control); after Stop, focus that row's "Restart"/"Start". Also handle Escape on `window` while `opened` (capture phase), so a lost focus never makes the modal unclosable.

### KEY-3 major: Shift+Escape leaves Shift pressed in the page

- **Area:** ui/src/browser
- **Steps:** 1. Take over a page (Tab to "Take over", Enter). 2. Press Shift+Escape. 3. Look at the `browser.input` frames in `session.log`.
- **Expected:** giving the page back releases every key Ovra pressed in it.
- **Actual:** the last frame is `{"kind":"key_down","key":"Shift","code":"ShiftLeft","modifiers":8}`; no `key_up` follows, because `onKeyUp` ignores keys once `controlling` is false. The page now believes Shift is held (selection extends, shift-click semantics, uppercase in some editors) until the agent or the person presses and releases Shift there. The same can happen when focus leaves the stage mid-chord (blur).
- **Why it matters for Oleh:** he uses Shift+Escape every time; the next thing the agent does on the page may behave differently than what he saw.
- **Suggested fix:** track keys sent as down; on leaving control (Shift+Escape, Give back, blur) send `key_up` for each still-down key. Unit test in `BrowserPane.test.ts`.

### KEY-4 minor: Alt chords type text into the page while controlling

- **Area:** ui/src/browser
- **Steps:** 1. Take over. 2. Press Alt+Shift+P (or any Alt+letter).
- **Expected:** on Linux and Windows, Alt chords do not insert text (the page gets the key without `text`).
- **Actual:** frame `{"kind":"key_down","key":"P","code":"KeyP","text":"P","modifiers":9}`: a "P" is typed into the page's focused field. `typed()` only drops text for Ctrl and Meta.
- **Why it matters for Oleh:** he hits Alt+Shift+P out of habit to start picking while still in control; the page gets stray characters.
- **Suggested fix:** in `typed()`, return null when `altKey` is set and not on Apple (on macOS Option produces real characters, which `event.key` already carries). Also consider letting Alt+Shift+P through to pick mode, or mention in the mode text that pick needs Shift+Escape first.

### KEY-5 minor: focus falls to body after Allow/Deny and after removing a pick chip

- **Area:** ui/src/browser, ui/src/agents
- **Steps:** A. Type `localhost:5173` in Address, Enter; Shift+Tab twice to "Allow", Enter. B. Pick an element, Tab to the chip's "x" (Remove) in the agents panel, Enter.
- **Expected:** A: focus goes to the new page's tab or the page view (or the next ask if there is one). B: focus goes to the next chip, or the reply textarea.
- **Actual:** both leave `document.activeElement` as `BODY`; the next Tab starts from the top of the document.
- **Why it matters for Oleh:** every lost focus costs him a round of Tabs to find his place.
- **Suggested fix:** in `BrowserPane.answer`, after the ask disappears focus the next ask's Allow, else the active tab; in `AgentsPanel` chip removal, focus the neighbouring chip's button or `#agent-reply`.

### KEY-6 minor: the navigation ask is out of the keyboard path and has no shortcut

- **Area:** ui/src/browser
- **Steps:** Type an address, Enter.
- **Expected:** the ask "Let the agent open http://localhost:5173?" gets focus (or is announced with a shortcut such as Enter = Allow, Escape = Deny, or a palette command "Allow navigation").
- **Actual:** focus stays in the Address field; the ask is rendered above it, two Shift+Tab presses away. When the pane is hidden, only the header button's `aria-label` ("Browser, 1 waiting for you") and a badge tell him; Ctrl+Shift+B then focuses the heading, from which Allow is Tab, Tab... after the session select and Hide.
- **Why it matters for Oleh:** the agent is blocked until he answers; he should answer in one or two keys.
- **Suggested fix:** when an ask appears and the pane has focus, move focus to its Allow button; when Ctrl+Shift+B opens the pane with asks waiting, focus the first ask instead of the heading. Add palette commands for Allow/Deny of the oldest ask.

### KEY-7 minor: Escape does not hide the browser pane

- **Area:** ui/src/browser
- **Steps:** Ctrl+Shift+B (focus on "Browser" heading), press Escape; also with focus on the page view while not controlling.
- **Expected:** Escape hides the pane and returns focus (as the Servers dialog does), or at least returns focus to where it was.
- **Actual:** nothing happens; only Ctrl+Shift+B or Tab to "Hide" works.
- **Why it matters for Oleh:** "Esc closes anything" is in his persona; one key that silently does nothing.
- **Suggested fix:** Escape on the pane (not while controlling or picking, where it already has a meaning) hides it and restores focus like `BrowserButton.toggle`. If a docked pane should not close on Escape by design, move focus back to the editor/tree instead.

### KEY-8 minor: Ctrl+Shift+S does not close the Servers dialog while focus is inside it

- **Area:** ui/src/servers
- **Steps:** Ctrl+Shift+S, Tab to any button, Ctrl+Shift+S again.
- **Expected:** the palette calls the command a toggle ("Servers", `servers.toggle`), so the same chord closes it.
- **Actual:** the dialog stays open; `onDialogKeydown` stops propagation of every key, so the global keybinding never sees it. (It does close when focus has fallen to body, see KEY-2, which makes it look random.) Ctrl+Shift+B toggles the pane both ways, so the two features behave differently.
- **Why it matters for Oleh:** toggle chords are muscle memory; inconsistent toggles cost a keystroke and a moment of doubt.
- **Suggested fix:** in `onDialogKeydown`, close on the `servers.toggle` chord (match with `matchesShortcut`) before stopping propagation.

### KEY-9 minor: no palette commands for server actions; no "open in browser"

- **Area:** ui/src/servers
- **Steps:** Ctrl+Shift+P, type "start web", "restart", "stop".
- **Expected:** commands like "Servers: start web", "Servers: restart web", and (task S.F in the spec lists "open in browser") "Servers: open web in the agent browser".
- **Actual:** no results; the only server command is "Servers". Restarting `web` takes Ctrl+Shift+S, then Tab x8 in a list with three servers (Output and Restart of every newer server come first, since "Started" is newest-first), Enter, Escape. There is no "open in browser" action in the dialog at all, so getting the running server into the agent browser means typing `localhost:5173` by hand.
- **Why it matters for Oleh:** restart is the most frequent dev-server action; he expects it in the palette.
- **Suggested fix:** register one palette command per defined server for start/restart/stop while a project is open, and add the "Open in browser" button (plus command) from S.F.

### KEY-10 polish: header buttons do not show their shortcuts

- **Area:** ui/src/servers, ui/src/browser
- **Steps:** Hover or focus the "Servers" and "Browser" header buttons.
- **Expected:** a `title`/`aria-keyshortcuts` with Ctrl+Shift+S / Ctrl+Shift+B, as the Pick button does ("Pick an element (Alt+Shift+P)").
- **Actual:** none; the palette is the only place that shows them.
- **Suggested fix:** add `title` and `aria-keyshortcuts` using `formatShortcut`.

## Checked and fine (no collision, no leak)

- While controlling the page, Ctrl+Shift+S, Ctrl+Shift+P, Ctrl+Shift+B, Alt+Shift+P and Tab all go to the page as `browser.input` frames and do not open IDE UI; plain Escape goes to the page too, and the mode text says "Shift+Escape gives it back". After Shift+Escape focus stays on the page view and the mode text updates.
- Non-Latin layouts: Ctrl+Shift+S, Ctrl+Shift+B and Alt+Shift+P dispatched with Cyrillic `key` values and Latin `code` values all work.
- No collisions among registered shortcuts. Note for Windows/Linux users who switch layouts with Alt+Shift: Alt+Shift+P is a chord, so it does not switch layouts, but it is worth a check on Windows.

## What worked well

- Ctrl+Shift+S focuses the dialog heading; Tab is trapped inside the modal and wraps both ways; Escape (with focus inside) closes and returns focus to the tree where he came from.
- Ctrl+Shift+B focuses the pane heading and hiding it returns focus to the element that had it before (the reply textarea), even after he had moved into the page.
- Page tabs are a proper tablist with roving tabindex and Left/Right arrows; the page view is a focusable `application` with a visible focus ring.
- Alt+Shift+P works from anywhere (the tree) and moves focus to the page; Escape leaves pick mode.
- Start to ready took 26 ms with the fake; output log is focusable and scrolls with the keyboard; the expanded log stays open when the dialog is reopened.
- The browser button's accessible name counts waiting asks ("Browser, 1 waiting for you").
