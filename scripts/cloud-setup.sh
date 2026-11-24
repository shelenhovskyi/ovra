#!/usr/bin/env bash
# Prepares a Linux (Ubuntu 24.04) cloud container or CI runner for Ovra work.
# Idempotent. Usage: scripts/cloud-setup.sh
set -euo pipefail

SUDO=""
if [ "$(id -u)" -ne 0 ]; then SUDO="sudo"; fi

if command -v apt-get >/dev/null; then
  $SUDO apt-get update -q
  DEBIAN_FRONTEND=noninteractive $SUDO apt-get install -y -q \
    build-essential pkg-config libssl-dev \
    libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev \
    bubblewrap ripgrep xvfb
fi

command -v just >/dev/null || cargo install just --locked
command -v cargo-nextest >/dev/null || cargo install cargo-nextest --locked
command -v cargo-deny >/dev/null || cargo install cargo-deny --locked
command -v cargo-machete >/dev/null || cargo install cargo-machete@0.9.1 --locked

corepack enable >/dev/null 2>&1 || npm install -g pnpm@10
pnpm install --frozen-lockfile

# Browsers: reuse a preinstalled Chromium when present, else install one.
if [ ! -d /opt/pw-browsers ]; then
  pnpm --filter @ovra/e2e exec playwright install --with-deps chromium
fi

# Sandbox prerequisites (phase 4) - report, do not fail.
if unshare -Ur true 2>/dev/null; then echo "user namespaces: ok"; else echo "user namespaces: unavailable (sandbox tests run in CI)"; fi
