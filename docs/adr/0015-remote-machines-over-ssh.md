# 0015. Remote machines over the person's own SSH

- Status: proposed (owner approved the feature on 2026-11-27; the spec
  waits for approval)
- Date: 2026-11-27
- Amends: ADR 0003 (a second, tokenless entry, `ovra-daemon --stdio`,
  whose authentication is the remote machine's sshd)

## Context

The owner wants agents to run on another machine, such as a powerful
server, while Ovra on the laptop shows and steers them. ADR 0002 keeps all
logic in the core and makes the UI a replaceable layer over one frame
protocol; headless mode (ADR 0003) already runs a whole core without a
window. ADR 0006 keeps Claude credentials in Claude Code's own flow, and
ADR 0010 uses the person's own `gh` for GitHub. The sandbox (phase 4) and
the netproxy are built for Linux first.

Two shapes were considered:

- **Agent only on the remote machine**: the local core starts
  `ssh host claude -p …`. Files, git, worktrees, the sandbox and the
  netproxy would be on the laptop while the agent edits files elsewhere, so
  every service would need a remote twin. Rejected.
- **The whole core on the remote machine** (the model of VS Code Remote
  SSH): the laptop is only a window. Every service, the sandbox and the
  netproxy run next to the files. Chosen.

## Decision

- The remote machine runs `ovra-daemon --stdio`: one client, frames as JSON
  lines over stdin and stdout, no listener, no served UI, no phone
  companion, no opener. Remote agents require the sandbox to be available.
- The laptop reaches it only through the person's own OpenSSH `ssh`, with a
  fixed command line that turns off agent, X11, port and tunnel forwarding,
  shared masters and local commands, checked again with `ssh -G` before
  each connection. Host key checks cannot be widened by the person's config
  (`KnownHostsCommand`, SSHFP and GSSAPI key exchange are off). SSH
  authentication is the only authentication. Ovra never sees passwords,
  passphrases or private keys, and never forwards the SSH agent or
  Kerberos tickets.
- Host keys are pinned per machine in Ovra's own `known_hosts` file. First
  trust stays with OpenSSH: Ovra copies keys the person's own
  `known_hosts` already has, after showing their fingerprints, and never
  accepts a key itself. A changed key refuses the connection.
- Every frame from a remote machine is untrusted. Each remote machine
  opens in its own window with Ovra's local UI assets, its own webview
  storage, a capability that reaches no local files, and an "on <name>"
  bar the app draws. The app routes calls and events by window label, and
  checks links and notifications itself. A frame bridge in `crates/ssh`
  checks and caps every frame.
- The headless daemon gets the bridge only in e2e builds, so the headless
  token gives no reach to the person's machines.
- Claude, `gh` and keys are signed in or set up on the remote machine
  itself. Nothing of the laptop's credentials, keys (secrets) or Claude
  login goes there.
- v1 needs the same Ovra build on both sides and the person installs
  `ovra-daemon` on the remote machine. Keeping the remote core alive while
  the laptop is away, and copying Ovra's own daemon there, are later steps
  with their own approval.

## Consequences

- One new crate, `crates/ssh` (security code), a `RemoteHosts` service
  trait and an `ssh.*` protocol domain on the laptop's core only.
- Remote machines must be Linux with a working sandbox in v1.
- When the connection drops, or no frame arrives for 60 s, the remote
  agents stop (the person can resume them), until the attach and detach
  step lands.
- Agent limits, billing and the `ANTHROPIC_API_KEY` warning are per
  machine, so with several machines the person can run one limit's worth
  on each.
- The person's `~/.ssh/config` still applies for host names, users, ports,
  identities and jump hosts; a `ProxyCommand` there runs as they set it.
- Whoever controls the remote account controls the agents there, as with
  any SSH login. A machine trusted at first use is trusted.
- The threat model is in `docs/specs/ssh-remotes.md`.
