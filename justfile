# Single entry point for humans and agents. `just --list` shows everything.

set windows-shell := ["pwsh", "-NoLogo", "-Command"]

default:
    @just --list

# Fast checks, about a minute: formatting, clippy, repo rules, UI types.
check:
    cargo fmt --all --check
    cargo clippy --workspace --exclude ovra-app --all-targets --all-features -- -D warnings
    scripts/check-english.sh
    python3 scripts/check_crate_graph.py
    pnpm exec biome check .
    pnpm --filter @ovra/ui check

# Rust and UI unit tests.
test:
    cargo nextest run --workspace --exclude ovra-app --all-features --no-tests=pass
    cargo test --workspace --exclude ovra-app --all-features --doc
    pnpm --filter @ovra/ui test
    python3 scripts/check_test_budgets.py rust ui

# Build the UI and the headless daemon.
build-headless:
    pnpm --filter @ovra/ui build
    cargo build -p ovra-daemon --features fake-agent
    cargo build -p ovra-agent-runtime --features fake-claude --bin fake-claude
    cargo build -p ovra-lsp --features fake-server --bin fake-lsp

# End-to-end UI tests against headless mode.
e2e: build-headless
    pnpm --filter @ovra/e2e test
    python3 scripts/check_test_budgets.py e2e

# Phase 1 speed gate: release daemon against a generated 100k-file repository.
# Separate from `just test` / `just e2e`; CI runs it in its own job.
perf:
    pnpm --filter @ovra/ui build
    cargo build --release -p ovra-daemon
    cargo build --release -p ovra-worktree --example bench
    pnpm --filter @ovra/perf test

# Live agent suite: the real, signed-in `claude` through headless mode
# (tests/agents/README.md). Opt-in: it uses the plan's limits and takes minutes.
# `just agents-live record` also rewrites crates/agent-runtime/fixtures.
agents-live mode="check": build-headless
    OVRA_UPDATE_FIXTURES={{ if mode == "record" { "1" } else { "0" } }} pnpm --filter @ovra/agents-live test

# The agent browser against a real Chromium, without the sandbox (opt-in).
# `path` is the browser; empty finds it on PATH.
browser-live path="":
    OVRA_BROWSER_LIVE="{{path}}" cargo test -p ovra-browser --test live -- --ignored --nocapture

# Everything CI lints.
lint: check
    cargo clippy -p ovra-app --all-targets -- -D warnings
    cargo deny check
    cargo machete
    pnpm exec knip

# Core + UI in a regular browser: open the printed OVRA_URL.
dev-headless: build-headless
    cargo run -p ovra-daemon -- --port 7777

# Desktop app with hot reload (needs a display).
dev:
    pnpm exec tauri dev

# Desktop app build without installers.
build-app:
    pnpm exec tauri build --no-bundle

# Format everything.
fmt:
    cargo fmt --all
    pnpm exec biome format --write .

# UX review: run a persona script against headless mode (docs/ux-reviews/README.md).
ux script *args: build-headless
    node tests/e2e/ux/run.ts {{script}} {{args}}

# Project site (site/): build it into site/dist. Not part of `just test`.
site:
    pnpm --filter @ovra/site build

# Retake the site's screenshots from headless mode.
site-screenshots: build-headless
    node tests/e2e/ux/run.ts site/screenshots.ts --out target/ux/site-screenshots
