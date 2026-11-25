# Language servers (LSP)

Status: approved by the owner on 2026-11-25 (research
`git-lsp-gaps.md`, section 2). Decision record: ADR 0007. Builds on
`docs/specs/security.md` (the sandbox) and ADR 0006 (project trust).

## What the person gets

Opening a file in a trusted project starts the language server for its
language, if one is installed: diagnostics in the gutter, hover, completion,
signature help, go-to-definition (also into other files of the project),
find references, rename and formatting. The status bar shows each running
server, whether it is sandboxed, and lets the person stop it. A missing
server shows its name and how to install it, once per language.

Untrusted projects get no language servers; the editor says why and links
to trust.

## Pieces

| Piece             | Where                                                           | Owns                                                                                                |
| ----------------- | --------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| Protocol `lsp.*`  | `crates/core/src/protocol/lsp.rs`, `ui/src/lib/protocol/lsp.ts` | the messages below                                                                                  |
| Filter            | `crates/core/src/lsp/`                                          | trust, one server per project and language, URI rewriting, answering server requests, limits        |
| `LanguageServers` | trait in `crates/core/src/services.rs`, fake in `fakes.rs`      | the seam                                                                                            |
| `ovra-lsp`        | `crates/lsp`                                                    | known servers and finding them, spawning through `Sandbox::wrap_server`, framing, process lifecycle |
| Sandbox profile   | `crates/sandbox-linux` (`wrap_server`)                          | what a server may touch                                                                             |
| Client            | `ui/src/editor/lsp/`                                            | `@codemirror/lsp-client` over Ovra's transport, a `Workspace` over the editor's buffers             |
| Status            | `ui/src/lsp/`                                                   | status-bar item, missing-server hints, stop                                                         |

## Protocol

Method and event names are `lsp.<name>`.

Requests:

- `lsp.status { project }` answers `lsp.status { trusted, sandbox, servers, known }`:
  - `trusted`: whether servers may start;
  - `sandbox`: the sandbox servers would run in, `null` for none;
  - `servers`: the project's servers (`ServerInfo`);
  - `known`: for each language Ovra knows a server for, the server's name,
    the path it was found at or `null`, and an install hint.
- `lsp.start { project, language }` answers `lsp.server(ServerInfo)`.
  Returns the running server for that project and language if there is
  one. `forbidden` for an untrusted project, `not_found` when no server is
  known or installed for the language, `unavailable` past the cap of
  running servers (4) or without the service.
- `lsp.send { server, message }` answers `ok`. `message` is one JSON-RPC
  message from the client: a request, a notification, or a response to a
  server request the core passed on (none in v1). `bad_request` for a
  message that is not JSON-RPC 2.0, names a `file://` URI, or names a path
  outside the project; `not_found` for a server that is gone.
- `lsp.stop { server }` answers `ok`; `lsp.changed` follows.

Events:

- `lsp.message { server, message }`: one JSON-RPC message from the server,
  after the filter.
- `lsp.changed { server: ServerInfo }`: a server started, stopped or failed.

`ServerInfo` is `{ id, project, language, name, state, sandbox }`, where
`state` is `starting`, `running`, `stopped` or `failed` (with `reason`
next to it), and
`sandbox` is the sandbox's name or `null` for unsandboxed.

## URIs

The UI names files `ovra:///<path>` with the path in the project, each
segment percent-encoded, and the root `ovra:///`. The client sends
`rootUri: "ovra:///"` and `workspaceFolders` with that URI.

The filter rewrites URIs only where LSP puts them, never in document text:

- values of the keys `uri`, `rootUri`, `targetUri`, `oldUri`, `newUri`,
  `scopeUri` and `baseUri`;
- the keys of a `WorkspaceEdit`'s `changes` map;
- `rootPath`, which the client never sends and the core sets to the root.

Client to server: an `ovra:` URI becomes `file://<root>/<path>` after
`RelPath::parse`; a `file:` URI anywhere in those keys is refused.
Server to client: a `file:` URI under the root becomes `ovra:///<path>`; one
outside it (a standard library, a dependency) passes unchanged, and the UI
shows such a location as text it cannot open.

## Requests from the server

The core answers them and never forwards them:

| Method                                                                                       | Answer                              |
| -------------------------------------------------------------------------------------------- | ----------------------------------- |
| `workspace/configuration`                                                                    | `null` for each item                |
| `workspace/applyEdit`                                                                        | `{ applied: false, failureReason }` |
| `window/showDocument`                                                                        | `{ success: false }`                |
| `window/showMessageRequest`                                                                  | `null`                              |
| `window/workDoneProgress/create`, `client/registerCapability`, `client/unregisterCapability` | `null`                              |
| `workspace/workspaceFolders`                                                                 | the project's root folder           |
| anything else                                                                                | error `-32601`                      |

Notifications from the server (`textDocument/publishDiagnostics`,
`window/logMessage`, `$/progress`, ...) pass through the URI filter to the
UI. v1 does not offer `workspace/didChangeWatchedFiles`: servers watch the
project themselves (reading is allowed in the sandbox), so changes an agent
makes on disk still reach them.

## Limits

- One server per project and language; at most 4 running at once.
- A message over 16 MiB ends the server (`failed`).
- The core holds back forwarding while the event channel is backed up, as
  terminals do, so a flood of diagnostics cannot make every client resync.
- A server stops when the person stops it, its project loses trust or is
  removed, or it exits. It keeps running when the project closes in every
  client, so a reload does not restart it.

## Sandbox profile

`ServerProfile { project, cache_dir, read, network }`:

- `project`: the project root, readable, never writable;
- `cache_dir`: `<data dir>/lsp/<project id>/<language>`, writable; the
  server's environment points build output there (`CARGO_TARGET_DIR`,
  `XDG_CACHE_HOME`, `TMPDIR`);
- `read`: more readable folders (toolchains the server needs);
- `network`: `Blocked` in v1 (`CARGO_NET_OFFLINE=true`).

The platform adds what `security.md` lists for agents except the agent's own
state: no `~/.claude`, no netproxy, the same environment allowlist, the
same hidden secrets. The user's package caches (`~/.cargo/registry`,
`~/.npm`, ...) are readable, never writable.

## Tasks

| #   | Task                                                                                                               | Area                                     | After                |
| --- | ------------------------------------------------------------------------------------------------------------------ | ---------------------------------------- | -------------------- |
| L.0 | This spec, ADR 0007, protocol, trait and fake, `Sandbox::wrap_server`, stub `crates/lsp`, crate graph, wiring      | shared files                             | owner's approval     |
| L.F | The filter: trust, start/stop, URIs, server requests, limits                                                       | `crates/core/src/lsp/`                   | L.0                  |
| L.A | `ovra-lsp`: framing, spawn through the sandbox, lifecycle, known servers on `PATH`, a fake server binary for tests | `crates/lsp`                             | L.0                  |
| L.B | Editor client: transport, workspace, diagnostics, hover, completion, definition, rename                            | `ui/src/editor/lsp/`                     | L.0                  |
| L.C | Status bar and hints                                                                                               | `ui/src/lsp/`                            | L.0                  |
| L.D | Linux `wrap_server` profile and attack tests (`build.rs` reads `~/.ssh`, writes the project)                       | `crates/sandbox-linux`, `tests/security` | L.0, security review |
| L.E | e2e on the fake server: diagnostics appear, go-to-definition opens the file                                        | `tests/e2e`                              | L.A, L.B, L.F        |
