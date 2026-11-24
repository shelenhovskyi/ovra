# 0002. Logic in a UI-free Rust core; Tauri and the browser are thin shells

- Status: accepted
- Date: 2026-11-24

## Context

Development happens mostly in Linux cloud containers without a display,
macOS or Windows. The product must also run headless.

## Decision

- `ovra-core` holds all logic and has no UI, OS, Tauri or HTTP dependencies.
- Front ends talk to it through one frame format over interchangeable
  transports: Tauri IPC in the window, WebSocket in headless mode.
- The UI is Svelte 5 + Vite + TypeScript (chosen over React for smaller
  runtime and fine-grained updates, given the 1 s cold start and 150 MB idle
  RAM targets).
- Crate dependency edges are whitelisted in `scripts/crate-graph.json` and
  checked in CI.

## Consequences

Everything except platform crates is built and tested on Linux, including UI
e2e tests through headless mode. Platform-specific features need a trait and a
fake implementation.
