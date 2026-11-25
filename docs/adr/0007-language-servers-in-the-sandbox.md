# 0007. Language servers run only for trusted projects, in the sandbox

- Status: accepted (owner, 2026-11-25)
- Date: 2026-11-25

## Context

Ovra's editor has tree-sitter highlighting but no diagnostics, hover,
completion, go-to-definition or rename. The Language Server Protocol gives
all of them through one client, and `@codemirror/lsp-client` (MIT, by the
author of CodeMirror) speaks it from the editor we already use.

Language servers run the project's code. rust-analyzer runs `build.rs` and
proc macros through `cargo check`, tsserver loads plugins named in
`tsconfig.json` from `node_modules`, gopls runs `go list` and toolchain
directives, clangd follows `--query-driver`. Opening a cloned repository with
a language server is running that repository, and in Ovra agents write these
files too. VS Code answers this with Workspace Trust; Ovra already has project
trust (`projects.trust`, ADR 0006) and a sandbox for agents (phase 4).

## Decision

- A language server starts only for a **trusted** project and only from a
  person's action: opening a file of that language in the editor. Taking
  trust back stops the project's servers. Servers never start for a
  worker's worktree, only for the project's own checkout.
- Every server runs **through the `Sandbox`**, with its own profile
  (`ServerProfile`, `Sandbox::wrap_server`): the project readable but never
  writable, one cache folder per project and language in Ovra's data folder
  writable (build output such as `CARGO_TARGET_DIR` goes there), the user's
  package caches and toolchains readable, the network blocked. The home
  folder's secrets stay hidden as for agents. Where the platform has no
  sandbox (macOS and Windows until their sandboxes land, Linux without
  bubblewrap) the server runs unsandboxed and the UI says so, as agent
  sessions do. Where the sandbox is available but cannot wrap a server, the
  server does not start.
- **Servers are found, never shipped.** Ovra looks for a known server per
  language on `PATH` and tells the person how to install it. It never
  bundles, downloads or installs one. Only servers under permissive licenses
  are suggested.
- **Messages go over Ovra's existing channel.** The UI's LSP client sends
  `lsp.send` and receives `lsp.message` events through
  `ui/src/lib/transport.ts`. No new socket, port or CSP entry.
- **The core is a filter, not a pipe** (`crates/core/src/lsp/`):
  - the UI names files by `ovra:///<path in the project>` URIs; the core
    turns them into `file://` URIs under the root and back, and refuses a
    client message that names a real `file://` URI;
  - requests from the server to the client (`workspace/applyEdit`,
    `window/showDocument`, `workspace/configuration`, capability
    registration, progress) are answered by the core and never reach the UI
    as requests. v1 applies no server-initiated edits and opens no
    documents or URLs;
  - one server per project and language, a cap on how many run at once,
    a cap on message size.
- The process work (spawning through the sandbox, `Content-Length` framing,
  lifecycle, finding servers) is the new crate `crates/lsp`
  (`ovra-lsp`, trait `LanguageServers`), depending on `ovra-core` only.

## Consequences

- People get diagnostics, hover, completion, go-to-definition and rename for
  the languages whose servers they installed, without a new way out of the
  UI or a new trust prompt.
- A malicious repository needs the person's trust before its build scripts
  run, and then runs them without the network, the home folder's secrets or
  write access to the project. Without a platform sandbox, trust is the only
  barrier; the UI says so.
- Servers that insist on writing into the project (a cache under
  `node_modules/.cache`, `target/`) are pointed elsewhere through their
  environment or fail on those writes. Each one is handled in the server
  registry.
- Without the network, a server cannot fetch dependencies the person has not
  fetched before (`cargo` runs offline). A per-project netproxy allowlist for
  package registries can come later; it would loosen the profile and needs
  the owner.
- `@codemirror/lsp-client` is young and tested mostly with TypeScript;
  expect fixes for other servers. Non-standard extensions
  (`experimental/*`) are ignored in v1.
- Diagnostics for agents (an IDE tool) would change `tools.json` and are not
  part of v1.
