# Agent security (phase 4)

Status: task 4.0; the netproxy (4.C) is built as described below. Reviewed twice by an independent reviewer agent in place
of the owner (owner's rule, 2026-11-24). The owner approved the environment
allowlist below on 2026-11-25; it amends ADR 0006. Builds on ADR 0005 (direct mode), ADR 0006
(Claude through the user's `claude` CLI) and `docs/specs/orchestration.md`.

Phase 4 has five parts. 4.0 defines the contracts between them:

- a **sandbox** around each agent process (4.A Linux, 4.B macOS, 5.A Windows);
- a **netproxy** that is the sandbox's only way out (4.C);
- a **secrets broker** that keeps secret values outside the sandbox (4.D);
- **policies** that decide a tool call without asking, or refuse it;
- an **audit log** of what agents did and who allowed it (4.E).

4.0 changes no behaviour. Sessions run as they do now, and every tool call
still asks the person, until a platform sandbox and project policies exist.
Nothing here loosens what holds today.

## Sandbox

**What is sandboxed.** The sandbox wraps the whole `claude` process of one
session, because its tools (Bash, Edit, ...) run inside that process. Each
session gets its own sandbox. Ovra's core, the UI and the person's own
terminals are never sandboxed.

**Contract.** `Sandbox` is a service trait in `crates/core/src/services.rs`:

```rust
pub trait Sandbox: Send + Sync + Debug {
    /// Whether this machine can sandbox agents, and with what.
    fn status(&self) -> SandboxStatus;
    /// The command line that runs `line` inside `profile`.
    fn wrap(&self, profile: &SandboxProfile, line: CommandLine) -> Result<CommandLine>;
}
```

- `CommandLine` is plain data: the program, its arguments, environment
  changes and the working folder. The core never spawns anything.
- The runtime builds the `claude` command line as it does today, calls
  `wrap`, and spawns the result.
- `wrap` starts nothing itself. A wrapper such as `bwrap` or `sandbox-exec`
  is the program it returns. Steps that must happen inside the process
  before `exec` live in a small helper binary that the platform crate ships
  and `wrap` names as the program: Landlock, seccomp, and handing `bwrap
  --seccomp` its file descriptor (4.A).
- `SandboxStatus` is `Available { name }` or `Unavailable { reason }`.

**One source of truth.** The runtime gets the sandbox when it is built. The
core only builds profiles.

- After each spawn (a resume spawns again), the runtime reports what it
  actually did with `AgentHost::sandboxed(Option<SandboxReport>)`. The
  report holds the sandbox's name and its network mode, or `None` for no
  sandbox. `SandboxReport` is a protocol type. A runtime that never reports
  counts as unsandboxed.
- Policies and the UI rely on this report only, never on what was
  configured.
- If the sandbox is `Available` and `wrap` fails, the session fails to
  start. It does not fall back to running unsandboxed.

**Profile.** The core builds it, because only the core knows the session:

| Field      | Meaning                                                                                                                            |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `work_dir` | The session's folder: its worktree, or the project root in direct mode. Readable and writable, except the protected paths (below). |
| `read`     | More folders the agent may read but not write. A worker's main repository git folder goes here: object store, refs, packed refs.   |
| `write`    | More folders the agent may write.                                                                                                  |
| `expose`   | A server pod's ports (`docs/specs/servers-and-browser.md`). Non-empty makes the sandbox a pod; `network` must not be `Open`.       |
| `forward`  | Ports of the work dir's servers the helper forwards to the netproxy as `OVRA-LOCAL <port>`. Needs `network` to be `Proxy`.         |
| `network`  | `Blocked`, or `Proxy { address }` for the session's netproxy. `Open` exists only for platforms that cannot limit the network.      |

A worker's own `<git dir>/worktrees/<name>` (its index and HEAD) stays
read-only with the rest of the main repository's git folder: with a
writable index the agent could add a submodule whose filters Ovra's own,
unsandboxed git would run. With the object store and index read-only, a
worker cannot stage or commit. It does not need to, because Ovra applies
its changes on approve.

**What the platform adds.** Each platform crate documents what it adds:

- **The agent's own state.** It comes from the session's provider (ADR
  0011): the profile's `agent_state` holds that provider's lists
  (`ProviderLists` in `services.rs`: its configuration folder and state
  file, the folders it writes, the files it rewrites, its transcripts
  folder, its config-folder variable and environment names) and the
  account's folder. The runtime of the provider sets it; Claude's lists
  are `CLAUDE_LISTS` in `crates/agent-runtime/src/claude_lists.rs`. Pods,
  the browser and language servers get none. The state of every known
  agent (`AGENT_STATE_IN_HOME` in `crates/core/src/protected.rs`) is
  never writable, whoever the sandbox is for. For Claude:
  - `~/.claude.json` and `~/.claude` stay readable, because `claude` needs
    its login and settings.
  - Writable: only the session data `claude` writes, and
    `~/.claude/.credentials.json`, which `claude` rewrites when it refreshes
    its login (part of the known limit below). The user's own sessions read
    that data too (shell snapshots are sourced before each Bash call), so a
    platform keeps it per session where it can: on Linux, `todos/`,
    `shell-snapshots/`, `session-env/`, `ide/`, `statsig/` and the like are
    empty private folders, and only this project's folder under
    `projects/` (the transcripts a resume reads) is kept on disk, without
    its `memory/`, which later sessions load like a `CLAUDE.md`.
    Transcripts a later resume reads can still be changed; that is a
    known limit.
  - Read-only: `settings*.json`, `CLAUDE.md`, `agents/`, `commands/`,
    `skills/`, `hooks/` and `plugins/`. An agent must not plant code that
    runs later outside the sandbox.
  - `~/.claude.json` holds `mcpServers` but must stay writable for
    `claude`. This is a known limit. 4.F tests it, and the trust fingerprint
    is the backstop until a platform can do better.
  - On macOS, keychain access cannot be scoped to one item (4.B notes how
    it limits this).
  - Gemini and Codex (ACP, `crates/acp-client/src/agents.rs`) write lock,
    temporary and database files right in `~/.gemini` and `~/.codex`, so
    their lists are `layered`: on Linux the folder is a tmpfs with its
    entries bound back read-only, their sign-in files writable, and their
    private entries (Gemini's `projects.json`, Codex's databases),
    secrets, links, sockets and pipes left out. What they add is lost with
    the session.
  - A session of an account with its own configuration folder
    (`CLAUDE_CONFIG_DIR`, ADR 0006) gets that folder instead, with the same
    rules, and its state file `<folder>/.claude.json` instead of
    `~/.claude.json`; `~/.claude` and every other account's folder stay
    hidden. The folder must exist, sit right in the home folder with a
    name that starts with `.claude` (such as `~/.claude-work`) and hold
    none of Ovra's data; otherwise the session does not start. No session
    works in or writes anything under `~/.claude*`, whatever its account. `CLAUDE_CONFIG_DIR` is set to the folder's real path,
    the only variable added to the allowlist for it. On Linux this is done;
    the Windows sandbox does not support account folders yet.
- **System and toolchains.**
  - The system's binaries and libraries are read-only.
  - Toolchain folders on `PATH` are readable. A `PATH` entry that is the
    home folder, an ancestor of it, or `.` is dropped from the sandbox's
    `PATH`.
  - Package caches the agent's builds need are writable, by default
    `~/.cargo/registry`, `~/.npm`, `~/.cache/pip` and `~/.cache/pnpm`. The
    platform crate lists them.
    Each project gets its own copy in Ovra's data folder, bound where the
    cache is and empty at first (owner approved): a shared cache would let
    an agent plant a package the user's next build outside the sandbox
    runs (an extracted crate's `build.rs`). The first build in a project
    downloads again, through the netproxy's allowlist. The copy is keyed
    by a hash of the project's id, so the main checkout and every
    worktree of a project share it (`docs/specs/worktree-setup.md`) and
    no two projects do.
- **Temporary files.** A private temporary folder is writable. On Linux
  `/tmp`, `/var/tmp`, `/run`, Ovra's temporary folder (the netproxy's
  sockets) and other routes to the same disks (`/mnt`, `/media`,
  `/sysroot`, `/.snapshots`) are hidden.
- **Deny by default.** Everything else in the home folder is hidden,
  including:
  - secret stores: `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config/gh`,
    `~/.netrc`, `~/.docker/config.json` and browser profiles;
  - Ovra's own data folder (store, trust, policies, audit), except the
    session's own worktree;
  - on Linux, `/run/user/<uid>`, because D-Bus reaches the keyring there;
  - agent sockets such as `SSH_AUTH_SOCK` and the gpg agent.
- **Environment (owner approved, 2026-11-25).** Outside a sandbox ADR 0006
  keeps every variable but the nested-session markers. A sandboxed session
  instead starts from an allowlist, so that `SSH_AUTH_SOCK`,
  `GITHUB_TOKEN`, `GH_TOKEN`, `AWS_*` and `NPM_TOKEN` do not pass. This
  amends ADR 0006:
  - the list names variables one by one, never by prefix (a
    `CLAUDE_CODE_*` prefix would pass `CLAUDE_CODE_OAUTH_TOKEN`);
  - it includes `PATH`, `HOME`, `LANG`, `TERM`, `ANTHROPIC_API_KEY`,
    `ANTHROPIC_BASE_URL`, the cloud-provider variables, CA bundles
    (`SSL_CERT_FILE`, `NODE_EXTRA_CA_CERTS`), toolchain homes (`CARGO_HOME`,
    `RUSTUP_HOME`, `NVM_DIR`) and the proxy variables below;
  - a project can add names to it (not built yet: it needs a profile
    field).

  The names every session keeps are `ENV_PERSON` and `ENV_TOOLS` in
  `crates/sandbox-linux` (and in `crates/sandbox-windows`); an agent's
  session keeps its provider's names too (`ProviderLists::env`, for
  Claude the `ANTHROPIC_*`, `CLAUDE_CODE_USE_*` and cloud-provider names,
  and `fixture_var`), and no other provider's. The wrapped
  command line removes every other variable Ovra's environment holds when
  the session starts, so bubblewrap's own processes (whose environment
  the agent can read where Landlock does not hold) have none of them. The
  helper starts the agent with only the kept names, so one the runtime
  adds to that line later still never reaches the agent. The netproxy
  bridge gets no environment. A kept URL with a password in it
  (`ANTHROPIC_BASE_URL=https://user:pass@...`) passes as it is. When the
  profile names them, the sandbox's own line sets `OVRA_PORT`,
  `OVRA_WORKTREE_ROOT` and `OVRA_MAIN_ROOT` over the command's
  (`docs/specs/worktree-setup.md`): a port and two paths, never secrets.

  Amended 2026-11-25 (tightening, standing rule): where the OS has no
  sandbox yet, a session and Ovra's checks of `claude` (version, sign-in)
  get the same allowlist, plus `PATH`, `TMPDIR` and the person's proxy and
  CA bundle variables (`crates/agent-runtime/src/env.rs`). Dropped there
  too, as in the sandbox: `CLAUDE_CODE_OAUTH_TOKEN`, a `CLAUDE_CONFIG_DIR`
  the person set in their shell (an account in Ovra sets its own),
  `AWS_PROFILE`, `GOOGLE_APPLICATION_CREDENTIALS` and `SSH_AUTH_SOCK`.

**Language servers (ADR 0007).** On Linux `wrap_server` builds a
server's sandbox from the same parts as an agent's: the namespaces, the
system read-only, the hidden folders and secrets, the filtered `PATH`,
the toolchains, Landlock and seccomp through the helper. It differs in:

- no agent state: nothing of `~/.claude` or `~/.claude.json`; a `read`
  path inside or holding them, a never-bound folder (`~/.config`,
  `~/.ssh`, ...) or a credentials file (`~/.netrc`, `~/.npmrc`,
  `credentials.toml`, ...) is refused;
- the project read-only (so no protected paths are needed) and the
  working folder; it must not hold the home folder;
- the cache folder the only writable folder besides a private `/tmp`
  and `/dev` (Landlock's list; `/var/tmp` and the home folder are empty
  and not writable). It must be
  inside `<data dir>/lsp` (not just any hidden folder: the temporary
  folder holds the netproxy's sockets), which is hidden, and is bound
  back after it;
  it must not be inside the project;
- the user's package caches (`~/.cargo/registry`, `~/.npm`, ...) read-only
  where they are, never the per-project copies agents get;
- always a new, empty network namespace: `Proxy` and `Open` are refused
  (`bad_request`); loosening that needs the owner;
- the environment allowlist every session keeps (`ENV_PERSON` and
  `ENV_TOOLS`), with no agent's names (`ANTHROPIC_*`,
  `CLAUDE_CODE_USE_*`, cloud-provider and `OVRA_FAKE_CLAUDE_FIXTURE`);
  the server's own line (build and cache folders, `CARGO_NET_OFFLINE`)
  is applied last.

**Server pods.** A profile with `expose` is a pod: an agent's sandbox
(the same mounts, hidden paths, protected paths and `write` folders)
without the agent's state. Nothing of `~/.claude` or `~/.claude.json` is
bound (not its login `.credentials.json`, transcripts or history), and
the environment allowlist is `ENV_PERSON` and `ENV_TOOLS`, as for
language servers: no `ANTHROPIC_*`, `CLAUDE_*` or cloud-provider names.
The pod's folder is writable inside it; on Linux pod-init adds a
Landlock layer for the servers it starts, so that only pod-init makes,
removes or renames entries there, and Ovra connects to the sockets in it
without following links and only to pod-init's (`docs/specs/servers-and-browser.md`).

**Protected paths.** They stay read-only. The list lives in
`crates/sandbox`, so every platform uses the same one:

- Git:
  - `.git/hooks`, `.git/config`, `.git/modules/*/hooks` and
    `.git/modules/*/config`. On Linux the whole `.git` folder of a direct
    session is read-only, and every nested repository's: git also reads
    `commondir`, `packed-refs` and new `modules/*` folders, and the agent
    has nothing to commit (Ovra reviews its files). Submodule folders that
    are not checked out stay read-only too, at any depth and also when
    their folder is missing, so the agent cannot put a repository there
    that git steps into; so do config files inside the project that git
    includes. A missing folder or include is made empty first, so it can
    be bound; these show up as untracked files in the project. Planning
    never runs git in a repository the agent could have made, and never
    with `core.fsmonitor`;
  - the folder `core.hooksPath` names, for example `.husky/`;
  - `refs/ovra/` and its reflog;
  - for a worker, its worktree's `.git` file, plus `commondir`, `gitdir` and
    `config.worktree` in `<git dir>/worktrees/<name>`, because Ovra later
    runs git there unsandboxed.
- Agent configuration: `.claude/`, `.mcp.json`, every `CLAUDE.md`,
  `CLAUDE.local.md` and `AGENTS.md`.
- Other agents' configuration, which they load when the person runs them
  outside Ovra (`docs/research/provider-agnostic.md`): `.codex/`,
  `.gemini/`, `.cursor/`, `.opencode/`, `opencode.json`, `opencode.jsonc`
  and `.aider.conf.yml`. Only names directly in the work dir: a path
  below a folder that is a link (`.github/`) could be moved away with it.
  Other agents' instruction files are only marked in review (below),
  because they are often links to `AGENTS.md` and a protected link
  refuses the session.
- Code that tools run on their own: `.vscode/`, `.idea/`, `.zed/`,
  `.devcontainer/` and `.envrc`.
- Ovra's own project settings: `.ovra/`, whose `servers.json` holds the
  commands a server start runs (`docs/specs/servers-and-browser.md`) and
  `worktree.json` the setup steps of new worktrees, and `.worktreeinclude`,
  which lists the ignored files copied into every new worktree
  (`docs/specs/worktree-setup.md`).

Known gaps on Linux, left to the backstop below:

- a git config value that names a file inside the project
  (`core.fsmonitor = ./x`, `filter.*.clean`, `diff.*.textconv`,
  `credential.helper`, `alias.* = !./...`): the file stays writable;
- a submodule the user adds while a session runs;
- a bind mount of the home folder outside `/mnt`, `/media`, `/sysroot` and
  `/.snapshots` (for example `/srv`) stays readable;
- a `CLAUDE.md`, `CLAUDE.local.md` or `AGENTS.md` that does not exist yet
  in a subfolder (new or not), and a missing `opencode.json`,
  `opencode.jsonc` or `.aider.conf.yml`, can be created.
- when the helper is a development build inside a checkout that is also an
  agent's writable work dir (`target/debug/ovra-daemon` in a direct
  session), the agent can rebuild or replace it between sessions, and the
  next session runs that file as the helper; while a session runs its
  folder is read-only. Run Ovra from an install outside the project.

The sandbox status lists these gaps and the UI shows them; that list must
change with this one.

Ovra's own git calls (fs-index, worktree) should pass
`--ignore-submodules=all` so a missed submodule cannot run a filter in
them; that is a follow-up in those areas.

On Linux a missing `.claude/`, `.codex/`, `.gemini/`, `.cursor/`,
`.opencode/`, `.vscode/`, `.idea/`, `.zed/`, `.devcontainer/` or `.ovra/`
is made as an empty folder and bound read-only, so the agent cannot
create one for the user's own tools to load later. Linux lets a process
remove a folder that is a mount point only in another mount namespace,
which detaches the bind there, so a folder an agent's session made is
removed only once every sandbox of that work dir (sessions and server
pods) and every process in them are gone, also when they are killed: each
holds a shared lock on the work dir's record in Ovra's data folder, and
the helper outside the sandbox that ends last removes what the record
lists. A folder that is no longer empty stays, and so do those a pod made.
A missing `CLAUDE.md`, `CLAUDE.local.md`, `AGENTS.md`, `.mcp.json`,
`.envrc` or `.worktreeinclude` at the top of the work dir is made as an
empty file and bound read-only the same way, for agents, pods and the
browser alike, and removed by the same last helper if it is still an
empty file. While a
session runs it shows in git status (the owner chose this over leaving
the files creatable, 2026-11-25); reviews leave such an added empty file
out. Removing or replacing it from outside while a session runs (`git
clean`, a save by rename) detaches the bind, as for any protected path,
and the agent can then create it. One in a subfolder can still be
created; the review backstop below covers it.

An agent that needs one of these changed says so in its summary, and the
person makes the change. A protected path that is a link cannot be kept
read-only (its name can be replaced), so on Linux the session does not
start and says which link to replace.

Not every platform can protect a path that does not exist yet, or make a
subtree read-only. The backstop is outside the sandbox:

- the review marks any change to a protected path, and approve never
  applies one unless the person includes protected files
  (`include_protected`): a worker's are otherwise left out, as in a partial
  approve, and a direct item with one is refused. A direct item's include
  is bound to the versions of its protected files the person saw (the
  item's fingerprint): one changed or added since is refused. Replacing
  the person's version with what a partial approve left (`review.take`)
  needs the same choice. The inbox offers it only once each protected
  file's diff has been shown. The marks cover the `work_dir` list, in any
  case, and also, though the sandbox does not keep them read-only:
  `.envrc`, `.claude/`, `.codex/`, `.gemini/`, `.cursor/`,
  `opencode.json(c)` and `.gitattributes` in any folder; `.husky/`,
  `.gitmodules`, `.lfsconfig`; other agents' instruction files
  (`GEMINI.md`, `AGENTS.override.md` and `QWEN.md` in any folder,
  `.cursorrules`, `.goosehints`, `.github/copilot-instructions.md`,
  `.github/instructions|agents|prompts|chatmodes|hooks/`, `.agents/`);
  GitHub's `.github/workflows/` and `.github/actions/`, which run with
  the repository's secrets once a pull request's branch is pushed;
  the configuration of agents Ovra does not know well yet (`.qwen`,
  `.kiro`, `.windsurf`, `.windsurfrules`, `.amazonq`, `.roo`,
  `.roomodes`, `.kilocode`, `.clinerules`, `.continue`, `.crush`,
  `crush.json`, `.factory`, `.junie`); and the folder `core.hooksPath`
  names when it is inside the working tree;
- what no diff shows: a review item also lists what changed, by anyone,
  since its session started (`ReviewItem.git_changes`, SEC-5): every
  config file git reads for the project (its own, `config.worktree`,
  includes, the user's and the system's), `info/attributes`,
  `info/exclude`, submodules' config and hooks, and the hook files
  (`core.hooksPath`, or `.git/hooks`; a link counts with what it points
  to; git's own hook names always, other files up to a cap, then their
  count). They are read from the project, never through a worker's `.git`
  file. Approve and reject never touch them; a decided item keeps the
  list it was decided with, and an item with only such changes still
  shows. When the record or the reading failed, `git_unchecked` says so.
  Not covered yet: sessions without a review item (a coordinator, a
  direct session without a baseline), the files `core.attributesFile`
  and `core.excludesFile` name, and nested `.git` folders;
- a change to agent configuration drops the project's trust (fingerprint,
  task 2.F).

**Without a sandbox.** If `status()` is `Unavailable`, the session runs
unsandboxed, as it does today. The UI says so on the session, and the audit
log records it. Policies never skip a question for such a session. A project
setting that refuses unsandboxed sessions comes with 4.A.

**Known limit.** The agent process reads Claude Code's own login, so its
tools can read it too. The netproxy is what keeps that login from leaving
the machine. The threat model in 4.F records this.

**Crate.** `crates/sandbox` holds what all platforms share:

- the protected-path list (the `work_dir` part is defined in
  `ovra-core`, `protected.rs`, so the review marks the same paths, and
  re-exported here);
- `Unsandboxed`, which is `Unavailable` and returns the line unchanged;
- `for_this_os()`, which picks the platform crate or falls back to
  `Unsandboxed`.

`daemon` and `app` pass it to the agent runtime. Its edges in
`scripts/crate-graph.json` are part of this task.

## Netproxy (4.C)

`NetProxy` is a service trait, implemented by `crates/netproxy`
(`NetProxies`):

```rust
pub trait NetProxy: Send + Sync + Debug {
    /// What every session may reach: the hosts `claude` itself needs.
    fn base(&self) -> Allowlist;
    /// Starts a proxy for one session; it lives until `stop`.
    fn start(&self, session: SessionId, allow: Allowlist, refusals: NetReporter) -> BoxFuture<'_, Result<String>>;
    /// Replaces a running proxy's project allowlist.
    fn update(&self, session: &SessionId, allow: Allowlist);
    fn stop(&self, session: &SessionId);
}
```

- **Lifetime.** The core starts a session's proxy before the runtime, puts
  its address in the profile as `Proxy { address }`, and stops it when the
  session ends. A resume starts a new one. If the proxy cannot start, the
  session fails; it never runs with an open network instead. Without a
  netproxy wired in (tests, other front ends), the profile says `Open`.
- **Address.** A Unix socket path on Linux and macOS, because a sandbox
  without a network namespace has no loopback. Each socket has its own
  folder with a random name, inside a randomly named folder under the
  system's temporary folder; only the user can open either. A sandbox must
  make only its own session's socket reachable, never the folder above
  it, or an agent could use another project's allowlist (4.A, 4.B). On
  Windows it is a 127.0.0.1 port, which any local process can use; 4.F
  records this. There the sandbox user's firewall rule blocks the
  internet but not loopback or the system's DNS resolver
  (`docs/specs/sandbox-windows.md`). The platform's helper bridges the
  socket to a local port inside the sandbox. `wrap` sets `HTTP_PROXY` and
  `HTTPS_PROXY` to that port, clears `NO_PROXY`, and sets
  `NODE_USE_ENV_PROXY=1` (4.A, 4.B). For each port in the profile's
  `forward`, the same bridge also listens on `127.0.0.1:<port>` inside the
  sandbox and opens each connection to the socket with
  `OVRA-LOCAL <port>\r\n` (the loopback rule in
  `docs/specs/servers-and-browser.md`). On Linux the plan refuses a port
  the bridge needs itself (3128), port 0, a port both exposed and
  forwarded, `forward` without a proxy, and a pod (`expose`) with an open
  network. Until a platform sandbox exists the
  proxy runs but nothing uses it, and the session reports no sandbox.
- **Requests.** `CONNECT host:port` for TLS, and plain `http://` requests
  in absolute form. A plain request is passed on as one request with
  `Connection: close`, its `Host` set from the URL, and the hop headers
  removed (`Proxy-*`, `Connection` and those it names, `Keep-Alive`, `TE`,
  `Trailer`, `Upgrade`). Its body is framed by one `Content-Length`;
  exactly that many bytes are passed on and nothing after them, so a
  pipelined second request never reaches the server or the user's proxy.
  `Transfer-Encoding` and repeated or malformed `Content-Length` are
  refused. Anything else is refused with 400.
- **Allowlist.** Host names or IP addresses, each optionally with a port.
  Names match exactly: no wildcards, so `github.com` does not allow
  `api.github.com`.
  - The base list holds what `claude` itself needs: `api.anthropic.com`,
    `claude.ai` and `platform.claude.com` (API, sign-in, token refresh),
    plus the user's Bedrock, Vertex or Foundry endpoint and
    `ANTHROPIC_BASE_URL`, read from the environment Ovra runs in.
    Telemetry hosts are left out: anyone with an account there can receive
    data.
  - Each provider has its own base list (`crates/netproxy/src/base.rs`;
    the list above is Claude's), and a session's proxy gets only its own
    provider's (ADR 0011). A server pod's proxy, which belongs to no
    agent, gets every provider's. `netproxy.get_allowlist` shows every
    provider's together, and no secret is ever added for a host on any of
    them.
  - An endpoint the user set in a `*_BASE_URL` variable may be on this
    machine or the local network (a gateway). Only that exact host and
    port may then reach a private address.
  - The project's own list adds to the base. It is kept by `ProjectStore`.
    `netproxy.get_allowlist` and `netproxy.set_allowlist` read and change
    it. A change reaches running proxies at once, for new connections;
    open tunnels stay.
  - `set_allowlist` normalizes names (lowercase, no trailing dot) and
    refuses wildcards, anything that is not a plain name or address, port
    0, and addresses that are never reachable.
- **Refusals.**
  - The proxy resolves names itself. It refuses a host when any address it
    resolves to is not public unicast: loopback, private, link-local (the
    metadata service, 169.254.169.254), shared, documentation, multicast
    and reserved ranges, IPv4-mapped and 6to4 forms of those, and IPv6
    outside 2000::/3. That keeps agents from reaching Ovra's own daemon or
    the local network. It connects to the addresses it checked.
  - It allows ports 443 and 80, unless the allowlist entry names another
    port; an entry with a port allows only that port.
  - Every refusal goes to the core through `NetReporter`, which pushes
    `netproxy.refused { project, session, host, reason }` to the UI and
    writes it to the audit log as `NetworkRefused`: at most 30 a minute
    for one session, with the host and reason cut to 255 characters, and
    one entry saying how many were left out, so an agent in a loop fills
    neither the log nor the UI.
- **Upstream proxy.** If the user's own proxy is set where Ovra runs
  (`https_proxy`, `HTTPS_PROXY`, `http_proxy`, `HTTP_PROXY`, as Claude Code
  reads them, with `NO_PROXY`), the netproxy chains to it, with the user
  and password from its URL. Only `http://` proxies are supported. A name
  that does not resolve where Ovra runs is left to that proxy to resolve;
  names that do resolve are still checked. Each name is also looked up as
  given, not fully qualified, as a proxy on this machine would: entries in
  `/etc/hosts`, search domains and the system's other name sources that
  answer with a loopback, private or link-local address refuse the
  request too.
- **Limits.**
  - Without TLS inspection the proxy cannot see inside a tunnel, so a
    broad host such as `github.com` is still a way to send data out, and a
    host on a shared CDN address can be reached by name through another
    allowed name on the same address (domain fronting). The
    `netproxy.allowlist` answer lists such entries under `broad`, and the
    UI warns about each.
  - The proxies serve at most 256 connections at once, one session's
    proxy at most 64; more wait. A connection where nothing moves for
    10 minutes is closed.
  - `api.anthropic.com` and the provider endpoints in the base list are
    channels out too: an agent can send data with someone else's API key.
    4.F records this, like the broad entries.
  - The runtime reports `Proxy` from the profile. Once a platform sandbox
    reports it, policy `auto` rules turn on, so 4.A and 4.B must prove that
    `wrap` really leaves the proxy as the only way out.

## Secrets (4.D)

A secret's value never enters the sandbox.

- `Secrets` is a service trait over the OS keychain: `list()` returns
  names, `set(name)` and `remove(name)` change them, and the netproxy calls
  `value(name)`, which no protocol method exposes.
- **Delivery.** Through the netproxy. A secret is bound to one host and one
  header, for example `Authorization: Bearer <value>` for `api.github.com`.
  The proxy adds the header to requests for that host only. To make this
  possible for HTTPS, 4.D proposes how the proxy terminates TLS for bound
  hosts. Its own spec needs review before code.
- Each use is written to the audit log as `SecretUsed { name, host }`.

## Policies: auto, ask, deny

A policy decides a tool call before the person is asked. It is a list of
rules for each project:

```json
{
  "rules": [
    { "tool": "Bash", "match": "npm test", "action": "auto" },
    { "tool": "Edit", "match": "docs/**", "action": "auto" },
    { "tool": "WebFetch", "action": "deny" }
  ]
}
```

- `tool` is a tool name. `match` is optional and tested against the call's
  main argument. Only `Bash`, `WebFetch` and the file tools below take a
  `match`; `policy.set` refuses one on any other tool, because it could
  never match.
  - Rules keep Claude's syntax, and a rule matches only calls of the tool
    it names. The main argument comes from the call's shared fields
    (ADR 0011), which the runtime fills: the `command` of an `execute`
    call for `Bash`, its `paths` for the file tools, its `url` for
    `WebFetch`.
  - A file rule and a call that names several files: `auto` applies only
    when it matches every one, a `deny` or `ask` when it matches any.
- For `Bash`, `match` is a command prefix, matched on word boundaries
  (`npm test` matches `npm test -- -u`, not `npm testx`).
  - With `"exact": true` it is the whole command instead: `npm test`
    matches `npm test` (in any spacing), not `npm test -- -u`. Rules stored
    without `exact` keep the prefix meaning. `exact` is for `auto` rules
    only: an exact `deny` or `ask` would let `npm test -- -u` past a rule
    meant to stop `npm test`, so `policy.set` refuses `exact` on a `deny`
    or `ask` rule and on any rule that is not Bash with a `match`. An exact
    `deny` or `ask` stored anyway matches the prefix.
  - A command that contains `;`, `&`, `|`, a backtick, `$(`, `>`, `<` or a
    newline never matches an `auto` rule.
  - A Bash `deny` rule is advisory: `env git push` or `git -C . push` gets
    past it. The boundary that holds is the sandbox plus the netproxy. The
    UI says so beside Bash deny rules.
  - A Bash `ask` rule is advisory in the same way: the same command written
    another way is not asked about. The UI says so beside Bash ask rules.
  - An `auto` rule runs more than its words: `npm test` runs the scripts in
    `package.json`, which the agent can edit. The UI says so beside Bash
    auto rules; the sandbox is what makes that acceptable.
- For `Read`, `Edit`, `MultiEdit`, `Write` and `NotebookEdit`, `match` is a
  glob on the path relative to `work_dir`.
  - The path is normalized lexically first (as `RelPath::parse`): no `..`
    segments. The core cannot see symlinks; the sandbox contains them,
    which is one more reason `auto` needs a sandbox.
  - On macOS and Windows the path is compared case-insensitively.
  - A path outside `work_dir` never matches an `auto` rule.
  - A glob has no empty or `.` segments (`secrets/` is refused).
  - A file `deny` rule is advisory too: `Grep`, `Glob` or `cat` in Bash
    still reach the file. The boundary that holds is the sandbox's protected
    and hidden paths.
  - A file `ask` rule is advisory the same way: Bash commands like `cat`
    still reach the file without a question. The UI says so beside it.
- For `WebFetch`, `match` is one host, written as the netproxy allowlist
  writes it (lower case, no scheme, port or wildcard). The netproxy still
  decides whether the host is reachable.
  - An `auto` rule matches a call whose `http` or `https` URL names exactly
    that host. Subdomains do not match.
  - A `deny` or `ask` rule matches that host and its subdomains
    (`evil.com` covers `api.evil.com`, not `notevil.com`).
  - Ovra reads the host only from a plain URL. It cannot read one with user
    info (`a@b`), a backslash before its path, a missing `//`, a `%` escape,
    non-ASCII letters or dots, or no URL at all. Such a call never matches
    an `auto` rule and always matches a `deny` or `ask` rule with a host:
    a URL parser may read it as the denied host, so the rule holds.
- `action` is `auto`, `ask` or `deny`.

**How a call is decided.**

- A `deny` rule wins and refuses the call. The agent receives the rule as
  the reason.
  - Deny is enforced in both places a tool call passes: `can_use_tool` and
    the `PreToolUse` hook (ADR 0006). The runtime forwards the hook to the
    core (`AgentHost::before_tool`, which already sees every call), so a
    policy changed mid-session applies at once.
  - The hook matters because calls the user's own Claude settings already
    allow never reach `can_use_tool`.
- An `ask` rule wins over an `auto` rule that also matches: the stricter
  rule decides.
  - Ask is enforced in the hook too: it answers `permissionDecision: "ask"`,
    so the CLI asks through `can_use_tool` and Ovra asks the person, even
    for a call the user's own Claude settings allow.
- An `auto` rule applies only when the runtime reported a sandbox for the
  session (above) whose network is not `Open`. Ovra skips the question only
  when the sandbox would contain a mistake. Otherwise `auto` counts as
  `ask`.
- Everything else asks the person, as now. That includes calls no rule
  matches.
- If the policy cannot be read (a store error), the hook refuses every
  call and `can_use_tool` asks: nothing is decided as `auto`.
- Ovra's own IDE tools (`mcp__ovra__*`) keep their role rules from the
  orchestration spec. A policy does not change them, and `policy.set`
  refuses rules for them.
- The CLI's own permission rules still apply inside `claude` before Ovra is
  asked. These are the user's settings, plus the project's
  `.claude/settings.json` when the project is trusted. Policies add a layer
  on top and replace nothing.
  - Only `deny` and `ask` rules override those settings, through the hook.
  - An `auto` rule never skips more than they allow: the hook leaves it to
    the CLI, and it applies only to calls the CLI asks Ovra about.

**Storage and protocol.**

- A project's policy is kept by `ProjectStore`.
- `policy.get { project }` reads it. `policy.set { project, policy,
  expected? }` replaces it.
  - `expected` is the policy the change was built from. When it is given
    and the stored policy differs, `set` refuses with `conflict` and keeps
    nothing, so a rule added elsewhere meanwhile is never dropped. The UI
    always sends it.
- Only the person changes a policy, through the UI. No agent tool can, and
  policies live outside every sandbox.
- `set` refuses unknown actions, empty tool names and globs that cannot
  match inside `work_dir`.

**Always allow (owner's approval, 2026-11-25).** An approval card may offer
"Always allow <scope> here": it adds one `auto` rule and allows the call.

- The core works out the rule when it registers the request
  (`PermissionRequest.always`); the UI only shows it. It is offered only
  when no project rule decided the call, and the session runs in a sandbox
  whose network is not `Open` (elsewhere the rule would never count).
- The scope is the narrowest rule for the call:
  - `Bash`: the exact command, in single spaces, as an `exact` rule, so
    it allows that command and nothing longer. Not offered for joined
    commands, commands longer than 200 characters, a first word that runs
    other commands (`env`, `sudo`, `xargs`, `source`, `npx`, shells, ...;
    compared without quotes, backslashes, folder, `.exe` or version, in any
    case) or sets a variable, a runtime or tool that takes code on its own
    (`node`, `python3`, `git`, ...), any word naming a path outside the
    folder (`/`, `~`, `$`, `%`, `..`, a drive) or a protected or
    review-marked path, or any word the shell expands past that check
    (`*`, `?`, `[`, `{`, `$` or a backslash anywhere, or a quote inside the
    word).
  - `Edit` and `Write`: the exact file, relative to `work_dir`. Not offered
    outside it, for protected or review-marked paths, for names with glob
    characters, for any path with a `.git` part (in any case, with
    trailing dots or spaces, or as `git~1`), for any path with a part that
    may be a Windows 8.3 short name (`~` and a digit, as in
    `PACKAG~1.JSO`, which may stand for any file), or for files that say what
    commands run: `package.json`, `Makefile`, `GNUmakefile`, `build.rs`,
    `Cargo.toml`, `pyproject.toml`, `setup.py`, `.npmrc`, `justfile`,
    `.envrc`, `lefthook.yml`, `.pre-commit-config.yaml`, `Rakefile` (by
    name, in any folder and case) and anything in `.github/workflows/`,
    `.cargo/` or `.husky/`. A rule for one would let the agent change what
    an allowed command does.
  - `WebFetch`: the URL's host. Not offered for addresses, single-label
    names or hosts where anyone can receive data (the netproxy's broad
    list).
  - Nothing else.
- `permissions.answer { always: true }` first takes the request out of
  the pending ones, so no other answer can come between. Then, under the
  same lock as `policy.set`, it adds the rule (marked `from_approval`),
  records `RuleAdded { rule, by: Person }` for the asking session and
  announces `policy.changed`. Last it answers allow.
  - It refuses with `not_found`, adding no rule, when the request is no
    longer pending.
  - It refuses with `bad_request` when the request offers no rule or a deny
    or ask rule matches the call by then. The request then stays pending
    for Allow once or Deny, announced again with `permissions.requested`
    so a client that reloaded meanwhile still shows it. If its session
    stopped or ended while the request was taken, it is denied and
    resolved instead.
  - A rule with the same tool, match, action and `exact` already in the
    policy, wherever it came from, is not added again.
- For a Bash rule the card says beside the action: "Runs what this
  project's scripts say; the sandbox still applies."
- Agents cannot reach it: it is a client method, not an IDE tool. The rule
  shows in Settings > Rules, marked "exactly" and "From an approval", where
  Remove takes it out. A person can add an exact Bash `auto` rule there
  too ("Exact command", shown only when the action is Auto).

**Quiet mode (owner's approval, 2026-11-27; ADR 0015).** A project's policy
has a `quiet` switch, off by default. With it on, a call that no rule
decides (no deny, ask or auto rule matches) is allowed without asking when
the sandbox contains it. It is decided in `can_use_tool` only, like an
`auto` rule, so it never skips more than the user's Claude settings allow.

- The session: the runtime reported a sandbox whose network is not `Open`,
  and whose name Ovra trusts for this (`QUIET_SANDBOXES`, today the Linux
  one: `bubblewrap...`). macOS (Seatbelt, off until verified on a Mac) and
  Windows (network gaps) still ask; a platform joins in its own change
  once verified.
- The call, by the kind the runtime filled (ADR 0011). Changes and
  commands run quietly only in a worktree (`isolated`): in a direct
  session an edit lands in the person's checkout, where their own tools
  (a pre-commit hook, `npm test`, a watcher) may run it before any review,
  and a research session changes nothing.
  - `read`: at least one path, and every path inside `work_dir`, not
    protected or review-marked, and with no `.git` part;
  - `search`: the same, where no path means the folder itself. Claude's
    `Glob`, `Grep` and `LS` report their `path`, and a `Glob` pattern or
    `Grep` glob that starts at `/`, `\` or `~` or holds `..` as a path
    too; `WebSearch` has no path and runs on Anthropic's side;
  - `edit`, `delete` and `move`: worktree only; at least one path, every
    one inside `work_dir`, not protected or review-marked, with no `.git`
    part and no part that may be a Windows 8.3 short name;
  - `execute`: worktree only. The command has no backtick, `$(`, `<(` or
    `>(` (what runs would be hidden). Every word that names `git` (as
    [`program`] reads it, quotes and folders stripped) is followed by one
    of git's reading subcommands (`status`, `diff`, `log`, `show`,
    `blame`, `grep`, `ls-files`, `ls-tree`, `rev-parse`, `rev-list`,
    `describe`, `shortlog`, `cat-file`, `show-ref`, `merge-base`,
    `name-rev`, `for-each-ref`, `diff-tree`, `diff-files`, `diff-index`,
    `check-ignore`, `check-attr`, `count-objects`, `help`, `version`),
    with only `--no-pager` or `-P` before it: anything else may change
    git state. No word (nor a part of `--opt=value`) names a protected or
    review-marked path, a `.git` part, an agent's own state folder
    (`.claude`, `.codex`, `.gemini`, ...), starts with `~`, holds `$` or
    `%`, or is a glob from `/`: those reach the home folder, where the
    agent's own login stays readable, under names the check cannot see.
    Other paths outside the folder are left to the sandbox;
  - `fetch`: a plain URL whose host is a name with a dot, not an address
    and not on the netproxy's broad list. The netproxy still decides
    whether it is reachable;
  - `think` and `subagent`: allowed (a helper agent's own calls are
    decided one by one);
  - anything else (`other`, any `mcp__` tool) asks.
- A path anywhere in `read`, `search`, `edit` or `execute` (paths and
  command words) is held by every deny and ask rule of a file tool
  (`Read`, `NotebookRead`, `Edit`, `MultiEdit`, `Write`, `NotebookEdit`),
  whatever tool the call is: `Read(.env)` deny keeps a quiet `Grep` or
  `cat .env` asking. Such a rule without a match makes every one of these
  calls ask. A `search` also asks when it covers the whole folder, or a
  folder above what such a rule names (`src` for `src/keys/**`, any
  folder for `**/.env`): it would read the held files. Paths starting
  with `~` or holding `$` or `%` never count as inside the folder, and a
  command word with a part that starts with an agent's state name
  (`.claude-work` too) asks.
- Keys still ask on first use (`UseKey`), Ovra's pushes keep their own
  rules (`Push`), and Ovra's IDE tools their role rules: quiet mode does
  not decide them.
- The audit log records `ToolDecided` with `DecidedBy::Policy { rule:
  "quiet mode" }`.
- The git check, like every Bash rule, is advisory: a script the agent
  wrote can run git. In a worktree `.git` is read-only in the sandbox, and
  pushes need the netproxy and a key, which asks.
- "Always allow" is still offered on what quiet mode asks about.
- A quiet command reaches every host the allowlist lets through, broad
  ones included, like any command the person allowed: data can leave to
  them without a question. The switch says so.
- Settings > Rules shows the switch, what runs without asking, what still
  asks, where it works, and while it is on the risk: a harmful
  instruction in a file could run unseen and send data to any allowed
  site; changes still wait in Reviews.

## Audit log

The log gets one entry for each event that matters for security. The schema
is part of the protocol (`protocol/audit.rs`) so the UI and tests share it:

```rust
pub struct AuditEntry {
    pub seq: u64,          // increasing within one Ovra data folder
    pub at_ms: u64,        // Unix time
    pub project: ProjectId,
    pub session: Option<SessionId>,
    pub event: AuditEvent,
}
pub enum AuditEvent {
    SessionStarted { mode: SessionMode, trusted: bool, sandbox: Option<SandboxReport> },
    ToolDecided { tool: String, input: AuditInput, allowed: bool, by: DecidedBy },
    TrustChanged { trusted: bool, fingerprint: Option<String> },
    PolicyChanged { policy: Policy },
    RuleAdded { rule: PolicyRule, by: DecidedBy },
    ReviewDecided { review: ReviewId, action: ReviewAction },
    NetworkRefused { host: String, reason: String },
    SecretUsed { name: String, host: String },
}
pub enum DecidedBy { Person, Policy { rule: String }, Role, SessionEnded }
pub enum ReviewAction { Approved, PartlyApproved, Rejected, UndoneReject, Taken, UndoneTake }
```

**`AuditInput` keeps secrets out:**

- For `Write`, `Edit`, `MultiEdit` and `NotebookEdit`, it records the path
  and a SHA-256 of the content, never the content.
- For anything else, it records the input as JSON, cut to 4 KiB, with these
  removed first:
  - values the secrets broker knows, as they are, JSON-escaped and
    percent-encoded;
  - strings that look like tokens: `sk-`, `ghp_`, `gho_`, `github_pat_`,
    `xox`, `AKIA` and JWTs;
  - the values of `Authorization` and `Cookie` headers.
- Tool names are agent-chosen too: they get the same treatment.
- If the secrets' values cannot be read (a locked keychain), nothing the
  agent wrote is kept: the input is `[redacted]`, a file tool keeps only
  its hash, and any tool name but Claude's built-in tools (`Bash`, `Read`,
  `WebFetch`, ...) is `[redacted]`, since an MCP tool's could hold a value.
- Known limit: a value in another encoding (base64, hex, split across
  strings, partly escaped) is not recognised, unless it also looks like a
  token.
- The core reads a project's values once and keeps them in memory until
  its secrets change (`secrets.set`, `secrets.remove`, removing the
  project). A project has at most one read in flight, and every decision
  of the project waits on it until one deadline, 5 s after the read began:
  a hung keychain costs 5 s once per read, not once per decision.
- A read that fails, or is not done by its deadline, counts as
  unreadable: the decisions waiting on it are recorded as above. A late
  answer still fills the cache; until it comes, later decisions do not
  wait. After a failed read the keychain is not asked again for 10 s.
  A change to the project's secrets drops what is known, including a
  failure or a hung read, so the next decision reads them again.
- Decisions are queued when they are made and recorded by a task of their
  own, so an agent that stops waiting loses no entry. Each project's
  entries keep the order its decisions were made in; projects do not wait
  for each other.

**Service:**

```rust
pub trait AuditLog: Send + Sync + Debug {
    fn append(&self, project: &ProjectId, session: Option<&SessionId>, event: AuditEvent) -> Result<AuditEntry>;
    fn list(&self, project: &ProjectId, range: AuditRange, limit: usize) -> Result<Vec<AuditEntry>>;
}
pub struct AuditRange { pub after: Option<u64>, pub before: Option<u64>, pub order: AuditOrder }
pub enum AuditOrder { OldestFirst, NewestFirst } // OldestFirst by default
```

- **Storage.** `ovra-store` keeps it in its SQLite file (4.E). The file
  mode is 0600.
- **Protocol.** `audit.list { project, after, before, order, limit }`
  returns the first `limit` entries (at most 500) with a `seq` between
  `after` and `before`, in `order`. Plus an `audit.appended` event for
  live viewers.
- **Viewer.** It opens on the newest page (`newest_first`) and reads the
  page before the oldest shown (`before`) when the person asks for older
  entries, so a long log is never read whole.
- **No edits.** The log is only appended to. No method deletes or edits an
  entry, and removing a project keeps its entries.
- **4.0 versus 4.E.** 4.0 defines the trait and the schema, with an
  in-memory fake. Writing entries from the core, the store and the viewer
  are 4.E.

## File operations

Creating, renaming, copying and trashing (`ovra-fs-index`, `edit`) check
each path's parent just before the change: it must resolve inside the
project and outside `.git`. The last component is never followed or
replaced: new files use `create_new`, renames use `RENAME_NOREPLACE`
(Linux) or `RENAME_EXCL` (macOS), and a copy opens every source file and
folder with `O_NOFOLLOW` and refuses one whose device and inode changed
since it was looked at. One race is accepted for now: a direct-mode agent
that swaps a parent folder for a symlink between the check and the change
can redirect it outside the project. The full fix is resolving every path
relative to the root's handle (`openat2` with `RESOLVE_BENEATH`, or
`cap-std`), a later task.

### Dropped files

Files and folders dropped onto the window from the system's file manager
are copied into the project only on the person's drop onto the file tree.
The desktop app keeps the dropped paths in Rust under a random id that is
used once and expires after 60 s (`ovra_fs_index::Drops`); a newer drop
replaces it. The window gets the id and copies by it with the window-only
command `ovra_import_drop`. No source path goes from the page to the app,
headless mode has no drops, and agents have no tool for it.

- The target folder is checked like `fs.copy`'s, so the parent-swap race
  described above applies here too.
- The same race exists on the source side: every source entry is opened
  without following a link and must keep its device and inode, but a
  folder above it that another process swaps for a link after the plan
  can make the copy read from where that link leads.
- The import is planned before anything changes: links are skipped and
  named, a device, pipe or socket refuses the drop, and so do more than
  10,000 entries, 1 GB or 256 nested folders. Two dropped names that one
  folder can hold only once (by case or Unicode normalization) refuse it.
- An existing name is replaced only when the person was shown it and
  answered Replace; the old entry goes to the trash first. Every refusal
  comes before the first change, and a name that appears during the copy
  stops it with `conflict` instead of being replaced.
- Copies keep the permission bits up to 0o755: no setuid, setgid, sticky
  or group and other write.
- A failed import keeps what was copied before the failure but removes a
  partly copied file.
- A dropped folder with a nested `.git` is copied as it is, as the
  system's file manager would.
- Error texts name the dropped item, not its full path.
- Windows: opening sources does not yet refuse a reparse point swapped in
  after the scan (`open_file` and `check_dir` have no Windows variant);
  pending with the Windows sandbox work.

## Tests

- Unit and integration tests use the fake sandbox and the fake netproxy, as
  for every service.
- 4.F is the one exception. Its attack tests run a real sandbox and a real
  netproxy around a scripted fake agent, never a real `claude`. They run on
  Linux CI, and on macOS for 4.B.
- The attacks, at least:
  - reading `~/.ssh`;
  - reaching a host outside the allowlist;
  - reaching 127.0.0.1 and 169.254.169.254;
  - writing `.git/hooks` and `~/.claude/settings.json`;
  - changing `~/.claude.json` `mcpServers` (the known limit);
  - widening rights from the repository.

## Tasks this unblocks

| Task | Builds on 4.0                                                                                                               |
| ---- | --------------------------------------------------------------------------------------------------------------------------- |
| 4.A  | `Sandbox` for Linux in `crates/sandbox-linux`: bubblewrap, the helper (Landlock, seccomp), inbox marks for protected paths. |
| 4.B  | `Sandbox` for macOS in `crates/sandbox-macos` (Seatbelt; `docs/specs/sandbox-macos.md`, off until verified on a Mac).       |
| 4.C  | `NetProxy` in `crates/netproxy`, project allowlists, `netproxy.*` methods.                                                  |
| 4.D  | Its own short spec first (TLS for bound hosts), then `Secrets` in `crates/secrets`.                                         |
| 4.E  | Writing `AuditEntry` from the core, the store in `ovra-store`, the viewer in `ui/src/audit`.                                |
| 4.F  | Attack tests in `tests/security`.                                                                                           |

Each of 4.A–4.F adds its own service trait or protocol module in a small
first commit if 4.0 did not, as the parallel-work rules require. 4.0 adds
`Sandbox`, `AuditLog`, the policy and audit protocol, and `AgentHost::sandboxed`.
