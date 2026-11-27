# Architecture

All logic lives in the Rust core. The UI and the OS are replaceable layers
around it, so the same code runs in the Tauri window, in a browser (headless
mode) and in tests.

```
 Tauri window (WKWebView / WebView2 / WebKitGTK)     Browser (headless mode)
        |  Tauri IPC: ovra_call + "ovra://frame"           |  WebSocket /ws?token=
        v                                                  v
   app/ (ovra-app)                                 crates/daemon (ovra-daemon)
        \______________________  ______________________/
                               \/
                     crates/transport  (JSON frames, WS endpoint)
                               |
                         crates/core  (protocol, state, events, service traits)
                               |
        services wired in by app/daemon: fs-index, store, opener, ...
```

## Services

The core never touches the OS. Each OS-facing capability is a trait in
`crates/core/src/services.rs` (`FsService`, `ProjectStore`, `Opener`, ...),
implemented in its own crate and wired in by `app` and `daemon` through
`Core::builder()`. Services that are not wired answer `unavailable`. Every
trait has an in-memory fake in `crates/core/src/fakes.rs` that tests in any
crate can use (`ovra-core` with the `fakes` feature). Client paths are checked
by `RelPath::parse` before they reach a service, so a request cannot leave its
project.

## Frames

Both transports carry identical JSON frames (`crates/transport/src/lib.rs`):

- client to core: `{"id": 1, "method": "fs.list_dir", "params": ...}`
- core to client: `{"id": 1, "result": ...}`, `{"id": 1, "error": ...}`,
  or `{"event": ...}` for pushed events.

The UI's `FrameRouter` (`ui/src/lib/transport.ts`) matches replies to calls and
fans out events; only the wire under it differs.

Things only a desktop window has stay outside the core and the frames: the
system's folder dialog (`ovra_pick_folder`) and dropped paths. They are
optional `Transport` members (`pickFolder`, `onDrop`) that headless mode
lacks, and a path from them goes to `projects.add` like a typed one. A drop
onto the file tree copies into the project by the drop's id
(`importDrop`, `ovra_import_drop`): the app keeps the dropped paths, and
no source path goes from the page back to the app. Every
window command is listed in `app/build.rs` and needs its own entry in
`app/capabilities/`.

## Headless mode

`ovra-daemon` binds to 127.0.0.1 only, generates a token per launch, prints
`OVRA_URL=http://127.0.0.1:<port>/?token=<token>` and serves `ui/dist`.
WebSocket connections need the token and, from browsers, an allowed `Origin`.
This is what lets agents in a Linux cloud container run Playwright e2e tests.

## Agents (phase 2, ADR 0004, ADR 0006)

Each Claude session is a `claude -p` process from the user's own Claude Code
installation, signed in with the user's own account. `crates/agent-runtime`
starts it and speaks stream-json on stdio: messages stream out, tool approvals
arrive as `can_use_tool` control requests and go to the core's approval flow.
Ovra never touches credentials. Other agents connect through ACP, with PTY +
CLI as a fallback. An Agent SDK sidecar (ADR 0004) may come later. All of them
sit behind one `AgentRuntime` trait in the core.

Sessions run either `isolated` (own git worktree) or `direct` (the main
checkout, at most one per project; review is a diff against a baseline
snapshot taken at session start). See ADR 0005.

Sessions and their transcripts are kept across restarts (`SessionStore`, in
the project database). A session that ran when the core quit shows as
stopped; `agents.resume` continues it with the agent's own conversation id
(`claude --resume`). One that was mid-turn is marked `interrupted`, and
`agents.resume_interrupted` continues those with Ovra's note, coordinators
first. While a session works, the core watches for a stuck turn (repeats,
failures in a row, silence) and sends a hint (`agents.stuck`); see
`docs/specs/stuck-and-resume.md`.

## Orchestration (phase 3)

A coordinator session plans a task and starts workers, each an isolated
session in its own git worktree, through Ovra's IDE tools: an MCP server
(`crates/mcp-server`) that the `claude` process reaches in-process over its
stream-json pipe. Tasks, limits and review items are core state
(`crates/core/src/orchestration`); git work is behind `Worktrees`
(`crates/worktree`). See `docs/specs/orchestration.md`.

## Security (phase 4)

The agent runtime starts every `claude` process through a `Sandbox`, which
wraps its command line in a platform sandbox; `crates/sandbox` holds what
all platforms share and picks the one for this OS (none yet, so sessions
run unsandboxed and say so). Project policies decide tool calls before the
person is asked (`crates/core/src/policy.rs`). Each session's network goes
through its own netproxy (`crates/netproxy`), which lets through only the
hosts `claude` needs and the project's allowlist, and never loopback or
private addresses. See `docs/specs/security.md`.

## Language servers (ADR 0007)

The editor's LSP client talks to language servers over the same frames
(`lsp.send`, `lsp.message`). Servers start only for trusted projects, run
through the `Sandbox` with the project read-only (`Sandbox::wrap_server`),
and are found on `PATH`, never shipped. `crates/lsp` runs the processes;
the core filters messages both ways (`crates/core/src/lsp/`): it maps the
UI's `ovra:///` URIs to files under the root and answers the server's
requests itself. See `docs/specs/lsp.md`.

## Planned crates

Added only when a phase needs them: `acp-client`, `ext-host`, `sandbox-linux`,
`-macos`, `-windows`, `secrets`. Platform code sits behind traits
with fake implementations so everything except the platform crates builds and
tests on Linux.
