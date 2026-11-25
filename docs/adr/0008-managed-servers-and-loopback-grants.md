# 0008. Managed servers in sandbox pods, reached through loopback grants

- Status: proposed
- Date: 2026-11-25

## Context

The owner wants dev servers that Ovra runs for the person and for agents,
and agents that test against them on localhost. Today the netproxy refuses
loopback, because the machine's loopback holds Ovra's own daemon, Docker,
databases and debug ports, and a port number says nothing about who
listens on it (research, 2026-11-25: Cursor CVE-2026-61613, Docker
CVE-2025-9074, DNS rebinding against Ollama and dev servers). The owner
approved loosening the netproxy for this feature only, kept minimal and
audited.

## Decision

- Servers run sandboxed, one pod (own network and PID namespaces) per work
  dir, defined by the person in `.ovra/servers.json`, a protected path.
- A pod listens on nothing on the host. A helper exposes each defined port
  as a Unix socket in a folder Ovra owns.
- A session's netproxy reaches `localhost:P` only through a grant the core
  holds while a server of the session's own work dir is ready on `P`, and
  only by connecting to that pod's socket. Every other loopback and private
  address stays refused. The exact rule is in
  `docs/specs/servers-and-browser.md`.
- A forwarder in the agent's sandbox makes `localhost:P` work for clients
  that skip the proxy, through the same rule.
- Only the person can open a server on the host's loopback.

## Consequences

- Agents never see the host's loopback, so the daemon and other local
  services stay out of reach.
- Two worktrees run the same server on the same port without a clash.
- Servers exist only where a sandbox does (Linux first). Elsewhere
  `servers.*` answers `unavailable`.
- A new crate, `crates/servers`, and new `SandboxProfile` fields.
- A grant lets an agent send anything to its own server, which runs project
  code it can edit anyway.
