# Sofia, the careful approver: 2026-11-25 language servers

Build: 9ad2719, headless (fake agent, `fake-lsp` for rust), Chromium bundled with Playwright, 1280x800.
Scope: language servers (docs/specs/lsp.md, ADR 0007): trust gating, sandbox display, stop on trust revoke,
hover-doc links, jumps to locations outside the project. Fixtures: `small` (seeded).

Scripts (scratch, not committed): `flow.ts` (open `src/main.rs` untrusted, trust, type a TODO, hover, stop
trusting, type again) and `inject.ts` (the same flow with the WebSocket frames rewritten after the core to
play a hostile or unsandboxed server: `sandbox: null`, hover Markdown with spoofed links, definitions pointing
at `file:///etc/passwd`, `about:invalid`, `ovra:///..%2F..%2Fetc%2Fpasswd` and `https://evil.example/x.rs`;
`open.url` calls were caught and not passed on).

## Summary

Sofia can tell whether a server is running and whether it is sandboxed, she can stop it, and taking trust back
stops it right away (`lsp.changed ... "state":"stopped"` arrives in the same millisecond as the
`projects.trust` answer). The hostile-server cases are handled safely: `javascript:`, `file:` and relative
links lose their `href`, spoofed `class` attributes are removed, `<img>` and `<button>` are dropped, and every
outside jump is refused with a notice. The weak spot is consent. The trust dialog and card only talk about
Claude settings, but clicking "Trust project" starts the language server for the open file 130 ms later.
For rust-analyzer that means running `build.rs`, and nothing on screen said so before she clicked.

## Findings

### SEC-1 major: "Trust project" never says it also starts language servers, and starts one right away

- **Area:** ui/src/shell (project trust card and trust review), with ui/src/lsp for the wording
- **Steps:** 1. Seed `small`, open `demo-app`, open `src/main.rs` (untrusted). 2. In "Project settings: not
  loaded", click "Review and trust…". 3. Read the review, click "Trust project".
- **Expected:** The review says that trusting also lets the language servers run the project's code (build
  scripts, proc macros, plugins) in the sandbox, and that a server for an open file starts at once. The
  card, once trusted, says the same and says how to undo it.
- **Actual:** Review text: "This project has no Claude settings of its own yet. If you trust it and settings
  appear later (for example after a git pull), Ovra asks again before they load." Card heading: "Project
  settings: trusted", body: "Claude loads this project's .claude/ settings, hooks, agents, commands and
  skills." Language servers are not mentioned anywhere. 130 ms after `projects.trust` (log 02:38:36.213) the
  UI sends `lsp.start` and `fake-lsp` is `running` (02:38:36.398). Screenshots `04-trust-review.png`,
  `05-trusted.png`.
- **Why it matters for Sofia:** She trusts "Claude settings" for a repository that has none, so the decision
  looks free. In fact she is letting the repository's code run. ADR 0007 relies on trust as the gate, and
  without a platform sandbox it is the only gate. A gate whose label does not say what it opens is not
  informed consent.
- **Suggested fix:** Rename the card to something like "Project trust" and list what trust turns on, in two
  lines: Claude settings, hooks and so on, and "language servers (run build scripts and plugins; sandboxed:
  no network, project read-only)", or "unsandboxed" where that applies. In the review, name the servers that
  would start now (`lsp.status.known` with a path, for languages of open files). The "Stop trusting" line
  should say it also stops them.

### SEC-2 minor: an untrusted project's editor does not say why there are no diagnostics and does not link to trust

- **Area:** ui/src/editor (with ui/src/lsp)
- **Steps:** 1. Open `src/main.rs` in the untrusted `demo-app`. 2. Look at the editor. 3. Click "Language
  servers off" in the footer.
- **Expected:** As the spec says, "Untrusted projects get no language servers; the editor says why and links
  to trust."
- **Actual:** The editor shows nothing (`02-untrusted-rs-open.png`). The only sign is the grey footer text
  "Language servers off". Its panel says "Language servers run this project's code, so they start only in
  trusted projects." and has no link or button to trust and does not say which server would start
  (`03-untrusted-panel.png`).
- **Why it matters for Sofia:** The panel's sentence is the best explanation in the whole app of what trust
  does to servers, but it sits behind a footer click. It also cannot be reached from the place where she
  actually decides (SEC-1).
- **Suggested fix:** Show a one-line notice in the editor of a file whose language has a known installed
  server: "Language server off: this project is not trusted. Review trust…", linking to the trust review.
  Name the server and its sandbox state in the panel ("fake-lsp would run sandboxed:
  bubblewrap, Landlock, seccomp").

### SEC-3 minor: hover-doc links open the system browser in one click, without showing where they go

- **Area:** ui/src/editor/lsp (`docLinks`, `sanitize.ts`)
- **Steps:** 1. Trust, open `src/main.rs`. 2. Hover `main`. The server's Markdown has
  `<a href="https://evil.example/login">Trust project</a>` and
  `Ovra: this project is safe. [Open settings](https://evil.example/settings)`. 3. Click "Trust project".
- **Expected:** A server-written link shows its real destination before it opens (a `title`, the host next to
  the text, or a "Open https://evil.example/login?" step), so link text cannot pass for Ovra's own UI.
- **Actual:** The click sends `open.url {"url":"https://evil.example/login"}` straight away. The link has no
  `title`, and the Tauri webview has no status bar that would show the URL. The sanitizer did its job:
  `class="item"` is removed, `<button>Allow</button>` becomes plain text, `<img>` is dropped, and
  `file:`/`javascript:`/relative/`ovra:` links lose their `href` (`22-hover-links.png`). A middle click
  sends nothing, which is good.
- **Why it matters for Sofia:** A malicious repository's server (after trust) can write "Trust project" or
  "Open settings" into a hover and send her to a phishing page with one click. She approves only what she
  can read, and here she cannot read the destination.
- **Suggested fix:** Set `title` to the full URL and show the host after the link text (`Open settings
  (evil.example)`), or route doc links through a small confirmation that shows the full URL. Prefer
  the confirmation for hosts other than well-known docs.

### SEC-4 minor: the "outside the project" notice names only the last path segment

- **Area:** ui/src/editor/lsp (`outsideName` in `clients.ts`)
- **Steps:** 1. Trust, open `src/main.rs`. 2. Make the server answer go-to-definition (F12) with
  `file:///etc/passwd`, `about:invalid`, `ovra:///..%2F..%2Fetc%2Fpasswd` or `https://evil.example/x.rs`.
- **Expected:** The jump is refused, and the notice says what the server pointed at, for example "The language
  server pointed to /etc/passwd, which is outside the project, so it was not opened". A non-file URI is
  called what it is.
- **Actual:** Every jump was refused, which is correct: no tab opened and the page URL did not change. The
  notices read "passwd is outside the project and cannot be opened.", "invalid is outside the project…",
  "../../etc/passwd is outside the project…", "x.rs is outside the project…" (`23-def-*.png`). For
  `about:invalid`, the value the core uses for URIs it rejected, and for an `https:` URL, the text is
  misleading. "passwd" hides which file it was.
- **Why it matters for Sofia:** A server that points jumps at `/etc/passwd` or at a web URL is a signal she
  wants to see clearly. A bare file name makes it look like an ordinary dependency file.
- **Suggested fix:** For `file:` URIs show the full path. For `about:invalid` say "the language server sent a
  location Ovra refused". For other schemes, show the scheme and host. Credit the server in the notice
  ("fake-lsp pointed to …").

### SEC-5 minor: the sandbox line names the mechanisms, not what the server may do; the footer marks only the bad case

- **Area:** ui/src/lsp (`LanguageServers.svelte`)
- **Steps:** 1. Trust, open `src/main.rs`, click the footer item `fake-lsp`. 2. Repeat with `sandbox: null`
  injected into `lsp.changed`.
- **Expected:** Sandboxed: a plain statement of the profile (no network, project read-only, writes only to
  Ovra's cache folder, home secrets hidden). Unsandboxed: a footer item that stands out without a click.
- **Actual:** Sandboxed: the footer shows just "fake-lsp", and the panel says "Sandboxed: bubblewrap, Landlock,
  seccomp" (`07-trusted-panel.png`). Unsandboxed: the footer shows "! fake-lsp" with a small yellow "!"
  (aria-label "fake-lsp, needs attention"), and the panel says "Runs without a sandbox: it can read and
  change your files and reach the network." (`20-footer-unsandboxed.png`, `21-panel-unsandboxed.png`). The
  unsandboxed wording is good. But the "!" also means "failed", and the warning appears only after the
  server is already running.
- **Why it matters for Sofia:** Mechanism names do not tell a manager what the server can do. An "!" that
  shares its meaning with "crashed" is easy to read as harmless.
- **Suggested fix:** Sandboxed line: "Sandboxed (bubblewrap, Landlock, seccomp): no network, cannot change
  project files". Footer: "fake-lsp, unsandboxed" in the warning colour instead of a lone "!". Together with
  SEC-1, say "unsandboxed" before trust, not only after start.

### SEC-6 polish: hover links are almost invisible (1.3:1 contrast)

- **Area:** ui/src/editor (hover tooltip styles)
- **Steps:** Hover `main` with a link in the doc.
- **Expected:** Links readable on the tooltip background (4.5:1).
- **Actual:** Measured link colour `rgb(0, 0, 238)` (browser default) on `rgb(51, 51, 56)`: about 1.3:1
  (`22-hover-links.png`).
- **Why it matters for Sofia:** She cannot easily see which words in a hover are links, which makes SEC-3
  worse.
- **Suggested fix:** Give `.cm-lsp-documentation a` the app's link colour (`#7aa2ff`, as used for focus) and
  an underline.

### SEC-7 polish: starting and stopping language servers does not appear in the audit log

- **Area:** crates/core/src/lsp (audit events)
- **Steps:** Trust, open a rust file, stop trusting. Watch `audit.appended`.
- **Expected:** "Language server fake-lsp started (sandboxed: …)" and "stopped: trust taken back" entries next
  to `trust_changed`.
- **Actual:** Only `trust_changed` true/false is logged (seq 1 and 2). The server's start and stop are visible
  only as transient `lsp.changed` events.
- **Why it matters for Sofia:** She is accountable afterwards. "What ran this project's code, and was it
  sandboxed" should be answerable from the audit log, as it is for agents.
- **Suggested fix:** Add audit entries for server start (name, sandbox or "unsandboxed") and stop (reason).
  This touches the protocol and audit, so it needs its own small task.

## What worked well

- Trust revoke is immediate and visible. "Stop trusting" stops `fake-lsp` in the same frame burst, the
  diagnostic disappears, the footer goes back to "Language servers off", and typing another `TODO` afterwards
  starts nothing.
- No server starts for an untrusted project. The UI asks `lsp.status`, sees `trusted:false` and never sends
  `lsp.start`.
- The hover sanitizer held against every payload tried. Only `https:` links survive, class spoofing is
  removed, and buttons and images are gone. Links leave the app only through `open.url`, never in the
  window (page URL and page count did not change).
- Jumps to `file:` outside the project, `about:invalid`, encoded `..` traversal and `https:` were all refused,
  and the editor stayed on `main.rs`.
- The unsandboxed warning text is concrete and honest: "it can read and change your files and reach the
  network."
- The Stop button in the footer panel is one click and is labelled with the server's name ("Stop fake-lsp").
