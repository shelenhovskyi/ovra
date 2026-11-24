# Ovra

A lightweight, agent-first IDE. Agents do the work in isolated git worktrees;
people set tasks, watch progress and approve results. Rust core, Tauri 2 shell,
Svelte 5 UI, and a headless mode that runs the same UI in a regular browser.

Status: phase 0 (scaffold).

## Getting started

```sh
scripts/cloud-setup.sh   # Linux deps, just, cargo tools, pnpm install
just check               # fast checks
just test                # unit tests
just e2e                 # Playwright against headless mode
just dev-headless        # open the printed OVRA_URL in a browser
just dev                 # desktop app (needs a display)
```

See `docs/architecture.md` and `CLAUDE.md`.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
