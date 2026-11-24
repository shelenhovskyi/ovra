# Governance

Ovra is a young project with one maintainer,
[@shelenhovskyi](https://github.com/shelenhovskyi), who has the final say
(a "benevolent dictator" model).

- **Decisions** that shape the project are recorded as architecture decision
  records in [`docs/adr/`](docs/adr/). Anyone may propose a decision in
  [Discussions](https://github.com/shelenhovskyi/ovra/discussions); the
  maintainer accepts or rejects it.
- **Features** larger than a day of work start with a short spec in
  [`docs/specs/`](docs/specs/) that the maintainer approves.
- **Security-sensitive code** (CSP, Tauri capabilities, sandbox, secrets,
  network proxy, extension permissions) always needs the maintainer's review,
  as listed in [`.github/CODEOWNERS`](.github/CODEOWNERS).
- **Conduct** issues are handled by the maintainer under the
  [Code of Conduct](CODE_OF_CONDUCT.md).

When other people contribute regularly, we will add maintainers and write down
how they are chosen and how decisions are made between them.
