# Security policy

Ovra is pre-alpha software. There are no releases yet, so only the latest
commit on `main` is supported.

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub:
**Security → [Report a vulnerability](https://github.com/shelenhovskyi/ovra/security/advisories/new)**.

Do not open a public issue, pull request or discussion for a vulnerability.

- You will get a first answer within 7 days.
- We aim to fix confirmed issues and publish an advisory within 90 days of the
  report, sooner when a fix is simple. We will agree the disclosure date with
  you and credit you unless you prefer otherwise.
- There is no bug bounty.

We will not take legal action against good-faith research that respects this
policy, avoids harm to other people's data and gives us a chance to fix the
problem before it is disclosed.

## Security model

Knowing what Ovra does and does not promise today helps you decide what is a
vulnerability.

What Ovra protects today:

- **Headless mode** (`ovra-daemon`) binds to `127.0.0.1` only, generates a
  random token per launch and rejects WebSocket connections without the token
  or with a foreign `Origin` ([ADR 0003](docs/adr/0003-headless-mode-security.md)).
  Reaching the core from another web page or another machine without the token
  is a vulnerability.
- **Project paths**: every path from the UI is checked before it reaches a
  service (`crates/core/src/path.rs`). Reading or writing outside an open
  project through the protocol is a vulnerability.
- **Credentials**: Ovra never reads, stores or sends your Claude credentials.
  Agents run through your own installed `claude` CLI, which handles sign-in
  ([ADR 0006](docs/adr/0006-claude-via-installed-cli.md)).

What Ovra does not protect yet:

- **Agents are not sandboxed.** An agent session runs with your user's
  permissions, in a git worktree or in your checkout. Tool calls go through
  Ovra's approval flow, but anything you approve runs unrestricted. Sandboxing
  is planned for a later phase.
- **The terminal** runs your shell with your permissions, by design.
- **Repository content is trusted** as much as you trust the repository you
  open. Opening a malicious repository and approving what an agent asks is not
  an Ovra vulnerability, although we want to hear about ways Ovra makes that
  easier than it should be.

Not vulnerabilities: issues that need an attacker who already runs code as your
user, reports from automated scanners without a working proof of concept, and
missing hardening headers on the loopback-only headless server.
