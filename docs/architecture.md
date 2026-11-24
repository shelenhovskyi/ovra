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
                         crates/core  (protocol, state, events)
```

## Frames

Both transports carry identical JSON frames (`crates/transport/src/lib.rs`):

- client to core: `{"id": 1, "method": "get_info", "params": ...}`
- core to client: `{"id": 1, "result": ...}`, `{"id": 1, "error": ...}`,
  or `{"event": ...}` for pushed events.

The UI's `FrameRouter` (`ui/src/lib/transport.ts`) matches replies to calls and
fans out events; only the wire under it differs.

## Headless mode

`ovra-daemon` binds to 127.0.0.1 only, generates a token per launch, prints
`OVRA_URL=http://127.0.0.1:<port>/?token=<token>` and serves `ui/dist`.
WebSocket connections need the token and, from browsers, an allowed `Origin`.
This is what lets agents in a Linux cloud container run Playwright e2e tests.

## Planned crates

Added only when a phase needs them: `orchestrator`, `acp-client`, `pty`,
`fs-index`, `worktree`, `mcp-server`, `ext-host`, `sandbox` (+ `-linux`,
`-macos`, `-windows`), `netproxy`, `secrets`. Platform code sits behind traits
with fake implementations so everything except the platform crates builds and
tests on Linux.
