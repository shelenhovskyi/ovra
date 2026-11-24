# Contributing to Ovra

Thanks for your interest. Ovra is pre-alpha: things change quickly and nothing
is stable yet. That also means feedback now has a big effect.

## Two ways work lands

- **The maintainer's agents** follow [`CLAUDE.md`](CLAUDE.md) and, for now,
  push straight to `main` after the full test suite passes. See
  [AI_POLICY.md](AI_POLICY.md#how-ovra-itself-is-built) for how that works.
- **Everyone else** works on a fork and opens a pull request. `CLAUDE.md`
  is written for agents. Its rules on code (layout, tests, Svelte 5 style)
  apply to you too, but its "push to `main`" rule does not.

## What we welcome

- **Bug reports** with steps to reproduce: use the bug form in
  [Issues](https://github.com/shelenhovskyi/ovra/issues/new/choose).
- **Bug fixes, tests and documentation**: just open a pull request.
- **Issues labelled `good first issue` or `help wanted`**: comment that you
  are taking one, then open a pull request.
- **Ideas and questions**: start a thread in
  [Discussions](https://github.com/shelenhovskyi/ovra/discussions).

**Please talk to us before building a feature.** Open an idea in Discussions
first; once it is accepted it becomes an issue you can work on. Feature pull
requests without prior agreement are usually closed, however good the code,
because the design may not fit the plan.

We will probably not merge:

- large refactorings or style-only changes nobody asked for;
- behaviour changes without tests;
- changes that cross crate boundaries or architecture decisions in
  [`docs/adr/`](docs/adr/) without a new or updated ADR;
- new dependencies under copyleft licenses;
- translations: the UI is English only for now.

Issues labelled `needs-design` or `needs-decision` are not ready for code yet;
pull requests for them wait until the decision is made.

## Setting up

You need Rust (the version in `rust-toolchain.toml` is installed
automatically by `rustup`), Node.js 22, pnpm, [`just`](https://just.systems)
and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for
your OS. On Ubuntu, `scripts/cloud-setup.sh` installs everything.

```sh
pnpm install
just dev-headless   # core + UI in your browser: open the printed OVRA_URL
just dev            # the desktop app
```

Read [`docs/architecture.md`](docs/architecture.md) (one page) and
[`docs/glossary.md`](docs/glossary.md) before changing code.

## Before you open a pull request

```sh
just test && just e2e && just lint
```

CI runs the same commands. It also checks rules that are easy to miss:

- **English only.** Code, comments and docs; CI fails on Cyrillic.
- **Conventional Commits**, for example `fix(editor): keep the cursor on save`.
- **Crate boundaries.** Allowed dependencies between crates are listed in
  `scripts/crate-graph.json`.
- **Fast tests.** Every test has a time budget (`scripts/test-budgets.json`);
  use fake clocks and fakes, never `sleep`, real agents or the network.
- **Small changes.** Aim for under about 400 changed lines per pull request.

Fill in the pull request template: Summary, Assumptions, Testing, and whether
you used AI tools ([AI_POLICY.md](AI_POLICY.md)).

## Developer Certificate of Origin

Every commit in a pull request must be signed off:

```sh
git commit -s -m "fix(tree): ..."
```

This adds `Signed-off-by: Your Name <you@example.com>` and means you agree to
the [Developer Certificate of Origin](https://developercertificate.org/):
you wrote the change, or otherwise have the right to submit it under the
project's license (Apache-2.0). There is no CLA. Only a person can sign off:
an AI tool cannot.

## What to expect

- We aim to answer issues and pull requests within a week.
- Reviews may ask for changes; that is normal and not a rejection.
- Please follow the [Code of Conduct](CODE_OF_CONDUCT.md).
- Security problems go through [SECURITY.md](SECURITY.md), never a public
  issue.
