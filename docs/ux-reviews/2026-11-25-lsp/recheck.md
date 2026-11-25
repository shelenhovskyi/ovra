# Language servers round: recheck

Build: 4e5f2ef, headless fake mode, Playwright Chromium, 1280x800.
Scope: the 16 issues in `summary.md`, plus what landed since: rename (F2)
across files that are not open, find references (Shift+F12) in files that
are not open, the "Language:" palette commands from the viewer, keyboard
hover (Mod+K Mod+I), language server start and stop in the audit log.
Fixtures: `small` (seeded), plus `app.rs`, `other.rs`, `third.rs` written
at the project root so the fake server's root-`.rs` search finds them.
Personas: newcomer (trust, viewer, footer), keyboard power user (palette,
keys, panel, focus), security reviewer (trust text, hover links, outside
paths, audit log).

## Summary

Most of the round is fixed: the trust dialog names language servers and
the sandbox, the viewer and untrusted editor say where language features
are, every action is in the palette and on a key, failed requests give one
short line instead of stacked error bars, and Stop keeps focus in the
panel. Cross-file rename and references work and land as unsaved tabs that
can be reviewed, undone and saved; a rename that would touch a file with
unsaved text is refused. Still open: the footer label (it names the wrong
server for the file you see), the restart story after Stop (the panel now
promises a restart on edit that does not happen), one-click hover links,
and a new gap where rename skips the unsaved text of other tabs without a
word.

## Recheck of summary.md

| #   | Sev   | Status                    | What I saw                                                                                                                                                                                                                                                                                                                                                                                      |
| --- | ----- | ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | major | fixed                     | "Review and trust..." shows `trust-language-servers`: "Trusting it also lets Ovra start language servers for the files you edit. They run the project's code (build scripts, for example) in the sandbox (bubblewrap, Landlock, seccomp), which keeps them to the project and their cache, without network. Stop trusting stops them."                                                          |
| 2   | major | fixed                     | The viewer header reads "fake-lsp in Edit" next to Edit. Untrusted, Ctrl+E shows a banner: "No fake-lsp here: language servers run the project's code, so they start only once you trust the project (Project settings)." (The untrusted viewer shows no hint; the banner in Edit covers it.)                                                                                                   |
| 3   | major | fixed                     | Palette, typing "Language:" in view mode lists 7 commands with keys: Format file, Rename symbol, Show problems, Find references, Go to definition, Go to next problem, Show hover at cursor. Running "Find references" from the viewer switched to Edit and opened the references panel (about 1.5 s). No context menu; the palette is enough for this persona. See NEW-R3 for the hover entry. |
| 4   | major | fixed                     | Ctrl+Shift+M opens the problems list with focus in it ("fake: TODO found"); F8 shows the diagnostic at the cursor; Ctrl+K Ctrl+I shows "fake hover" at the cursor, Escape closes it.                                                                                                                                                                                                            |
| 5   | major | fixed                     | After Stop, F12, Shift+F12, F2 and Shift+Alt+F in the rust file give one notice "No language server is running for this file."; no `.cm-panel` error bars, no page errors, no `Field is not present in this state` in `session.log`.                                                                                                                                                            |
| 6   | minor | fixed                     | Shift+Alt+F with fake-lsp (no formatting provider): "fake-lsp cannot format this file." In `ui/app.ts` (no server): "No language server is running for this file." Ctrl+Space not retested.                                                                                                                                                                                                     |
| 7   | minor | still open (half)         | Links are now readable (`#8fb4ff`, underlined) and non-http links lose their `href`. But one click still sends `open.url` with nothing shown first: a link labelled "Trust project" opened `https://evil.example/login` in one click (3 `open.url` frames for 3 clicks).                                                                                                                        |
| 8   | minor | fixed                     | F12 to `file:///etc/passwd`: "/etc/passwd is outside the project and cannot be opened." Unparsable targets (`about:invalid`, `ovra:///..%2F..`, `https://...`): "A location is outside the project and cannot be opened."                                                                                                                                                                       |
| 9   | minor | fixed                     | Untrusted project in Edit shows the banner quoted in 2, naming Project settings.                                                                                                                                                                                                                                                                                                                |
| 10  | major | fixed                     | Enter on "Stop fake-lsp": focus moves to the panel heading (`H2 Language servers`), panel says "Stopped fake-lsp. ..." in a `role=status`.                                                                                                                                                                                                                                                      |
| 11  | minor | fixed                     | Untrusted panel: "Language servers run this project's code, so they start only in trusted projects." with a "Review and trust..." button.                                                                                                                                                                                                                                                       |
| 12  | minor | still open                | With `ui/app.ts` shown (no server installed) the footer reads "! fake-lsp" (aria "fake-lsp, needs attention"): marked now, but it names the rust server, not the missing one. After Stop, with `app.rs` (rust) shown, the footer reads "! fake-ts not installed" (screenshot `06-after-edit`). Both name a server unrelated to the file on screen.                                              |
| 13  | minor | still open, worse wording | Panel after Stop: "Stopped fake-lsp. It starts again when you edit a rust file." Typing `// TODO again` into `app.rs` and waiting 1.5 s: no `lsp.start` frame, no diagnostic, footer still "fake-ts not installed". The server only comes back when the tab is shown again (`LanguageClients.showing`). No Restart button. See NEW-R2.                                                          |
| 14  | minor | mostly fixed              | Clicking into the editor closes the panel; opening the palette closes it. Tabbing out of it does not: after 8 Tabs focus is on the header's Browser button and the panel is still open.                                                                                                                                                                                                         |
| 15  | minor | fixed                     | Panel: "In the sandbox: reads the project, writes only its own cache, no network (bubblewrap, Landlock, seccomp)." Unsandboxed: a yellow "Runs without a sandbox: it can read and change your files and reach the network." The footer still marks it with only "!", and the same "!" now also means "a server is missing" (12).                                                                |
| 16  | minor | fixed                     | fake-lsp answers references, rename and completion; fake mode lists `fake-ts` for typescript as not installed.                                                                                                                                                                                                                                                                                  |

Not filed last round, now done: the audit log (Audit log button, Show:
Servers) lists "Language server fake-lsp started / For rust files, in the
sandbox (bubblewrap, Landlock, seccomp)." and "Language server fake-lsp
stopped / It served rust files." with timestamps.

## Findings

### NEW-R1 major: rename skips other tabs' unsaved text without a word, leaving broken code

- **Area:** ui/src/editor/lsp (`clients.ts`, `rename.ts`)
- **Steps:** 1. Trust a project with root `app.rs` (`fn greet() {}` and a
  call), `other.rs` (`use crate::greet;`), `third.rs` (`fn x() { greet(); }`).
  2. Edit `app.rs`, F2 on `greet`, rename to `hello`: all three get the
  edit, `other.rs` and `third.rs` as unsaved tabs. 3. Save `other.rs` only;
  type `// mine` in `third.rs` (still unsaved, now calls `hello()`).
  4. Back in `app.rs`, F2 on `hello`, rename to `hi`.
- **Expected:** `third.rs` is renamed too, or the rename is refused because
  `third.rs` has unsaved text ("third.rs has unsaved changes...", which does
  happen when the server proposes an edit there).
- **Actual:** "Renamed in 2 files. Not saved yet: review other.rs in their
  tabs..." `third.rs` keeps `hello();` (log: `third.rs after refused rename:
  fn x() { hello(); }`). The editor sends `didClose` for every tab it stops
  showing (`didOpen`/`didClose` pairs per tab switch in `session.log`), so
  the server reads `third.rs` from disk, sees no `hello`, and the refusal
  check in `renameAcross` never runs. Find references has the same blind
  spot. A real server would do the same: closed documents come from disk.
- **Why it matters for the keyboard power user:** cross-file rename is
  trusted to be complete; this one silently leaves a call to a name that no
  longer exists, found only at build time.
- **Suggested fix:** keep every open tab with unsaved text open to the
  server (`didOpen` until the tab closes), or refuse a rename while any
  project file of that language has unsaved text and name the file.

### NEW-R2 major: after Stop the panel promises a restart on edit that does not come

- **Area:** ui/src/lsp (`LanguageServers.svelte`), ui/src/editor/lsp (`clients.ts`)
- **Steps:** 1. Edit a rust file with fake-lsp running. 2. Footer, Stop
  fake-lsp. 3. Read the panel. 4. Type a `// TODO` line in the same file.
- **Expected:** as the panel says, the server starts again and the TODO
  gets its diagnostic; or the panel offers Start.
- **Actual:** panel: "Stopped fake-lsp. It starts again when you edit a
  rust file." After the edit: no `lsp.start`, no diagnostic, the stale
  notice "No language server is running for this file." stays, footer
  "! fake-ts not installed", and the panel no longer mentions fake-lsp at
  all (screenshot `06-after-edit`). Switching to another tab and back
  starts it.
- **Why it matters for the newcomer:** they follow the message, see nothing
  happen, and have no button to press; the only fix (switch tabs) is
  undiscoverable.
- **Suggested fix:** start the server on the next change in a buffer of
  that language (or on the next action key), and/or keep a stopped row
  with a Start button; make the message match what happens.

### NEW-R3 polish: the hover command shows its keys inside the title

- **Area:** ui/src/editor (`commands.ts`)
- **Steps:** Ctrl+Shift+P, type "Language:".
- **Expected:** "Language: Show hover at cursor" with "Ctrl+K Ctrl+I" in
  the key column like the other six.
- **Actual:** "Language: Show hover at cursor (Mod+K Mod+I)" with an empty
  key column; "Mod" is not shown as Ctrl as everywhere else.
- **Why it matters for the keyboard power user:** inconsistent hints, and
  "Mod" is internal wording.
- **Suggested fix:** let the palette show a chord, or at least write the
  platform name (Ctrl / Cmd) in the title.

### NEW-R4 polish: the rename notice does not count the shown file as unsaved

- **Area:** ui/src/editor/lsp (`rename.ts`)
- **Steps:** Rename `greet` to `hello` across three files.
- **Expected:** the notice says all three files are unsaved.
- **Actual:** "Renamed in 3 files. Not saved yet: review other.rs, third.rs
  in their tabs, ..."; `app.rs` is unsaved too (dot on its tab) but not
  named. There is no way to save all three at once.
- **Why it matters for the newcomer:** they save the two named tabs and
  leave the definition unsaved.
- **Suggested fix:** name every changed file in the notice ("app.rs,
  other.rs, third.rs"); a "Save all" can wait for its own task.

## What worked well

- Cross-file rename (new): edits land in unsaved tabs (aria "unsaved
  changes"), disk untouched until Ctrl+S; Ctrl+Z / Ctrl+Shift+Z in
  `other.rs` undo and redo the rename there; saving writes only that file.
- Refusal (new): with `third.rs` dirty and the server proposing an edit
  there: "Rename not applied: third.rs has unsaved changes. Save or revert
  it, then rename again." and nothing changed in any file.
- Find references (new) lists `other.rs` and `third.rs` while they are not
  open, focus lands in the list, picking an entry opens the file.
- Keyboard hover (new) and the problems list make diagnostics fully
  keyboard-readable.
- Audit log (new): start and stop entries with the sandbox named, under
  the Servers filter.
- Clear, plain wording in the trust dialog, the untrusted banner and the
  panel's sandbox line.

## Fixed after this recheck

- NEW-R1: a rename is refused while a file of the server's language has
  unsaved text the server is not shown, naming the file; find references
  says which unsaved files it could not search.
- NEW-R2: after Stop the panel says the server starts again the next time
  a file of its language opens, which is what happens.
- Issue 12: the footer names the shown file's server, or its missing one.
- Issue 7: a hover link that does not show its site gets the site after it.
- NEW-R4: the rename notice names the shown file too.

Still open: 14 (Tab out of the panel keeps it open), 15 (one "!" for
both an unsandboxed and a missing server), NEW-R3 (the hover command's
chord is only in its title).
