# Agents on a remote machine over SSH

Status: draft, waiting for the owner's approval. Architecture: ADR 0016.
Security code: every step that touches `crates/ssh`, the SSH command line,
the host key pins, the frame bridge or the remote window's capabilities
gets an independent security review before it lands. The first review
(2026-11-27, approve with changes) is folded in.

## Goal

A person keeps a project on a more powerful machine (a workstation, a cloud
VM, a build server) and works with it from Ovra on their laptop: browse and
edit files, run agents, review and keep their changes, open terminals. The
agents, their sandbox, their netproxy, git and the language servers all run
on the remote machine. The laptop only shows the UI.

Not in scope: a local project whose agents run elsewhere (that needs file
sync), several machines in one window, Windows or macOS as the remote
machine, installing anything on the remote machine for the person, and
password or passphrase prompts.

## How it works

```
 Laptop                                             Remote machine (Linux)
 Ovra window "on build-box"                         ovra-daemon --stdio
   UI (local assets) --IPC--> app --stdio--> ssh ====> sshd --> core, agents,
                              bridge                           sandbox, netproxy
```

- The remote machine runs the same core as headless mode, as
  `ovra-daemon --stdio`: one client, frames as JSON lines on stdin and
  stdout, logs on stderr, no listening socket, no served UI.
- The laptop starts the person's own `ssh` (OpenSSH) with a fixed, hardened
  command line (below) and the remote command `ovra-daemon --stdio`. The
  SSH login is the only authentication: there is no token and no port.
- `crates/ssh` spawns `ssh` and turns its stdio into a stream of frames.
  The app bridges that stream to one window. The window loads Ovra's
  **local** UI assets; the remote machine never sends HTML or scripts.
- Each remote machine opens in its own window. The title bar always shows
  "on <name>", and the window has its own colour accent so it cannot be
  mistaken for a local one.

**Principle: every frame from a remote machine is attacker-controlled.**
Anything that protects the laptop is enforced by the app (Rust) or the UI,
never by the remote core: links, notifications, status, storage, drops.
The remote core's own filters (policies, LSP filtering, netproxy) protect
the remote machine only.

## Steps

Each lands on its own, in this order.

1. **Stub**: `crates/ssh` (Cargo.toml, `lib.rs` with the `RemoteHosts`
   trait, a fake, one test), protocol module `ssh.rs` on both sides, edges
   `daemon -> ssh` and `app -> ssh` in `scripts/crate-graph.json`
   (`crates/ssh` depends on `ovra-core` only).
2. **`ovra-daemon --stdio`**: the remote side (below).
3. **Hosts and pins**: `ssh.*` methods, the pinned `known_hosts`, version
   check, the hardened command line, with a fake `ssh` in tests.
4. **Bridge and remote windows** in the app, plus the e2e-only bridge in
   the daemon.
5. **UI**: "Connect to remote machine…" in the File menu and the palette,
   the add-machine dialog, the "on <name>" bar, connection status and
   reconnect.
6. **Later, separate approval**: keep the remote core running while the
   laptop is away (attach and detach), and copying Ovra's own daemon to the
   remote machine.

## 2. The remote side: `ovra-daemon --stdio`

- Reads one frame per line from stdin, writes one per line to stdout.
  A line over 16 MiB or one that is not a frame closes the connection.
- The first line it writes is a hello frame, a new frame shape defined in
  `crates/transport` and `ui/src/lib/protocol/` together and valid only as
  the first line: `{"hello": {"version": "<crate version>", "protocol":
  <n>, "os": "linux", "sandbox": <status>}}`. The laptop refuses any
  version other than its own (no releases yet, so both sides are the same
  build).
- Wires the same services as headless mode, except: no `Opener` (opening
  files in the remote machine's apps makes no sense), no `RemoteAccess`
  (the phone companion never listens on the remote machine's network), no
  `KeepAwake`.
- Nothing that runs code for agents starts without the sandbox: when
  `Sandbox::detect` is not `Available`, agents (research sessions
  included), language servers, dev servers and the agent browser answer
  `unavailable` with the reason. Terminals still work (they are the
  person's own shell, as `ssh` would give them anyway). Running any of
  these unsandboxed on a remote machine is a follow-up that needs the owner.
- **Liveness.** A laptop that sleeps leaves stdin open until TCP gives up,
  often hours later. So the laptop sends a ping frame every 15 s, and the
  core stops every session as on quit after 60 s without any frame, or at
  once when stdin closes. Sessions and transcripts stay in the remote
  machine's store and can be resumed after reconnecting, only by the
  person; a reconnect never resumes anything on its own. Step 6 changes
  this.
- **One core per data directory.** `--stdio` takes an exclusive lock on
  the data directory. A second `--stdio` (a reconnect while the old one has
  not timed out) asks the first to stop its sessions and quit, waits up to
  10 s for the lock, and otherwise answers `busy`. The laptop keeps at most
  one link per machine.
- Data stays in the remote machine's own data directory
  (`~/.local/share/ovra`), created or checked as 0700 and owned by the
  user, so its projects, sessions, policies and review state belong to that
  machine.
- Limits and billing are per machine: each core has its own agent limit
  (Settings > Agents on that machine) and shows where its usage is billed,
  with the `ANTHROPIC_API_KEY` warning, from its own `claude`.

## 3. Hosts and pins (laptop side)

### Protocol (`ssh.*`, laptop core only)

`ssh.*` is not on the phone companion allowlist (ADR 0014), and the bridge
drops every `ssh.*` frame that comes from a remote machine.

- `ssh.list` -> `[RemoteHost]`: `id`, `name`, `destination`, `daemon`
  (path of `ovra-daemon` on the remote machine, default `ovra-daemon`),
  `fingerprints` (SHA256 strings of the pinned keys), `last_connected`,
  and the `ssh` binary in use.
- `ssh.prepare { destination }` -> what `ssh -G` resolves (host name,
  port, user, host key alias) and the keys already in the person's own
  `known_hosts` for it, with fingerprints. It makes no SSH connection,
  though the person's config can run commands or DNS lookups while it is
  read (`Match exec`, `CanonicalizeHostname`).
- `ssh.add { name, destination, daemon, fingerprints }`: pins exactly the
  keys `ssh.prepare` returned and the person saw. Refuses when the person's
  `known_hosts` no longer has those keys. `name` is 1 to 40 printable
  characters, no control or bidi characters.
- `ssh.remove { id }`, `ssh.rename { id, name }`.
- `ssh.connect { id }` opens the machine's window, or focuses it when a
  link is already open; `ssh.disconnect { id }`.
- Event `ssh.status { id, state, reason }` with `connecting`, `open`,
  `closed`, `failed` and a `reason`: `host_key_changed`, `no_pin`,
  `auth_failed`, `no_daemon`, `version_mismatch`, `busy`, `no_ssh`,
  `unsafe_config`, `exited`. It reaches the remote window on an app-only
  channel, never in-band, so a remote machine cannot fake its status or
  name.
- Hosts are kept in `<data>/ssh/hosts.json` (0600, atomic writes), pins in
  `<data>/ssh/<id>/known_hosts` (0600).

### Trust on first use stays with OpenSSH

Ovra never accepts a host key itself and never runs `ssh-keyscan`.

- Ovra looks the keys up under the name `ssh` would check: the
  `hostkeyalias` from `ssh -G` if set, else `hostname`, or `[hostname]:port`
  for a port other than 22, in the `userknownhostsfile` files `ssh -G`
  names, with `ssh-keygen -F` (which understands hashed hosts, the
  Debian and Ubuntu default). Keys marked `@revoked` are dropped; a host
  known only through `@cert-authority` is refused in v1.
- If keys are found, the dialog shows their fingerprints: "Your SSH already
  trusts this machine with key SHA256:…". The person adds the machine;
  Ovra writes normalised lines `ovra-<id> <type> <key>` into the machine's
  own pin file.
- If not, the dialog says: "Connect once in a terminal with
  `ssh <destination>` and check the fingerprint it shows, then try again."
  That keeps the one risky step in OpenSSH's own flow, the same way Claude
  sign-in stays in Claude Code's.
- Every connection uses only the pin file, with `HostKeyAlias=ovra-<id>`.
  A later change to the person's `known_hosts` (a new key, a revocation)
  does not change what Ovra trusts; re-pinning is the answer. A changed key
  fails with `host_key_changed`; the window shows the pinned fingerprint
  and says how to check; the person removes and adds the machine again.

### The `ssh` binary and its environment

- `ssh` is resolved once to an absolute path from `PATH`, skipping empty
  and relative entries, checked to be OpenSSH 8.7 or later (`ssh -V`,
  which every option below needs), and shown in Settings > Remote
  machines. Older or missing answers `no_ssh` with the reason.
- The `ssh` process gets only a short list owned by `crates/ssh`: `PATH`,
  `HOME`, `USER`, `LOGNAME`, `LANG`, `LC_*`, `SSH_AUTH_SOCK` (so the local
  agent can authenticate; it is never forwarded) and `XDG_RUNTIME_DIR`
  (where `IdentityAgent` sockets of gpg-agent or 1Password live). No proxy
  variables (they can carry passwords) and nothing else, so `SendEnv`,
  which adds up and cannot be cleared from the command line, has nothing
  secret to send. Assumption: a `ProxyCommand` that needs more (AWS SSM,
  gcloud IAP, `KRB5CCNAME`) fails; say so in its error.

### The command line

Arguments, never one string through a shell:

```
ssh -T -x -a
    -o IgnoreUnknown=GSSAPIKeyExchange -o GSSAPIKeyExchange=no
    -o BatchMode=yes -o ConnectTimeout=20
    -o StrictHostKeyChecking=yes -o UpdateHostKeys=no
    -o HostKeyAlias=ovra-<id>
    -o UserKnownHostsFile="<data>/ssh/<id>/known_hosts"
    -o GlobalKnownHostsFile=none -o KnownHostsCommand=none
    -o VerifyHostKeyDNS=no -o CheckHostIP=no
    -o ForwardAgent=no -o ForwardX11=no -o ForwardX11Trusted=no
    -o GSSAPIDelegateCredentials=no -o ClearAllForwardings=yes
    -o Tunnel=no -o PermitLocalCommand=no -o RemoteCommand=none
    -o RequestTTY=no -o SessionType=default -o StdinNull=no
    -o ForkAfterAuthentication=no -o EscapeChar=none
    -o ControlMaster=no -o ControlPath=none
    -o SetEnv=OVRA_UNUSED=1
    -o ServerAliveInterval=15 -o ServerAliveCountMax=4
    -- <destination> <daemon> --stdio
```

- OpenSSH takes the first value it sees for an option and reads the command
  line first, so these win over the person's `~/.ssh/config`. Everything
  else in their config still applies (`HostName`, `User`, `Port`,
  `IdentityFile`, `ProxyJump`): it is their own setup, as with `gh` in
  ADR 0010.
- `KnownHostsCommand`, `VerifyHostKeyDNS` and the distribution patch
  `GSSAPIKeyExchange` can each trust a key that is not in the pin file, so
  all three are off. `GSSAPIDelegateCredentials=no` keeps the person's
  Kerberos ticket on the laptop; Kerberos authentication itself still
  works.
- `SetEnv` keeps only its first value; ours exists only to replace any
  `SetEnv` in the person's config. Nothing reads it.
- The pin path is quoted, because `UserKnownHostsFile` is a list split on
  spaces (macOS data directories have one), and `%` in it is written as
  `%%`. A data directory containing `"` or a newline is refused.
- Before every connection Ovra runs `ssh -G` with the same arguments and
  environment and refuses with `unsafe_config` unless the effective values
  are: `stricthostkeychecking true`, `globalknownhostsfile none`, no
  `knownhostscommand`, `verifyhostkeydns false`, `gssapikeyexchange`
  absent or `no`, `gssapidelegatecredentials no`, `forwardagent no`,
  `forwardx11 no`, `clearallforwardings yes`, no `controlpath`,
  `controlmaster false`, `permitlocalcommand no`, `tunnel false`,
  `sessiontype default`, `stdinnull no`, `forkafterauthentication no`,
  `hostkeyalias ovra-<id>`, and no `localforward`, `remoteforward` or
  `dynamicforward` line. The `userknownhostsfile` check compares the whole
  value against the quoted pin path, and the spaced-path case has its own
  snapshot test. (Checked with OpenSSH 9.6 against a hostile config: all
  of the above win, `SendEnv` from the config stays.)
- `destination` is a host alias or `user@host`. Each part is
  `[A-Za-z0-9._-]+`, at most 64 characters, and does not start with `-` or
  `.` (the host also reaches `%h` in the person's `ProxyCommand`). No
  `:port` and no `ssh://` form: a port goes in `~/.ssh/config`. The `--`
  before it is required: without it, `ssh box -oProxyCommand=…` sets the
  option.
- `daemon` is a bare name, an absolute path or a path starting with `~/`,
  made of `[A-Za-z0-9._/-]+` after that prefix, with no `..` segment and
  not starting with `-`. The remote shell runs it, so no other character is
  allowed.
- No passwords, passphrases or keyboard-interactive prompts
  (`BatchMode=yes`): the key must be in the person's SSH agent or have no
  passphrase. A FIDO key that only needs a touch works.

### Errors and noise

- `ssh` stderr carries the sshd banner and the remote daemon's logs, both
  remote-controlled: it is capped at 64 KiB, kept as plain text and shown
  only in a "Details" fold. `host_key_changed` and `auth_failed` are read
  from an exit of 255 before any stdout byte; anything after the hello is
  `exited`.
- Lines before the hello that are not frames (a remote `.bashrc` that
  prints) are skipped up to 64 KiB and never shown; more fails with
  `no_daemon`.

## 4. Bridge and remote windows

- `crates/ssh` gives `Link`: frames in, frames out, a status channel, and
  `close()`, which ends `ssh` and its process group.
- The bridge forwards client frames to the remote core and server frames
  to the window, nothing else. It checks every server frame against the
  frame shape (`id` + `result`/`error`, or `event`) and the 16 MiB cap,
  drops `ssh.*` events, and closes the link on the first bad frame. The
  queue to the window is bounded; when it fills, the bridge drops events
  and sends `info.resync`, as the WebSocket transport does.
- **Routing by window, never by frame.** Every emit from the app is
  `emit_to(label)`, including the frame event and menu commands, so a
  remote window never sees the local core's events or another remote's.
  `ovra_call` picks its core from the calling window's label: `main` goes
  to the local core, `ssh-<id>` to that link, anything else is refused.
- **Capability.** A remote window has label `ssh-<id>` and its own Tauri
  capability with only `ovra_call`, the frame event and `ovra_open_url`.
  It has no folder dialog, no drops or imports, no notifications of its
  own and no window commands that reach the laptop's files. Its window is
  built with the drag-and-drop handler off, and drop events from any label
  but `main` are ignored. Its `Transport` lacks `pickFolder`, `onDrop` and
  `importDrop`. Its CSP has no `asset:` source.
- **Storage.** Each remote window gets its own webview data store
  (`data_directory` on Linux and Windows, `data_store_identifier` on
  macOS), so drafts, preferences and the last opened project of a remote
  machine never mix with the laptop's. Where that is not available, every
  UI storage key is prefixed with the machine id.
- **Links.** `ovra_open_url` (Rust) opens a URL from a remote window in the
  laptop's browser only if it is `http` or `https` and its host is not
  loopback, private, link-local or `localhost`: those would point at the
  laptop's own services, not the remote machine's. Markdown, transcripts,
  terminal OSC 8 links and LSP `showDocument` all go through it. OSC 52
  clipboard writes stay off; LSP HTML goes through
  `ui/src/editor/lsp/sanitize.ts` as now.
- **Notifications.** The app shows them for a remote window from a
  separate app command that takes a title and body and prepends "<name>:"
  from the host record, never from the frame.
- **Previews.** Dev server previews and the agent browser of a remote
  machine show its screencast only; the window never opens a laptop-side
  `localhost` for them.
- **e2e.** A daemon built with the `fake-agent` feature serves the same
  bridge at `/ws/ssh/<id>` with the token and Origin checks of `/ws`, and
  `--fake-ssh` runs a local `ovra-daemon --stdio` with a fake agent
  instead of `ssh`. Release builds do not have the endpoint (a test checks
  for 404), so the headless token gives no reach to the person's machines.

## 5. UI

- File menu and palette: "Connect to remote machine…". Nothing on the
  start screen (power feature, tucked away).
- Add-machine dialog: Name, SSH destination (as typed in a terminal), path
  of Ovra on the machine (optional), then the fingerprint step above.
  Plain language: "Ovra connects with your own SSH setup. Your SSH keys
  stay on this computer."
- The remote window's title bar: "on <name>", a status dot, and a
  Disconnect action. When the link drops: a bar "Disconnected from <name>.
  Agents on it stop within a minute." with Reconnect.
- The remote window's onboarding checks `claude` on the remote machine, as
  the local one does, and says: "Sign in on <name>: open a terminal here
  and run `claude`." The terminal is the remote core's own.
- Settings > Remote machines: the list, rename, remove, pinned
  fingerprints, the `ssh` in use.

## Threat model

| # | Threat | Answer |
|---|--------|--------|
| T1 | Someone between the laptop and the machine poses as it | Pinned keys under `HostKeyAlias=ovra-<id>`, `StrictHostKeyChecking=yes`, Ovra's own pin file; `KnownHostsCommand`, SSHFP (`VerifyHostKeyDNS`) and GSSAPI key exchange off; a changed key refuses. First trust is OpenSSH's own. |
| T2 | A compromised remote machine attacks the laptop | Every remote frame is untrusted. No agent, X11, port or tunnel forwarding. Frames go only to that window, checked and capped. Local UI assets only, own webview storage. The window's capability reaches no local files; links, notifications and status are checked by the app. |
| T3 | Local credentials leak to the remote machine | The SSH agent and Kerberos tickets are never forwarded; `ssh` gets only a short variable list, so `SendEnv` has nothing secret to send; Ovra's keys (secrets), Claude credentials and `gh` login are never sent. Claude and `gh` sign in on the remote machine itself. |
| T4 | A crafted destination or path injects options or commands | Strict character sets per part, `--` before the destination, a fixed remote command, arguments without a local shell, a quoted pin path. |
| T5 | The person's `~/.ssh/config` turns on forwarding, a shared master, or changes the session | Command-line options win, checked again with `ssh -G`; `ControlPath=none` avoids reusing a master that has forwarding. |
| T6 | Other users on the remote machine reach the core | `--stdio` has no listener; the data directory is 0700. Step 6 must keep that (a 0700 socket with a peer uid check). |
| T7 | Agents on the remote machine reach the laptop or its network | No reverse channel exists. The remote netproxy refuses loopback and private addresses as before. Links to the laptop's loopback and private hosts are refused. |
| T8 | Agents run unsandboxed on a machine without bubblewrap | Agents, language servers, dev servers and the browser refuse to start there. |
| T9 | A remote machine floods or crashes the UI | 16 MiB line cap, frame checks, bounded queue with resync, capped stderr, close on violation. |
| T10 | Local agents change the pins, the host list or `ssh` | Both files are under the data directory, a protected path; `ssh` is an absolute path. This holds where the laptop's own sandbox is available; on a laptop without one (macOS until verified, Windows) local agents could change them. |
| T11 | The phone companion exposes a remote machine | It never runs in `--stdio`; `ssh.*` is not on its allowlist; a phone sees only the laptop's core. |
| T12 | The person thinks they act locally but act on the remote machine | "on <name>" bar and accent on every remote window, from the app, not the remote. |
| T13 | Agents keep running unattended after the laptop leaves | Ping every 15 s, sessions stop after 60 s without frames; one core per data directory. |
| T14 | The headless token reaches the person's machines | `/ws/ssh/<id>` exists only in e2e builds. |

Remaining risks, documented in ADR 0016: the person's own `ProxyCommand`
or `ProxyJump` runs as they configured it, and jump hosts use the person's
own `known_hosts`, not Ovra's pins (the target's key is still pinned end to
end); a machine the person trusted at first use is trusted; revocations
added to the person's `known_hosts` later are not seen until re-pinning;
whoever controls the remote account controls the agents there, like any SSH
login; with several machines the person can run one agent limit's worth on
each.

## Tests

- `crates/ssh`: the command line as an `insta` snapshot (with a data
  directory that has a space and a `%`); destination and daemon path checks
  (option injection, shell characters, `..`, `-` or `.` at the start of a
  part); the `ssh -G` check against hostile outputs; the environment list;
  a fake `ssh` script that prints a bad frame, an oversized line, a wrong
  version, `.bashrc` noise, a host key error; the link closes and reports
  the right reason.
- Core: `ssh.*` with the fake `RemoteHosts`; pins refused when
  `known_hosts` changed between prepare and add; hashed, `[host]:port`,
  `@revoked` and `@cert-authority` lines.
- Daemon: `--stdio` hello, a round trip, agents, language servers and dev
  servers refused without a sandbox, sessions stopped when stdin closes and
  after 60 s without frames (fake clock), a second `--stdio` takes over or
  answers `busy`, no `remote.*`, `/ws/ssh` answers 404 without the e2e
  feature.
- App: the remote capability file has only the listed commands (repo rule
  test); a local event never reaches `ssh-<id>`; `ovra_open_url` refuses
  loopback, private and non-http URLs.
- e2e: connect through the fake `ssh`, open a project on the "remote",
  start a fake agent, disconnect, see the bar; a remote project with the
  same id as a local one shows no local draft.
- Attack tests (`tests/security`): a destination of `-oProxyCommand=…`, a
  `~/.ssh/config` with `ForwardAgent yes`, `ControlMaster auto`,
  `KnownHostsCommand`, `VerifyHostKeyDNS yes`, `GSSAPIKeyExchange yes`,
  `GSSAPIDelegateCredentials yes` and `SessionType none`, and a remote that
  sends `ssh.*` events or frames meant for another window.
