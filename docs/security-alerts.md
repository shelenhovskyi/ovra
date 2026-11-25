# Security alerts

Once a week an agent goes through every security finding for this repository,
fixes what it can on `main` and records here what it accepts. A finding that
is neither fixed nor listed below is a new one.

## Sources

| Source                        | Where the agent reads it                                                                                                            |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| CodeQL (Rust, JS/TS, Actions) | Log of the "List findings" step of the latest scheduled `Security` run. Agents cannot read the code scanning API.                   |
| OpenSSF Scorecard             | `https://api.securityscorecards.dev/projects/github.com/shelenhovskyi/ovra` (published results, refreshed on every push to `main`). |
| RustSec advisories            | `cargo deny check advisories` (`unmaintained` and `unsound` count anywhere in the graph).                                           |
| npm advisories                | `pnpm audit --prod --audit-level high`.                                                                                             |
| Workflow audit                | zizmor job in `Security`.                                                                                                           |

Dependency updates come from Renovate (`renovate.json`). Dependabot alerts are
off; Scorecard's Vulnerabilities check (OSV over `Cargo.lock` and
`pnpm-lock.yaml`) covers the same advisories.

## How a pass works

1. Read every source above.
2. Fix each finding that has a fix: update the dependency, change the code or
   the workflow. Commit with Conventional Commits and push to `main` after
   `just test && just e2e && just lint`.
3. A finding with no fix, or a false positive, goes in the table below with
   the reason and the condition for revisiting it. A RustSec one also goes in
   `deny.toml` `[advisories] ignore` with the same reason.
4. Remove rows whose finding is gone. `cargo deny` warns about an ignore that
   no longer matches.
5. Ask the owner only when a fix would weaken security or needs an
   architecture change (see `CLAUDE.md`).

## Accepted findings

| Finding                                                                               | Source             | Why it is accepted                                                                                                                                                                                       | Revisit when                                             |
| ------------------------------------------------------------------------------------- | ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| RUSTSEC-2024-0429, `glib` 0.18 unsound `VariantStrIter`                               | RustSec, Scorecard | Comes from Tauri's GTK 3 stack on Linux. Ovra never calls `VariantStrIter`.                                                                                                                              | Tauri moves to gtk-rs 0.20 or later.                     |
| RUSTSEC-2024-0370, `proc-macro-error` unmaintained                                    | RustSec, Scorecard | Build-time macro through `glib-macros` 0.18 (same stack).                                                                                                                                                | Same as above.                                           |
| RUSTSEC-2026-0370/0371/0372, `zvariant`, `zbus_names`, `zvariant_derive` unmaintained | RustSec, Scorecard | Merged into `zbus` 6. They come from `zbus` 5 in `zbus-secret-service-keyring-store` 1.0.1 (`ovra-secrets`, Linux), its latest release.                                                                  | The keyring store moves to `zbus` 6.                     |
| RUSTSEC-2026-0543, `chrono` unmaintained                                              | Scorecard          | False positive: `chrono` is only an optional dependency of `serde_with` that nothing enables. It is in `Cargo.lock` but never compiled (`cargo tree --workspace -i chrono --target all` prints nothing). | It shows up in `cargo tree`.                             |
| Pinned-Dependencies: `npm install -g pnpm@10` in `scripts/cloud-setup.sh`             | Scorecard          | Fallback for containers without corepack; dev setup only, never in CI or releases. npm cannot pin a global install by hash.                                                                              | The script runs in CI or a release build.                |
| Code-Review, Branch-Protection, Maintained, Contributors, Fuzzing, CII-Best-Practices | Scorecard          | Project setup, not code: one owner, work lands on `main` without pull requests, the repository is new.                                                                                                   | Pull requests, external contributors or a first release. |
