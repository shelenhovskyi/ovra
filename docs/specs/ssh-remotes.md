# Agents on a remote machine over SSH

Status: approved by the owner on 2026-11-27 as written (agent limit per
machine). Steps 6 and 7 (staying on while the laptop is away, installing
Ovra on the machine) approved by the owner on 2026-11-28. Architecture:
ADR 0016.
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
machine, installing anything other than Ovra's own daemon on the remote
machine, and password or passphrase prompts.

## How it works

```
 Laptop                                             Remote machine (Linux)
 Ovra window "on build-box"                         ovra-daemon --attach
   UI (local assets) --IPC--> app --stdio--> ssh ====> sshd --> (socket)
                              bridge                           ovra-daemon --core:
                                                               core, agents,
                                                               sandbox, netproxy
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
6. **Staying on** (section 6): the remote core keeps running while the
   laptop is away, and the laptop attaches to it again.
7. **Installing Ovra on the machine** (section 7): the laptop copies its
   own `ovra-daemon` there when the person asks.

## 2. The remote side: `ovra-daemon --stdio`

- Reads one frame per line from stdin, writes one per line to stdout.
  A line over 16 MiB or one that is not a frame closes the connection.
- The first line it writes is a hello frame, defined in
  `crates/transport/src/stdio.rs` and valid only as the first line (the
  bridge reads it; the UI never sees it):
  `{"hello": {"version": "<crate version>", "protocol": <n>, "os": "linux"}}`,
  with `"no_sandbox": "<reason>"` when the machine has no working sandbox.
  The laptop refuses any version other than its own (no releases yet, so both sides are the same
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
  often hours later. So the laptop sends the line `{"ping":{}}` every 15 s,
  and the core stops every session as on quit after 60 s without any line,
  when a line takes it more than 60 s to write, or at once when stdin
  closes. Sessions and transcripts stay in the remote
  machine's store and can be resumed after reconnecting, only by the
  person; a reconnect never resumes anything on its own. Step 6 keeps
  the core and its agents running instead (section 6); `--stdio` stays
  as the one-client mode for tests and the e2e bridge.
- **One core per data directory.** `--stdio` takes an exclusive lock on
  `stdio.lock` in the data directory. A second `--stdio` (a reconnect while
  the old one has not timed out) waits up to 10 s for the lock, then writes
  the single line `{"busy":{}}` instead of the hello and exits; the laptop
  shows `busy` ("the last connection is still closing; try again in a
  minute"). With `--attach` a new client replaces the old one instead
  (section 6). The laptop keeps at most one link per machine.
- Data stays in the remote machine's own data directory
  (`~/.local/share/ovra`), created or checked as 0700 and owned by the
  user (the owner is compared with the effective uid, section 6), so its
  projects, sessions, policies and review state belong to that machine.
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
- Opening a machine's window is the app's, not the core's, since only the
  app opens windows: the main window calls the app command
  `ovra_remote_show { id }`, which opens the window or focuses it and
  connects unless a link is open; a remote window calls
  `ovra_remote_reconnect` and `ovra_remote_disconnect` for itself only.
  Headless mode has no such commands (its e2e bridge connects when the
  page opens `/ws/ssh/<id>`).
- The app message `sshStatus { id, state, reason, details }`, sent
  to that window and the main window, has `connecting`, `open`, `closed`
  and `failed` and a `reason`: `host_key_changed`, `no_pin`,
  `auth_failed`, `no_daemon`, `version_mismatch`, `busy`, `replaced`,
  `no_ssh`, `unsafe_config`, `exited`. It is never in-band, so a remote machine
  cannot fake its status or name.
- Hosts are kept in `<data>/ssh/hosts.json` (0600, atomic writes), pins in
  `<data>/ssh/<id>/known_hosts` (0600).

### Trust on first use stays with OpenSSH

Ovra never accepts a host key itself and never runs `ssh-keyscan`.

- Ovra looks the keys up under the name `ssh` would check: the
  `hostkeyalias` from `ssh -G` if set, else `hostname`, or `[hostname]:port`
  for a port other than 22, in the `userknownhostsfile` files `ssh -G`
  names, with `ssh-keygen -F` (which understands hashed hosts, the
  Debian and Ubuntu default). Keys marked `@revoked` are dropped; a host
  known only through `@cert-authority` is refused in v1, and so is a
  config with `RevokedHostKeys` (`ssh-keygen -Q` cannot check keys Ovra
  holds only in memory, so v1 shows none rather than a revoked one). Only
  absolute paths are read: `ssh -G` joins the list with spaces, so a path
  with a space cannot be told from two.
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

- `ssh` and `ssh-keygen` are resolved once to absolute paths from `PATH`,
  skipping empty and relative entries and files without an execute bit,
  checked to be OpenSSH 8.7 or later (`ssh -V`,
  which every option below needs), and shown in Settings > Remote
  machines. Older or missing answers `no_ssh` with the reason.
  Assumption: a macOS app gets the system's short `PATH`, so it finds
  `/usr/bin/ssh` rather than one from Homebrew; that is OpenSSH too.
- `ssh -G` and `ssh-keygen` run in their own process group for at most
  10 s, with their output capped; on timeout the whole group is killed, so
  a command the person's config leaves running (`Match exec "… &"`) cannot
  hold Ovra up. They run off the core's request path. Windows has no process groups here: the call still ends on time, but such a command may outlive it.
- The `ssh` process gets only a short list owned by `crates/ssh`: `PATH`,
  `HOME`, `USER`, `LOGNAME`, `LANG`, `LC_*`, `SSH_AUTH_SOCK` (so the local
  agent can authenticate; it is never forwarded), `XDG_RUNTIME_DIR`
  (where `IdentityAgent` sockets of gpg-agent or 1Password live), and on
  Windows `SYSTEMROOT`, `USERPROFILE`, `HOMEDRIVE` and `HOMEPATH` (how the
  system and Win32-OpenSSH find `~/.ssh`). No proxy
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
    -- <destination> <daemon> --attach
```

(`--attach` since step 6; the install step of section 7 uses the same
arguments with its own fixed remote command in place of `<daemon> --attach`.)

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
- **Routing by window, never by frame.** The app sends nothing as a Tauri
  event: an event reaches every window that listens for it, whatever
  window it was sent to. Each window hands the app its own
  `tauri::ipc::Channel` (`ovra_subscribe`), and frames, menu commands,
  drops and link status go on the one window's channel only, so a remote
  window never sees the local core's events or another remote's, nor the
  main window a remote's. No window may listen to Tauri events.
  `ovra_call` picks its core from the calling window's label: `main` goes
  to the local core, `ssh-<id>` to that link, anything else is refused.
- **Capability.** A remote window has label `ssh-<id>` and its own Tauri
  capability with only `ovra_call`, `ovra_subscribe`, reconnect,
  disconnect and stop of itself, `ovra_open_url` and window dragging (a
  test keeps the list). Installing and restarting are asked for in the main
  window (section 7), never confirmed in the remote webview.
  It has no folder dialog, no drops or imports, no notifications of its
  own and no window commands that reach the laptop's files. Its window is
  built with the drag-and-drop handler off, and drop events from any label
  but `main` are ignored. Its `Transport` lacks `pickFolder`, `onDrop` and
  `importDrop`. It cannot navigate away from Ovra's own UI or open
  windows of its own. The machine's id and name reach it as
  `window.__OVRA_REMOTE__`, set by the app from the host record.
  Assumption: Tauri has one CSP for every window, so a remote window
  keeps the `asset:` source; the asset protocol is not enabled, so it
  serves nothing.
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
  instead of `ssh`. A failure closes the socket with its reason (`busy`, `version_mismatch`, …) as the WebSocket close reason, never as a frame. Release builds do not have the endpoint (a test checks
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
  Agents there keep working." with Reconnect (section 6).
- The remote window's onboarding checks `claude` on the remote machine, as
  the local one does, and says: "Sign in on <name>: open a terminal here
  and run `claude`." The terminal is the remote core's own.
- Settings > Remote machines: the list, rename, remove, pinned
  fingerprints, the `ssh` in use.

## 6. Staying on while the laptop is away

The laptop's remote command becomes `<daemon> --attach` (the same hardened
command line otherwise). The core then lives in a process of its own on the
machine and outlives any one connection.

- **`--core`** runs the core in the background. It wires the same services
  as `--stdio` (section 2): no `Opener`, `RemoteAccess` or `KeepAwake`, and
  the `Required` sandbox (agents, language servers, dev servers and the
  browser answer `unavailable` without one, and the hello carries
  `no_sandbox`). `--attach` and `--core` use the `--stdio` default data
  directory, and `--attach` passes it to `--core` as an absolute
  `--data-dir`.
- **Folders and the socket.** `<data>` and `<data>/run` are opened with
  `O_NOFOLLOW|O_DIRECTORY` and checked with `fstat`: a folder, owned by the
  effective uid, mode set to 0700 on the descriptor; `run/` is made with
  `mkdirat` relative to `<data>`'s descriptor. (This also fixes the owner
  check `--stdio` lacks today.) `--core` takes the data-directory lock that
  `--stdio` takes first, so the two never run on the same data; only while
  holding it does it remove a leftover `run/core.sock` (refused unless
  `lstat` says it is a socket), bind with `umask 077`, and `chmod` it to
  0600. Bind and connect go through `/proc/self/fd/<n>/core.sock`, so no
  path race applies and the 108-byte path limit does not. Every
  connection's peer uid (`SO_PEERCRED`) must be the core's own or it is
  closed unread. No TCP and no other listener.
- **`core.log`** gets the core's logs: opened with `O_NOFOLLOW`, mode 0600,
  cut to its last 1 MiB when it starts and whenever it passes 2 MiB.
- **`--attach`** is what `ssh` runs. It connects to the socket; `ENOENT` and
  `ECONNREFUSED` mean no core, and then it starts `--core` itself, detached
  from the SSH session (its own session via `setsid`), and keeps retrying
  the connect for up to 10 s. A second `--core` started meanwhile fails to
  take the lock and exits, so no start lock of its own is needed. While a
  `--stdio` holds the lock, `--attach` writes `{"busy":{}}`, as `--stdio`
  does. `--core` starts with stdin and stdout on `/dev/null` (not closed,
  so later files never take descriptors 0 to 2), stderr on the log, its
  working folder `$HOME`, and without `SSH_*`, `DISPLAY`, `XAUTHORITY` and
  `OVRA_UNUSED` in its environment. It keeps that environment for its
  life; Restart picks up a new one. Then `--attach` copies bytes between
  its stdin/stdout and the socket and exits when either side closes. It
  reads no frames: framing, the hello and the checks stay in the core and
  on the laptop.
- **One client at a time.** The core serves each client as `--stdio` does
  (hello first, then frames, pings, the 16 MiB cap). It sends the hello to
  a new client and replaces the current one only when the new client sends
  its first line: a client that refuses the hello (another build) or closes
  without a line never disturbs the current one. The replaced client gets
  the exact line `{"replaced":{}}` and is closed. Replacing, not `busy`, is
  what lets a laptop that slept attach again at once; both are the same
  SSH account, so neither is trusted more.
- **In-band lines.** The core recognises `{"ping":{}}` and `{"quit":{}}`
  only as exact lines. The laptop's bridge writes only request frames the
  app serialised itself, plus the pings and the quit it adds, so a window
  can send neither raw lines nor `quit`; `quit` comes only from the app
  command behind "Stop Ovra on <name>". The bridge accepts `{"replaced":{}}`
  only after the hello and only as the exact line: it closes the link with
  reason `replaced`, carried in `sshStatus` from the app with the name from
  the host record, and never forwards it as a frame. After `replaced` the
  laptop does not reconnect on its own, so two laptops never replace each
  other in a loop.
- **Detaching.** When the client closes, stays silent for 60 s or a write
  takes longer than that, the core drops the client and keeps running:
  agents, their sandbox, netproxy, dev servers and terminals go on.
  Requests that need the person (approvals, questions) wait, as they do
  locally while the person is away. Nothing new starts while no client is
  attached: scheduled tasks (ADR 0017) run only while a client is attached,
  and an enabled schedule does not keep the core from its idle exit.
  Events sent while detached are lost; the next client gets an
  `info.resync` right after the hello and the UI reloads what it shows.
- **Ending.** The core exits as on quit (sessions stop and stay resumable):
  15 minutes after the last client left while no agent session, dev server
  or terminal runs; or on the line `{"quit":{}}`, which the laptop sends for
  "Stop Ovra on <name>" (it says that running agents stop). Disconnect and
  closing the window only detach.
- **Versions.** A core left running from an older build answers attach with
  its own hello, and the laptop refuses it as `version_mismatch`. Only then
  the main window offers "Restart Ovra on <name>", behind a confirmation
  that says the agents there stop. It attaches with `--attach --restart`,
  which sends `{"quit":{}}` to the running core (no first line needed),
  waits up to 30 s for its lock and starts the installed build; when the
  lock is still held after 30 s it fails with that reason and starts
  nothing.
- **Limits of staying on.** A machine whose login manager ends a user's
  processes at logout (systemd `KillUserProcesses=yes`) ends the core with
  the SSH session; the next attach starts a fresh one and the sessions are
  resumable as before. Settings > Remote machines says so when the core the
  laptop attaches to started after the last detach while sessions were
  running (the hello carries the core's start time). After the last logout
  logind may also remove `XDG_RUNTIME_DIR` and the session bus, so the
  Secret Service behind keys (secrets) may stop answering while detached:
  that fails closed, and agents that need a key wait or fail as without
  one.
- **Same user.** Any unsandboxed process of that user can connect to the
  socket, replace the laptop and answer pending approvals: Ovra's own
  terminals and anything the person runs there. That is the trust of an SSH
  login to the account, as before; agents in the sandbox cannot (T15).
- UI: the bar after a drop says "Disconnected from <name>. Agents there
  keep working." The chip's menu has Disconnect and "Stop Ovra on <name>".

## 7. Installing Ovra on the machine

When a connection fails with `no_daemon` or `version_mismatch`, the bar
points to the main window, and Settings > Remote machines offers "Install
Ovra on <name>" with a confirmation in the main window. Nothing is copied
without that click.

- **What is copied.** Only this laptop's own build of `ovra-daemon` for
  Linux on the machine's architecture: the app carries
  `remote/ovra-daemon-<arch>-unknown-linux-musl` for `x86_64` and `aarch64`
  as resources, with their SHA-256 fixed at build time. The laptop checks
  the resource against that hash before it uploads. A development build
  reads them from `OVRA_REMOTE_DAEMON_DIR` (`just remote-daemon` builds
  them). When there is none for that architecture, the button says "This
  Ovra has no build for <arch> Linux. Install `ovra-daemon` there
  yourself." Producing these binaries for releases is release work, which
  waits with releases.
- **How.** Each step is one more connection with the same hardened command
  line and a fixed remote command, never built from the person's input:
  1. `uname -sm`: its output is the machine's, so it must be exactly
     `Linux x86_64` or `Linux aarch64`, which map to the two resource
     names; anything else refuses.
  2. The binary on stdin to this one-line command (no newline, so csh
     takes it too), where `<sha256>` is the 64 hex digits fixed at build
     time:

     ```
     sh -c 'umask 077; [ -n "$HOME" ] || exit 1; p="$HOME/.local/share/ovra"; d="$p/bin"; f="$d/ovra-daemon.new"; mkdir -p "$d" && for x in "$p" "$d"; do [ -d "$x" ] && [ ! -L "$x" ] && [ -O "$x" ] && chmod 700 "$x" || exit 1; done; rm -f "$f"; if cat > "$f" && [ "$(sha256sum < "$f" | cut -c1-64)" = "<sha256>" ] && chmod 700 "$f" && mv -f "$f" "$d/ovra-daemon"; then exit 0; fi; rm -f "$f"; exit 1'
     ```

  The command is single-quoted for the person's login shell and runs in
  `sh`, so it works under bash, zsh, fish and csh alike; it refuses a
  `bin` or data folder that is a symlink or someone else's, removes a
  planted `.new` before writing, compares the hash itself (busybox's
  `sha256sum` has no `--status`), and removes the `.new` after a short or
  failed upload, so the old binary stays in place.
- The remote hash check guards against truncation and corruption only: the
  machine is not trusted. What protects the laptop is that the commands
  are fixed and the laptop never runs the uploaded file; the machine runs
  it on the next attach, as it would one the person installed.
- The machine's record then names `~/.local/share/ovra/bin/ovra-daemon`
  (the host record's `daemon` field). A path the person set themselves is
  kept unless they choose Install.
- Uploads are capped at 128 MiB and 5 minutes, with progress shown.
- The app's command `ovra_remote_install` is for the main window only, for
  any machine; a machine's window can only point there.

## Threat model

| #   | Threat                                                                                    | Answer                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| --- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T1  | Someone between the laptop and the machine poses as it                                    | Pinned keys under `HostKeyAlias=ovra-<id>`, `StrictHostKeyChecking=yes`, Ovra's own pin file; `KnownHostsCommand`, SSHFP (`VerifyHostKeyDNS`) and GSSAPI key exchange off; a changed key refuses. First trust is OpenSSH's own.                                                                                                                                                                                                                       |
| T2  | A compromised remote machine attacks the laptop                                           | Every remote frame is untrusted. No agent, X11, port or tunnel forwarding. Frames go only to that window, checked and capped. Local UI assets only, own webview storage. The window's capability reaches no local files; links, notifications and status are checked by the app.                                                                                                                                                                      |
| T3  | Local credentials leak to the remote machine                                              | The SSH agent and Kerberos tickets are never forwarded; `ssh` gets only a short variable list, so `SendEnv` has nothing secret to send; Ovra's keys (secrets), Claude credentials and `gh` login are never sent. Claude and `gh` sign in on the remote machine itself.                                                                                                                                                                                |
| T4  | A crafted destination or path injects options or commands                                 | Strict character sets per part, `--` before the destination, a fixed remote command, arguments without a local shell, a quoted pin path.                                                                                                                                                                                                                                                                                                              |
| T5  | The person's `~/.ssh/config` turns on forwarding, a shared master, or changes the session | Command-line options win, checked again with `ssh -G`; `ControlPath=none` avoids reusing a master that has forwarding.                                                                                                                                                                                                                                                                                                                                |
| T6  | Other users on the remote machine reach the core                                          | `--stdio` has no listener; the data directory is 0700. `--core` listens only on a Unix socket in a 0700 folder, 0600, and closes any peer whose uid is not its own (section 6).                                                                                                                                                                                                                                                                       |
| T7  | Agents on the remote machine reach the laptop or its network                              | No reverse channel exists. The remote netproxy refuses loopback and private addresses as before. Links to the laptop's loopback and private hosts are refused.                                                                                                                                                                                                                                                                                        |
| T8  | Agents run unsandboxed on a machine without bubblewrap                                    | Agents, language servers, dev servers and the browser refuse to start there.                                                                                                                                                                                                                                                                                                                                                                          |
| T9  | A remote machine floods or crashes the UI                                                 | 16 MiB line cap, frame checks, bounded queue with resync, capped stderr, close on violation.                                                                                                                                                                                                                                                                                                                                                          |
| T10 | Local agents change the pins, the host list or `ssh`                                      | Both files are under the data directory, a protected path; `ssh` is an absolute path. This holds where the laptop's own sandbox is available; on a laptop without one (macOS until verified, Windows) local agents could change them. A `PATH` entry that local agents can write could put another `ssh` first, which would run outside the sandbox with `SSH_AUTH_SOCK`; that holds for `git` and `gh` today too.                                    |
| T11 | The phone companion exposes a remote machine                                              | It never runs in `--stdio`; `ssh.*` is not on its allowlist; a phone sees only the laptop's core.                                                                                                                                                                                                                                                                                                                                                     |
| T12 | The person thinks they act locally but act on the remote machine                          | "on <name>" bar and accent on every remote window, from the app, not the remote.                                                                                                                                                                                                                                                                                                                                                                      |
| T13 | Agents keep running unattended after the laptop leaves                                    | With `--attach` the core keeps agents running after the laptop leaves (section 6, owner approved 2026-11-28). Policies, approvals, the agent limit, the sandbox and the netproxy stay; nothing new starts while no client is attached (schedules included). The core ends 15 minutes after the last client once nothing runs, or on Stop. `--stdio` keeps the 60 s stop.                                                                              |
| T14 | The headless token reaches the person's machines                                          | `/ws/ssh/<id>` exists only in e2e builds.                                                                                                                                                                                                                                                                                                                                                                                                             |
| T15 | Agents on the machine drive its core through `core.sock`                                  | The data directory, `run/` included, is a protected path: the sandbox does not mount it, so the socket does not exist inside it (attack tests connect from a sandboxed agent, with `<data>` under the home folder and with `XDG_DATA_HOME` outside it). Unsandboxed processes of the same user (the person's terminals) can reach it, replace the laptop and answer approvals, as with an SSH login (section 6).                                      |
| T16 | A detached core keeps acting with nobody watching                                         | It does only what the person's policies allow; anything that asks waits for a person. Agent limits and the netproxy stay. It ends 15 minutes after the last client once nothing runs, and "Stop Ovra on <name>" ends it at once.                                                                                                                                                                                                                      |
| T17 | Another computer with the same SSH account takes over the machine                         | It replaces the current client, which is told `replaced`. Same account, same trust as an SSH login.                                                                                                                                                                                                                                                                                                                                                   |
| T18 | The install step runs something other than Ovra's own build, or a crafted command         | The remote commands are fixed strings with only the build-time SHA-256 (64 hex digits) in them; the binary comes only from the app's own resources the laptop checks it against the build-time hash before uploading; the remote check only guards against truncation, since the machine is not trusted; the laptop never runs the file. A planted symlink or foreign folder on the machine is refused. Nothing is copied without the person's click. |

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
  feature. Headless mode has no remote machines (`ssh.*` is unavailable
  there) outside e2e builds.
- Assumption: the `crates/ssh` tests for hosts and pins run the real
  `ssh -G` and `ssh-keygen` against a config and `known_hosts` of their own
  (nothing connects); their output is what the parsing must match, which a
  fake would only guess. Where OpenSSH 8.7+ is missing they say so and
  pass. The link's tests use a fake `ssh`.
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
- Staying on (daemon tests, a real `--core` on a temporary data
  directory): attach, detach and attach again with an agent still running
  and an `info.resync` after the second hello; a second client replaces
  the first, which reads `{"replaced":{}}`; `{"quit":{}}` stops the core;
  the idle exit with a fake clock; a peer of another uid is closed unread
  (where the test can switch uid, else the check is a unit test on the
  credential comparison); a `run/` that is a symlink or not 0700 is
  refused; `--stdio` and `--core` never share a data directory; a
  leftover `core.sock` with no listener is replaced; attach while a
  `--stdio` runs reads `busy`; a client that closes right after the hello
  leaves the first client attached; `--core` refuses an agent when the
  fake sandbox is unavailable; nothing scheduled starts while detached.
- Installing (`crates/ssh` with a fake `ssh`): the exact remote commands
  (snapshots), `uname` answers other than the two architectures refuse, a
  hash mismatch keeps the old binary, the size cap, and the host record
  pointing at the installed path afterwards; the install command has no
  newline; a planted symlinked `bin` or `.new` is refused and a short
  upload leaves no `.new` (both run the command under `sh` on a temporary
  `HOME`).
- Attack tests: a sandboxed agent on the machine cannot connect to
  `core.sock`, with `<data>` in the home folder and with `XDG_DATA_HOME`
  outside it.
- App: the remote capability list adds stop of itself only.
