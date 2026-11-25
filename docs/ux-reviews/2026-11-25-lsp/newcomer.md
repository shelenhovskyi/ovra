# Nadia, the newcomer: 2026-11-25-lsp

Build: 9ad2719, headless (fake agent, `fake-lsp` for rust), Playwright Chromium, 1280x800.
Scope: language servers (docs/specs/lsp.md): trust, diagnostics, hover, go to
definition (F12), find references (Shift+F12), rename (F2), the status-bar item
with Stop and the sandbox note, missing-server and untrusted hints.
Fixtures: `small`, `empty`.

## Summary

Nadia can get language-server help, but only after finding two things nobody
tells her about: clicking "Edit" (the default file view has no language
features and no hint) and trusting the project through a card that talks only
about Claude settings. Once both are done, diagnostics and hover work at once
and the footer item and its panel are clear, but the untrusted and
missing-server states only show up in a small footer label, and the panel never
offers the fix (trust) it describes. Rename, references and go to definition
can't be found with the mouse: no context menu, no palette commands,
Ctrl+click does nothing.

## Findings

### NEW-1 major: Opening a file shows no language features and no hint; they only start in Edit mode

- **Area:** ui/src/editor (with ui/src/lsp for the hint)
- **Steps:** 1. Open `demo-app`, trust it. 2. Click `src/main.rs` in the tree.
  3. Hover `main`, look at the footer.
- **Expected:** Clicking a file shows it with diagnostics and hover, as in VS Code.
  If those need the editor, a hint should say so.
- **Actual:** The file opens in the read-only view. No server starts, no
  `lsp.*` request is sent, the footer shows only "Connected to ovra 0.1.0
  (headless)", and hover does nothing (`07-main-reopened.png`). Everything
  starts only after clicking the small "Edit" link at the top right
  (`09-todo.png`). Before trusting, the view doesn't say why either
  (`03-mainrs-untrusted.png`).
- **Why it matters for Nadia:** her goal is to "open, find, read". The read-only
  view is exactly where she reads, so she concludes Ovra has no code
  intelligence.
- **Suggested fix:** attach the LSP client (diagnostics, hover, definition) to
  the read-only view as well, or at least show a one-line hint in the view
  header ("Language features in Edit mode") and let the footer item reflect the
  shown file's language.

### NEW-2 major: The trust card that turns on language servers never mentions them

- **Area:** ui/src/agents/setup (ProjectTrust) with ui/src/lsp
- **Steps:** 1. Open `demo-app` (untrusted). 2. Open `src/main.rs`, click Edit.
  3. Footer shows "Language servers off"; click it.
- **Expected:** The panel explains why and offers "Trust project…" (the spec:
  "the editor says why and links to trust"). The trust card says that trusting
  also lets language servers run this project's code.
- **Actual:** The panel says "Language servers run this project's code, so they
  start only in trusted projects." and has no button or link (measured: 0
  links/buttons in the panel, `a2-panel-untrusted.png`). The editor area says
  nothing about trust. The only trust control is the "Project settings: not
  loaded / Review and trust…" card in the Agents column, whose text talks only
  about Claude's `.claude/` settings, hooks and skills (`05-trust-review.png`).
  Nadia has no reason to connect "Claude settings" with "Language servers off".
- **Why it matters for Nadia:** she reads messages literally. She ends up in a
  dead end (servers off, no action), or she trusts Claude hooks without knowing
  that this is also the switch for code intelligence.
- **Suggested fix:** add a "Review and trust…" button to the untrusted panel
  that opens the same review. Add one line to the trust card and review ("Also
  lets language servers run this project's code"). Show a slim notice in the
  editor for files whose language has a known server.

### NEW-3 minor: With a server running, opening a file whose server is missing changes nothing visible

- **Area:** ui/src/lsp
- **Steps:** (TypeScript added to `lsp.status.known` with `path: null` by
  rewriting the frame, see NEW-8) 1. Trust `demo-app`, edit `src/main.rs`
  (fake-lsp runs). 2. Open `ui/app.ts`, click Edit.
- **Expected:** The footer shows that this file has no language server ("No
  TypeScript server", with the attention mark), so she knows why hover is
  missing.
- **Actual:** The footer still says "fake-lsp" with no mark (aria-label
  "fake-lsp", `a5-ts.png`). The install hint only appears once she opens the
  panel (`a6-panel-ts.png`). The editor shows nothing.
- **Why it matters for Nadia:** "a hint when something is not ready" is what
  she expects. A label naming a different language's server tells her the
  opposite.
- **Suggested fix:** make the label follow the shown file's language: its
  server's name when it runs, "TypeScript: no server" plus the `!` mark when it
  is missing. Keep the full list in the panel.

### NEW-4 minor: After Stop, the footer says "Language server missing"

- **Area:** ui/src/lsp (`label` in LanguageServers.svelte)
- **Steps:** 1. Trusted project with fake-lsp running and a missing TypeScript
  server in `known`. 2. Footer item, then "Stop".
- **Expected:** "Language server stopped" or the item for the stopped server, so
  she sees her action took effect and knows how to start it again.
- **Actual:** Label becomes "Language server missing" (`a7-stopped.png`), as if
  the server she just stopped had been uninstalled. The panel shows only the
  TypeScript install hint.
- **Why it matters for Nadia:** she reads it literally and may try to reinstall
  something.
- **Suggested fix:** keep a stopped server listed as "Stopped" with a short
  "Starts again when you edit a rust file" note, and don't use "missing" for a
  stopped server.

### NEW-5 minor: A stopped server starts again silently

- **Area:** ui/src/editor/lsp (clients.ts) / ui/src/lsp
- **Steps:** 1. Stop fake-lsp from the panel. 2. Click `src/main.rs` again (or
  type in it).
- **Expected:** Either it stays stopped until she starts it, or the panel told
  her it would come back.
- **Actual:** Footer shows "fake-lsp" again and the diagnostic returns within
  about 2 s (`a8-main-after-stop.png`). Nothing said it would restart.
- **Why it matters for Nadia:** "Stop" looks like it didn't work. If she
  stopped it because of the "unsandboxed" warning, it is running again without
  her knowing.
- **Suggested fix:** a line under Stop: "It starts again when you open or edit a
  rust file". Or keep the server stopped for the session until a "Start"
  button is pressed. Decide this together with the security reviewer.

### NEW-6 minor: Rename, find references and go to definition can't be found by mouse

- **Area:** ui/src/editor/lsp, ui/src/keys (palette commands)
- **Steps:** 1. Edit `src/lib.rs` in a trusted project. 2. Right-click a name.
  3. Ctrl+click `a` in `a + b`. 4. Ctrl+Shift+P, type "rename", then
  "definition".
- **Expected:** VS Code habits work: Ctrl+click goes to the definition, and
  the context menu and palette list "Go to definition", "Find references",
  "Rename symbol" with their keys.
- **Actual:** Right-click shows only the browser menu. Ctrl+click only moves
  the cursor (the cursor stayed on line 2; the fake answers definitions with
  line 1). The palette has no such commands: "rename" matches only "Toggle
  screen reader mode in terminals" (`b4-palette-rename.png`). F12 works when
  you know it (the cursor jumped to `fn` in run 1).
- **Why it matters for Nadia:** she knows the shortcuts only vaguely and uses
  the mouse first. Features she can't find are features she doesn't have.
- **Suggested fix:** add palette commands (with their keys shown) for the LSP
  actions when a server is attached, and Ctrl/Cmd+click for go to definition.

### NEW-7 minor: F2 and Shift+F12 give no feedback when the server cannot do them

- **Area:** ui/src/editor/lsp
- **Steps:** 1. Edit `src/lib.rs` with fake-lsp running. 2. Put the cursor on
  `a`, press F2, then Shift+F12.
- **Expected:** A rename field or a references list, or a short "This language
  server cannot rename" message.
- **Actual:** Nothing happens at all: no panel, tooltip or message (`b2-f2.png`).
  The fake server doesn't advertise rename or references, so the actions
  themselves could not be tested in fake mode. This finding is only about the
  silence.
- **Why it matters for Nadia:** a key that does nothing looks like a broken app.
- **Suggested fix:** when the attached server lacks the capability, show a brief
  editor message instead of returning `false`. Also see NEW-8.

### NEW-8 minor: Fake mode cannot show the missing-server hint or test rename/references

- **Area:** crates/daemon (`language_servers` in fake mode), crates/lsp (fake-lsp)
- **Steps:** 1. `just ux` with the `small` fixture, trust, edit `ui/app.ts`.
- **Expected:** As the round brief and spec say, other languages show "Not
  installed" with an install command.
- **Actual:** In fake mode `lsp.status.known` lists only rust/fake-lsp, so
  `app.ts` gets no hint at all and the footer stays "fake-lsp"
  (`17-ts.png`, `18-panel-ts.png`). I could only review the hint by rewriting
  the frame in my script. fake-lsp also lacks `renameProvider` and
  `referencesProvider`, so F2 and Shift+F12 (in scope) cannot be exercised in
  headless mode.
- **Why it matters:** reviewers and e2e can't see the newcomer's most likely
  first state (a missing server).
- **Suggested fix:** in fake mode, also register a known TypeScript server
  with a program that does not exist. Teach fake-lsp `textDocument/references`
  and `textDocument/rename` (one file).

### NEW-9 minor: The servers panel stays open after clicking elsewhere and overlaps the palette

- **Area:** ui/src/lsp
- **Steps:** 1. Click the footer item. 2. Click in the editor. 3. Press
  Ctrl+Shift+P.
- **Expected:** A popup closes on an outside click, like other menus.
- **Actual:** It stays open (panel count 1 after the outside click,
  `b3-outside.png`) and is still on screen under the open palette
  (`b4-palette-rename.png`). Only Escape inside it or the footer button closes
  it.
- **Why it matters for Nadia:** a mouse user doesn't know how to dismiss it.
- **Suggested fix:** close on focusout or pointerdown outside the panel.

### NEW-10 polish: Panel jargon: "Sandboxed: bubblewrap, Landlock, seccomp", the raw language id, and the install hint shown before trust

- **Area:** ui/src/lsp
- **Steps:** Open the panel with fake-lsp running (`16-panel-trusted.png`),
  and in an untrusted project (`a2-panel-untrusted.png`).
- **Expected:** Plain words: "Sandboxed: can't change your files or reach the
  network". Language names as people say them ("Rust", "TypeScript"; the
  editor header already says "Rust").
- **Actual:** Lists the mechanisms by name, and the lowercase ids `rust` and
  `typescript` next to the server name. In an untrusted project the panel
  shows "Not installed ... npm install -g ..." under "servers start only in
  trusted projects". Installing would not help until she trusts, and she
  hasn't opened a TypeScript file yet.
- **Why it matters for Nadia:** unknown words scare her. Running an install
  command that changes nothing wastes her time.
- **Suggested fix:** a plain-language sandbox line (mechanisms in a tooltip),
  display names for languages, and while untrusted, put the trust step first
  and hide or mute install hints.

## What worked well

- After trusting with a rust file already open in Edit mode, the diagnostic
  appeared about 1 s after typing, with no reload or reopen (`a3-after-trust-same-tab.png`).
- Diagnostics and hover share one tidy tooltip ("fake hover" + "fake: TODO
  found"), and the wavy underline is easy to spot (`10-hover-lint.png`).
- The footer item is quiet when there is nothing to say. "Language servers off"
  is a clear, non-scary label. Untrusting the project switched it back at once.
- The panel's wording is short and honest ("Ovra does not install language
  servers. To get one, run its install command yourself."). The install
  command can be selected with one click. Escape closes the panel and returns
  focus.
- The footer item's text contrast is #9a9aa6 on #16161a, about 6.5:1.
