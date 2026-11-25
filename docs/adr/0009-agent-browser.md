# 0009. The agent browser is a sandboxed Chromium driven over a CDP pipe

- Status: proposed
- Date: 2026-11-25

## Context

The owner wants a browser that agents drive and the person watches, takes
over and picks elements in. Options (research, 2026-11-25):

- A native Tauri webview. Child webviews need Tauri's `unstable` feature,
  work only under X11 on Linux, draw above the UI, and get Tauri's IPC
  scripts injected whatever their URL. There is no screenshot API and no
  automation on macOS, the per-webview proxy can silently stop applying on
  macOS, and headless mode and the e2e tests have no Tauri at all.
- A separate Chromium driven over the Chrome DevTools Protocol. One engine
  everywhere, full automation, it can run in the sandbox behind the
  session's netproxy, and the UI shows it as a screencast in both front
  ends. It costs a browser the user must have installed and some latency.

## Decision

- Each agent session can have a Chromium of the user's own, never
  downloaded (as ADR 0006 does for `claude`), started through the
  `Sandbox` with the session's netproxy as its only way out, a fresh
  profile, and control over `--remote-debugging-pipe` only.
- `crates/browser` implements a `Browser` service with `chromiumoxide`.
- The UI draws the screencast and Ovra's own pick overlay in a panel; the
  page never runs Ovra code and Ovra's UI never runs in the page.
- Agents get Playwright-style tools (accessibility snapshot with refs) but
  no arbitrary script evaluation in v1. Page content reaches them wrapped
  as untrusted data.

## Consequences

- No native webview and no new Tauri capability.
- The browser works in headless mode and in cloud containers.
- Without an installed Chromium the panel says how to install one.
- Linux first, like the servers (ADR 0008).
- A new crate, `crates/browser`, with a permissive dependency.
