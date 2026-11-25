# Language servers round: summary

Three personas (newcomer, keyboard power user, security reviewer) used
language servers in headless fake mode on 2026-11-25 (main at 9ad2719).
27 findings, deduplicated below into 16 issues, grouped by owning area.
What works: trust gating (no `lsp.start` before trust), stopping on
untrust, sanitized hover docs, jumps outside the project refused,
diagnostics within milliseconds, the footer panel by keyboard.

## ui/src/agents/setup (trust card)

1. **major** - The trust card and dialog never say that trusting starts
   language servers, which run project code (`build.rs`) about 130 ms
   later. Name them, whether they are sandboxed, and that "Stop trusting"
   stops them. (SEC-1, NEW-2)

## ui/src/editor (read-only view, palette, keys)

2. **major** - Clicking a file opens the read-only view, where no server
   starts and nothing says why; features appear only after Edit (Ctrl+E).
   (NEW-1, KEY-10)
3. **major** - No language-server action is in the command palette or a
   context menu: go to definition, references, rename, format, the
   server panel. (KEY-3, NEW-6)

## ui/src/editor/lsp (this thread)

4. **major** - Diagnostics and hover cannot be read from the keyboard: no
   lint keymap or panel key, no hover key. (KEY-1, KEY-2)
5. **major** - A failed request leaves the library's unstyled error bars;
   they stack, Escape does not close them, and after Stop they throw
   `Field is not present in this state`. (KEY-6)
6. **minor** - F2, Shift+F12, Ctrl+Space do nothing silently when the
   server lacks the feature. (KEY-7, NEW-7)
7. **minor** - Hover links open the browser in one click without showing
   where they go, and are nearly invisible (1.3:1). (SEC-3, SEC-6)
8. **minor** - "Outside the project" names only the last path segment
   (`/etc/passwd` reads "passwd", `about:invalid` reads "invalid"). (SEC-4)
9. **minor** - Untrusted project: the editor does not say why there are
   no diagnostics. (SEC-2)

## ui/src/lsp (footer and panel, this thread)

10. **major** - Stop drops focus to `<body>`. (KEY-4)
11. **minor** - The untrusted panel has no way to trust. (NEW-2, KEY-8, SEC-2)
12. **minor** - The footer label misleads: a missing server for the shown
    file is not marked while another runs; after Stop it says "Language
    server missing". (NEW-3, NEW-4)
13. **minor** - A stopped server restarts silently on the next edit, and
    cannot be restarted from the panel. (NEW-5, KEY-9)
14. **minor** - The panel stays open when focus leaves it and overlaps the
    palette. (KEY-5, NEW-9)
15. **minor** - Jargon: "Sandboxed: bubblewrap, Landlock, seccomp", raw
    language ids; an unsandboxed server only gets a small "!". (SEC-5, NEW-10)

## Test support (crates/lsp fake server, daemon)

16. **minor** - The fake server offers no references, rename or
    completion, and fake mode lists only rust, so the missing-server hint
    and those flows cannot be reviewed or tested end to end. (NEW-8)

Not filed: audit log entries for server start and stop (SEC-7, polish).
