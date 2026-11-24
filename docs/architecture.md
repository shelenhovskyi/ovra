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

## Headless mode

`ovra-daemon` binds to 127.0.0.1 only, generates a token per launch, prints
`OVRA_URL=http://127.0.0.1:<port>/?token=<token>` and serves `ui/dist`.
WebSocket connections need the token and, from browsers, an allowed `Origin`.
This is what lets agents in a Linux cloud container run Playwright e2e tests.

## Agents (phase 2, ADR 0004)

Claude runs through the Claude Agent SDK, which exists only for TypeScript and
Python. So each Claude session lives in a sidecar process, `agent-host/`
(TypeScript), that the core supervises over JSON lines on stdio. Other agents
connect through ACP, with PTY + CLI as a fallback. All of them sit behind one
`AgentRuntime` trait in the core.

Sessions run either `isolated` (own git worktree) or `direct` (the main
checkout, at most one per project; review is a diff against a baseline
snapshot taken at session start). See ADR 0005.

## Planned crates

Added only when a phase needs them: `pty`, `agent-runtime`, `acp-client`,
`worktree`, `orchestrator`, `mcp-server`, `ext-host`, `sandbox` (+ `-linux`,
`-macos`, `-windows`), `netproxy`, `secrets`. Platform code sits behind traits
with fake implementations so everything except the platform crates builds and
tests on Linux.
