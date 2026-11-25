# Oleh, the keyboard power user: 2026-11-25-lsp

Build: 9ad2719, headless, Playwright's bundled Chromium, 1280x800.
Scope: language servers (docs/specs/lsp.md): diagnostics, hover, go to
definition (F12), find references (Shift+F12), rename (F2), completion,
formatting (Shift+Alt+F), the status-bar item with its Stop button.
Fixtures: throwaway projects with `main.rs`, `other.rs`, `lib.rs`, `notes.md`
and `app.ts`, which are the ones that get `fake-lsp`. I also used `small` to
check the palette.

## Summary

Oleh can get diagnostics, jump to a definition with F12, open the
language-server panel from the footer, close it with Escape and stop a server,
all without the mouse. The shortcuts also work on a Cyrillic layout (Alt+P,
Ctrl+E and Shift+Alt+F are matched by physical key). The worst gaps: a
keyboard user cannot read a diagnostic or a hover at all (no lint keymap, no
gutter, no keyboard hover). None of the LSP actions are in the command
palette, so F12, Shift+F12, F2 and Shift+Alt+F are hidden. Stopping a server
leaves focus on `<body>`. A failed formatting request stacks error bars that
Escape does not close, and they throw page errors once the server is stopped.
The core works, but every step off the happy path loses Oleh's focus or
leaves a key that silently does nothing.

## Findings

### KEY-1 major: diagnostics cannot be read from the keyboard

- **Area:** ui/src/editor (buffers.svelte.ts keymap, lsp client extensions)
- **Steps:** 1. Trust the project, Alt+P `main.rs`, Enter, Ctrl+E. 2. Wait
  for the red squiggle under `TODO` (it shows within 10 ms). 3. Put the cursor
  inside `TODO` with the arrow keys. 4. Press F8, Ctrl+Shift+M, Alt+F8 and
  Ctrl+. in turn.
- **Expected:** a way to read "fake: TODO found" without the mouse: the lint
  panel (Ctrl+Shift+M, `@codemirror/lint`'s `lintKeymap`), next/previous
  diagnostic (F8), or a tooltip while the cursor is on the range. A gutter
  marker should also show which lines have problems.
- **Actual:** none of these keys do anything (0 panels, 0 tooltips after
  each). The page has no lint gutter (0 `.cm-gutter-lint` elements), so the
  only sign is a thin underline (`04-cursor-on-diag.png`). The message shows
  only when the mouse hovers the squiggle.
- **Why it matters for Oleh:** diagnostics are the main thing a language
  server gives him. Seeing that something is wrong but not what wrong means
  he has to reach for the mouse on every error.
- **Suggested fix:** add `lintKeymap` (Ctrl+Shift+M opens the panel, F8 goes
  to the next diagnostic) and `lintGutter()` to the buffer extensions when a
  server is attached. Register "Go to next problem" and "Show problems" in
  the palette.

### KEY-2 major: hover (type and docs) has no keyboard trigger

- **Area:** ui/src/editor/lsp (clients.ts)
- **Steps:** 1. In `main.rs`, put the cursor on `main`. 2. Try Ctrl+K Ctrl+I
  (VS Code), Ctrl+Shift+Space and Ctrl+Space.
- **Expected:** one key shows the hover at the cursor ("fake hover"), as Vim's
  `K` or VS Code's Ctrl+K Ctrl+I do. Escape closes it.
- **Actual:** no tooltip appears for any of these keys. Moving the mouse onto
  the word shows "fake hover" right away (`02-mouse-hover-diag.png`), so the
  data is there and only the trigger is missing.
- **Why it matters for Oleh:** types and docs are what he checks most often
  while reading code.
- **Suggested fix:** add a command, "Show hover" (for example Ctrl+K Ctrl+I,
  or Mod+I), that asks `textDocument/hover` at the cursor and shows it as a
  cursor tooltip that Escape closes. Put it in the palette.

### KEY-3 major: no LSP action is in the command palette, so the shortcuts are hidden

- **Area:** ui/src/editor (commands.ts), ui/src/lsp
- **Steps:** 1. Open `main.rs` with a running server. 2. Press Ctrl+Shift+P
  and type `definition`, `references`, `rename`, `language`, `lsp`,
  `diagnostic`, `problems` and `hover` in turn.
- **Expected:** "Go to definition (F12)", "Find references (Shift+F12)",
  "Rename symbol (F2)", "Format document (Shift+Alt+F)", "Trigger
  completion (Ctrl+Space)" and "Language servers: show / stop" in the list,
  each with its shortcut.
- **Actual:** none of them are in the palette. `rename` matches only "Toggle
  screen reader mode in terminals" and `lsp` only "Close project"
  (`11-palette-lsp.png`). The keys come from `@codemirror/lsp-client`'s keymaps
  and are written down nowhere in the UI.
- **Why it matters for Oleh:** the persona expects the palette to list every
  command with its shortcut. He found F12 and F2 only because he guessed the
  VS Code defaults. The footer item is also the only way into the
  language-server panel.
- **Suggested fix:** register palette commands that call `jumpToDefinition`,
  `findReferences` (through `LanguageClients.references`), `renameSymbol`,
  `formatDocument` and `startCompletion`, with their keys, enabled only when
  the editor has a server. Add a `lsp.show` command that opens the footer
  panel.

### KEY-4 major: stopping a server drops focus to `<body>`

- **Area:** ui/src/lsp (LanguageServers.svelte)
- **Steps:** 1. Tab to the footer item `fake-lsp` (7 Tabs after Escape in the
  editor) and press Enter: focus goes to the "Language servers" heading.
  2. Tab once to "Stop", press Enter.
- **Expected:** the server stops, and focus goes back to the editor or to the
  footer item (as Escape does). If the item disappears, focus goes to the
  editor.
- **Actual:** the server stops, the footer item and panel disappear (no
  servers left, so `visible` is false), and `document.activeElement` is
  `BODY` (`FOCUS[after Stop] BODY`). The next Escape or Tab starts from the
  top of the page.
- **Why it matters for Oleh:** "focus predictable after every action" is his
  first check. Here he has to press Ctrl+E to find his place again.
- **Suggested fix:** in `stop()`, before the item can unmount, move focus to
  the footer item if it stays, otherwise to the editor (`Ctrl+E` target).
  Add a unit test next to the "closes on Escape and returns focus" test in
  `LanguageServers.test.ts`.

### KEY-5 minor: the server panel stays open when focus leaves it, and Escape outside it does nothing

- **Area:** ui/src/lsp (LanguageServers.svelte)
- **Steps:** 1. Open the panel with Enter on the footer item. 2. Press Tab
  twice: Stop, then `BODY` (focus falls off the end of the page), then
  "Reviews" and "Audit log". 3. Press Escape. 4. Press Ctrl+E, then Escape in
  the editor.
- **Expected:** the panel closes when focus leaves it, or Escape closes it
  from anywhere (as other popovers do). The Tab order should not fall into
  `<body>` after the last button.
- **Actual:** the panel stays open through all of it, drawn over the bottom
  of the editor (`04-editor-with-panel.png`). Escape works only while focus
  is inside the panel.
- **Why it matters for Oleh:** "Esc closes anything". An open popover he
  cannot see the focus in is noise he has to go back to and close.
- **Suggested fix:** close on `focusout` when the new target is outside
  `.lsp`. Optionally also close on a window-level Escape while it is open.

### KEY-6 major: a failed LSP request leaves error bars that Escape cannot close; they stack, and throw page errors after Stop

- **Area:** ui/src/editor/lsp (clients.ts, the library's `reportError`
  dialog), ui/src/editor (buffers.svelte.ts `setLanguageServer`)
- **Steps:** 1. Open `main.rs` in the editor with `fake-lsp` running (it does
  not format). 2. Press Shift+Alt+F three times. 3. Press Escape. 4. Stop the
  server from the footer, then Alt+P `other.rs`, Enter, Ctrl+E.
- **Expected:** one short notice that the server does not format, which
  Escape or the next keystroke dismisses. Better: Shift+Alt+F does not ask a
  server that did not advertise `documentFormattingProvider`. Stopping a
  server and switching files raises no errors.
- **Actual:** each press adds another unstyled "Formatting request failed:
  not in the fake [OK] x" bar above the code (three stacked in
  `01-stacked.png`). Escape does not close them. After Stop and the file
  switch, `session.log` has `pageerror Field is not present in this state`
  three times, once per bar. In one of those runs `other.rs` then showed no
  squiggle even though the new server had sent its diagnostics (0 instead of
  1). Without the error bars (no Shift+Alt+F first), the same Stop and switch
  shows no page error. I reproduced this 3 out of 3 times.
- **Why it matters for Oleh:** he presses format often out of habit. Bars
  that pile up, take Tab stops and need the mouse or Tab+Enter to close, plus
  a broken editor state after Stop, cost real time.
- **Suggested fix:** check the server's capabilities before sending
  formatting (and rename and references), and show "fake-lsp cannot format"
  through the existing `tabs.notice` instead of the library dialog. If the
  dialog stays, make Escape close it and keep only one. Clear it (or keep the
  field) in `setLanguageServer([] ...)` when a server is dropped, and add a
  test for "stop the server while an error dialog is open, then switch
  files".

### KEY-7 minor: F2, Shift+F12 and Ctrl+Space do nothing, without a word, when the server lacks the feature

- **Area:** ui/src/editor/lsp (clients.ts)
- **Steps:** 1. With `fake-lsp` running (hover and definition only), put the
  cursor on `add` and press Shift+F12, then F2, then type ` ad` and press
  Ctrl+Space.
- **Expected:** a short note, such as "fake-lsp does not support rename".
- **Actual:** nothing happens: no request is sent (the frame log has only
  `textDocument/definition`), no panel, no notice
  (`08-after-shiftF12.png`, `09-after-F2.png`, `10-completion.png`).
  Formatting is the opposite case: it sends the request anyway and errors
  (KEY-6).
- **Why it matters for Oleh:** "notices every key that silently does
  nothing". He cannot tell "this server cannot rename" from "Ovra did not
  hear the key".
- **Suggested fix:** when a server-backed command is not supported, show the
  server name and the missing feature in `tabs.notice`. Note for the
  coordinator: because `fake-lsp` lacks references, rename and completion, I
  could not check the references panel (its Escape and focus return), the
  rename prompt or the completion list from the keyboard. Adding
  `referencesProvider`, `renameProvider` and `completionProvider` to the fake
  would let the next round and the e2e test cover them.

### KEY-8 minor: an untrusted project's panel has nothing to act on

- **Area:** ui/src/lsp (LanguageServers.svelte), ui/src/editor
- **Steps:** 1. Open a new (untrusted) project, Alt+P `main.rs`, Enter,
  Ctrl+E. 2. The footer says "Language servers off". Tab to it and press
  Enter.
- **Expected:** as the spec says ("the editor says why and links to trust"),
  a button or link in the panel or the editor that goes to "Review and
  trust…", reachable with one Tab.
- **Actual:** the panel holds only the heading and the sentence "Language
  servers run this project's code, so they start only in trusted projects."
  It has 0 focusable controls (`02-untrusted-panel.png`). The editor shows no
  notice. Oleh has to find the trust card in the right column himself (4 Tabs
  from the tree).
- **Why it matters for Oleh:** a dead-end popover is one more thing to close.
  One key to trust would make it a two-keystroke fix.
- **Suggested fix:** add a "Review and trust…" button to the panel that
  focuses the trust card's button, or opens its review.

### KEY-9 minor: a stopped server cannot be restarted where it was stopped

- **Area:** ui/src/lsp, ui/src/editor/lsp (clients.ts)
- **Steps:** 1. Stop `fake-lsp` from the footer while `main.rs` is shown.
  2. Keep typing in `main.rs`.
- **Expected:** a "Start" or "Restart" key or button (in the panel or the
  palette), the usual "Restart language server" command.
- **Actual:** the footer item disappears and no server starts while the same
  file stays shown. A new one starts only after switching to another file and
  back. This works: `lsp.start` is sent and the squiggles come back.
- **Why it matters for Oleh:** restarting a stuck rust-analyzer is a routine
  step. Here it is a hidden file-switch trick.
- **Suggested fix:** add a palette command "Restart language server" that
  stops the server and connects the shown buffer again. Optionally keep a
  stopped row with "Start" in the panel.

### KEY-10 polish: language features only exist in edit mode

- **Area:** ui/src/editor
- **Steps:** 1. Alt+P `main.rs`, Enter. The file opens in the viewer.
- **Expected:** at least diagnostics, or a hint that Ctrl+E turns on the
  language server.
- **Actual:** no server starts until Ctrl+E (no `lsp.start` frame). F12 in
  the viewer does nothing.
- **Why it matters for Oleh:** one extra key per file, and it took a failed
  first script to find out why there were no diagnostics.
- **Suggested fix:** show "Ctrl+E to edit with fake-lsp" in the viewer's
  header for files whose language has a running or known server, or start the
  server in the viewer too (read-only).

## What worked well

- Non-Latin layout: with a Cyrillic layout (non-Latin `key` values, code
  `KeyP`/`KeyE`/`KeyF`), Alt+P opened Go to file, Ctrl+E focused the editor
  and Shift+Alt+F sent `textDocument/formatting`. F-keys and Escape are the
  same on every layout.
- F12 jumps to the definition and keeps focus in the editor (cursor moved
  from `add` to 1:0, as the fake answers).
- Diagnostics appear almost at once after Ctrl+E (6 ms after the editor took
  focus) and clear as soon as the server is stopped.
- Footer panel: Enter opens it with focus on its heading, Stop is one Tab
  away, the Stop button's name says which server ("Stop fake-lsp"), Escape
  inside the panel closes it and returns focus to the footer item, and
  `aria-expanded` reflects the state.
- The panel says "Sandboxed: bubblewrap, Landlock, seccomp" next to each
  server, which a careful user wants to see.
- Trusting the project is reachable by keyboard (4 Tabs to "Review and
  trust…", 2 more to "Trust project"). After trusting, focus returns to the
  file tree. While the review opens, focus briefly sits on `<body>`; that
  belongs to an earlier round's trust findings, not to this scope.
