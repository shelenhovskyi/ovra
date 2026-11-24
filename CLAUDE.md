# Ovra

Ovra is a lightweight, agent-first IDE: a Rust core, a thin Tauri 2 shell and a
Svelte 5 UI. People set tasks and approve results; agents do the work.
Read `docs/architecture.md` (one page) and `docs/glossary.md` before changing
code, and the ADRs in `docs/adr/` that touch the area you change.

Always write repository content in English, even when the user writes in
another language. Only `locales/` may contain other languages; CI fails on
Cyrillic anywhere else.

## Commands

Use `just`; do not invent other commands.

- `just check` - fast checks (fmt, clippy, lints, repo rules). Run often.
- `just test` - Rust and UI unit tests.
- `just e2e` - builds headless mode and runs Playwright against it.
- `just lint` - everything CI lints, including cargo-deny and knip.
- `just dev-headless` - core + UI in a browser, no Tauri.

Before every push: `just test && just e2e && just lint` must pass.

## Layout and boundaries

- `crates/core` - protocol (`src/protocol/`, one module per domain), request
  handling, path validation (`src/path.rs`) and the service traits
  (`src/services.rs`) with in-memory fakes (`src/fakes.rs`, `fakes` feature).
  No UI, no OS APIs, no Tauri, no axum.
- `crates/fs-index` - `FsService`: listings, `.gitignore`, git status, watching.
- `crates/store` - `ProjectStore`: the project list, persisted.
- `crates/opener` - `Opener`: hands files to other applications.
- `crates/transport` - JSON frames and the WebSocket endpoint (`ws` feature).
- `crates/daemon` - headless binary: wires services into the core, serves
  WebSocket + static UI.
- `app/` - Tauri shell: wires services, windows and IPC glue only, no logic.
- `ui/` - Svelte 5 front end. It talks to the core only through
  `ui/src/lib/transport.ts` and never knows which OS it runs on. One folder per
  area (`shell`, `tree`, `viewers`, `editor`, `palette`, `keys`); panels get the
  transport and workspace state from `useOvra()` (`ui/src/shell/context.ts`).
- `tests/e2e` - Playwright against headless mode.

A new OS-facing feature is a trait in `crates/core/src/services.rs`, a fake in
`fakes.rs`, and a real implementation in its own crate wired in by `daemon`
and `app`. The core never calls the OS itself.

Allowed crate dependencies are listed in `scripts/crate-graph.json`; CI
rejects any other edge. Platform crates (`sandbox-macos`, `sandbox-windows`,
when they exist) are changed only for issues labelled `platform:*`.

## Parallel work

Several agents work at once, each in its own git worktree, on one task that
owns one area (a crate, or one folder under `ui/src/`). To avoid conflicts:

- Change only the area your task owns. Need something from another area?
  Use its public API; if that API is missing, stop and say so instead of
  editing the other area.
- Shared hot spots change only in their own small task that lands first:
  root `Cargo.toml` (workspace deps and members), `scripts/crate-graph.json`,
  the protocol (`crates/core/src/protocol/`, `ui/src/lib/protocol/`), the
  service traits (`crates/core/src/services.rs`), the workspace state
  (`ui/src/shell/workspace.svelte.ts`), the command types
  (`ui/src/keys/commands.ts`), CI workflows.
- The protocol grows by domain: one module per domain (for example
  `protocol/fs.rs`, `protocol/agents.rs`) so parallel tasks touch different files.
- A new crate starts as a stub (Cargo.toml, `lib.rs` with the trait, a fake,
  one test) landed on `main` before its implementation work starts.
- A dependency used by one crate goes in that crate's `Cargo.toml` (or the
  area's entry in `ui/package.json`), not in `[workspace.dependencies]`.
  Lockfile conflicts (`Cargo.lock`, `pnpm-lock.yaml`): take `main`'s version
  and regenerate with `cargo check` / `pnpm install`.
- Rebase on `main` before pushing; re-run `just test` after a rebase that
  touched your area's dependencies.

## Rules

- Keep `ui/src/lib/protocol/` in sync with `crates/core/src/protocol/`;
  method and event names are namespaced `<domain>.<name>`.
- Every behaviour change comes with a test; use fake agents and fake sandboxes,
  never real ones, in tests. Use `insta` snapshots for structured output.
- `pub(crate)` by default (`unreachable_pub` is on). No speculative
  abstractions, flags or dead code.
- Svelte 5 only: runes (`$state`, `$derived`, `$effect`, `$props`) and
  `onclick={...}` attributes. No `export let`, `$:`, stores for local state,
  or `on:click`.
- Small changes: aim for under ~400 changed lines per commit or PR; split larger work.
- Conventional Commits. For now work lands directly on `main` without PRs;
  when PRs are used, their sections are Summary, Assumptions, Testing.
- Security code (policies, sandbox, CSP, Tauri capabilities, extension
  manifests, netproxy, secrets, WIT) needs a separate human review.

## Test speed

The suite must stay fast; slow tests are bugs.

- Budgets per suite and per test are in `scripts/test-budgets.json` (run time,
  compilation excluded). `just test` and `just e2e` fail when one is exceeded
  and always print the five slowest tests. Raising a budget needs a human.
- Hard limits: Rust tests are killed after 3 s (`.config/nextest.toml`),
  Vitest after 1 s, Playwright after 10 s.
- No `sleep` or fixed waits: use fake clocks (`tokio::time::pause`,
  `vi.useFakeTimers`) and wait on conditions.
- Test logic at the lowest level that can catch the bug: unit tests first,
  integration tests for crate seams, e2e only for user-visible flows.
- Use fakes instead of real processes, network or agents. Share expensive
  setup (one daemon per e2e run, not per test).
- When you add tests, check the printed slowest list; if your tests are on it,
  make them faster before pushing.

## When to ask

Work autonomously within approved specs and ADRs. Ask the user only before:

- adding a dependency or changing a public API (crate API, WIT, MCP tool schemas);
- writing a new ADR or deviating from an existing one;
- touching security code (policies, sandbox, CSP, capabilities, extension permissions);
- making a UX or architecture choice that is costly to reverse.
  For anything else, pick the most reasonable option, record it under "Assumptions" in the commit message (or PR), and continue.
  Batch questions into one message, give options and a recommendation, and keep working on independent parts.

Features larger than a day start with a short spec in `docs/specs/` that a
human approves.
