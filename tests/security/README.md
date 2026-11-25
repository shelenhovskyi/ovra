# Attack tests and threat model (tasks 4.F, L.D)

`tests/attacks.rs` runs the real Linux sandbox (`ovra-sandbox-linux`, built
as `ovra_sandbox::for_this_os` builds it) and a real netproxy
(`ovra-netproxy`) around shell scripts that play a hostile agent. No real
`claude` runs. Each script line is one attack and prints `yes` if it
worked; the snapshots in `tests/snapshots/` hold the results, so a change
in what holds shows up in review. Each test also runs its script once
without the sandbox, as a control: there every attack must work, so a typo
or a missing tool cannot pass for a refusal. (The rights test runs its
control only as root, and never the lines that would change the machine.)
The test folders live under Cargo's target folder, not the temporary
folder, so the home and data folders are hidden on their own. The tests are part of `just test`. They
take well under a second, and they skip (and say so) where bubblewrap
cannot run, except in CI or where `/usr/bin/bwrap` exists.

The secret test plays the user's own proxy and binds the secret to
`bound.invalid`, a name that never resolves, so the netproxy hands the
tunnel to that proxy and no request leaves the machine. The netproxy runs
in a child process (`secret_proxy`, an ignored test) because it reads
`HTTPS_PROXY` from its own environment. A request that passes the checks
gets 502 there, because the fake proxy hangs up before TLS.

`src/main.rs` is the program inside the sandbox: the sandbox helper first,
as in the daemon and the app, then two probes a shell cannot do (connect
to a Unix socket path, or to an abstract one).

## Who attacks

- **The agent.** A `claude` process that follows a prompt injection in a
  file, a web page or a tool answer, or a model that misbehaves. It runs any
  command its tools allow, inside its session's sandbox.
- **The repository.** A cloned project that ships files meant to widen
  what an agent, or the person's own tools later, may do: git config and
  hooks, agent settings, editor tasks, links to secrets. In a trusted
  project it also runs code through its language servers.

Ovra's core, its UI and the person's own terminals are trusted and never
sandboxed. The person decides what a project's allowlist and policy hold.

## What the tests check

| Attack                                                                                                                                                                                                                                                                                                                                             | Expected                                                                                                                                                                                                                                       | Test                                                                    |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Read `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config/gh`, `~/.netrc`, Docker, kube, git and cargo credentials, `~/.npmrc`, browser profiles, the keyring folder                                                                                                                                                                                         | hidden                                                                                                                                                                                                                                         | `an_agent_finds_no_secret_of_the_user_or_of_ovra`                       |
| Read Ovra's data folder (store, trust, policies, audit), also through a link the repository planted                                                                                                                                                                                                                                                | hidden                                                                                                                                                                                                                                         | same                                                                    |
| Search every writable or hidden place for a planted secret; find a host process that holds one, or signal it                                                                                                                                                                                                                                       | nothing found                                                                                                                                                                                                                                  | same                                                                    |
| Find `GITHUB_TOKEN`, `GH_TOKEN`, `NPM_TOKEN`, AWS keys, `CLAUDE_CODE_OAUTH_TOKEN`, `SSH_AUTH_SOCK`, the gpg agent or `DBUS_SESSION_BUS_ADDRESS` in the environment of any process                                                                                                                                                                  | not passed in                                                                                                                                                                                                                                  | same                                                                    |
| Write `.git/hooks`, `.git/config`, the `core.hooksPath` folder (or move it away), `.claude/`, `.mcp.json`, `CLAUDE.md`, `AGENTS.md`, `.vscode/`, `.envrc`, `.devcontainer/`                                                                                                                                                                        | read-only                                                                                                                                                                                                                                      | `a_hostile_repository_cannot_widen_what_later_runs_outside_the_sandbox` |
| Fill a missing `.idea/`; poison `~/.cargo/registry` for the person's next build                                                                                                                                                                                                                                                                    | read-only; the write lands in the project's private cache                                                                                                                                                                                      | same                                                                    |
| Write `~/.claude/settings.json`, `CLAUDE.md`, `hooks/`, `agents/`, `commands/`, `skills/`, `plugins/`, this project's `memory/`                                                                                                                                                                                                                    | read-only                                                                                                                                                                                                                                      | same                                                                    |
| Write `~/.bashrc`, `~/.profile`, `~/.gitconfig`, systemd user units, cargo config, the system                                                                                                                                                                                                                                                      | gone with the sandbox, or read-only                                                                                                                                                                                                            | same: every file, folder and link is compared before and after          |
| Keep a capability, gain one through setuid, start a user namespace, mount, write a sysctl or the system                                                                                                                                                                                                                                            | refused                                                                                                                                                                                                                                        | `rights_cannot_be_gained_inside_the_sandbox`                            |
| Leave a process running after the session (`setsid`, `nohup`)                                                                                                                                                                                                                                                                                      | killed with the sandbox                                                                                                                                                                                                                        | `nothing_the_agent_starts_outlives_its_session`                         |
| Reach 127.0.0.1 (Ovra's daemon), 169.254.169.254, the internet over IPv4 and IPv6, an abstract Unix socket, without the proxy                                                                                                                                                                                                                      | no route                                                                                                                                                                                                                                       | `the_netproxy_is_the_only_way_out_and_holds`                            |
| Use another session's proxy socket, or find its folder                                                                                                                                                                                                                                                                                             | not there                                                                                                                                                                                                                                      | same                                                                    |
| Through the proxy: a host not listed (tunnel and plain), the daemon, `localhost` and `169.254.169.254` even when listed, `[::1]`, a base host on another port                                                                                                                                                                                      | 403, and each refusal reaches the core with its reason                                                                                                                                                                                         | same                                                                    |
| With a secret bound to a host: find its value in the environment, a process or a file, or the session CA's key; smuggle a request past the parser (length and chunked, two lengths, odd codings, folded lines, bare LF, `Upgrade`, `CONNECT` in the tunnel, another authority, missing, repeated or foreign `Host`, a second request in one write) | nothing found; 400 with the reason for each refused form, 502 for well-formed requests (they passed the checks; TLS to the fake upstream then fails). That a second request is dropped is unit-tested in `crates/netproxy/src/inject/tests.rs` | `a_bound_secret_never_enters_the_sandbox`                               |

A language server (ADR 0007) runs the project's code too (`build.rs`,
proc macros, plugins), so it gets its own sandbox (`wrap_server`).
`a_language_server_reads_the_project_and_writes_only_its_cache` plays one
with a shell script: it may read the project and the user's package
caches and write only its cache folder and temporary files. Writing the
project or `~/.cargo/registry`, reading `~/.ssh`, `~/.claude` or Ovra's
data, reaching the daemon or the internet, and finding
`ANTHROPIC_API_KEY`, `GITHUB_TOKEN` or a planted secret in any process
environment all fail; on the host only the cache folder changes. Which
profiles the sandbox refuses for a server (any network, a project that
holds the home folder, a cache folder in the project) is in
`crates/sandbox-linux/src/plan/tests/server.rs`.

## Known limits

These hold today and are recorded on purpose. The first three are attacks
in `the_known_limits_are_what_the_threat_model_says`, and the created
`CLAUDE.local.md` and the project's cache are in the `changed_on_the_host`
snapshot, so fixing one of those changes a snapshot. The rest come from the
spec.

- **Claude's own login.** The agent process reads
  `~/.claude/.credentials.json` (and rewrites it on refresh), so its tools
  can too. The netproxy is what keeps the login from leaving the machine:
  only the base hosts and the project's allowlist are reachable, so a
  broad entry is a way out for it too (see "Channels out through allowed hosts").
- **`~/.claude.json`.** `claude` must write it, so an agent can add
  `mcpServers` there that the person's own `claude` starts later. The trust
  fingerprint is the backstop for Ovra's sessions.
- **Transcripts.** This project's transcripts under `~/.claude/projects/`
  stay writable, because a resume reads them; an agent can change what a
  later resume sees.
- **Protected files that do not exist yet.** A missing `CLAUDE.md`,
  `CLAUDE.local.md`, `AGENTS.md`, `.mcp.json` or `.envrc` can be created,
  and so can a `CLAUDE.md` in a new folder: empty placeholders would show
  up in git status and break tools. Missing protected folders (`.claude/`,
  `.vscode/`, `.idea/`, `.devcontainer/`) are made empty and read-only. The
  review marks changes to protected paths; in direct mode there is no
  review.
- **Channels out through allowed hosts.** Without TLS inspection a tunnel
  to an allowed host can carry anything: a broad entry such as
  `github.com`, domain fronting through a shared CDN address, or
  `api.anthropic.com` with someone else's API key. The UI warns about broad
  entries.
- **Names only the user's proxy resolves.** With an upstream proxy, a name
  that does not resolve where Ovra runs is left to that proxy, which may
  resolve it to a private address. That includes names in `/etc/hosts`:
  the proxy resolves names fully qualified (`name.`), which glibc's
  `/etc/hosts` lookup does not match. `localhost` and names under it are
  always loopback: refused, and never sent to the user's proxy (fixed in
  4.F).
- **Package caches** (`~/.cargo/registry`, `~/.npm`, ...) are private to
  the project, not to the session: what one session puts there, the
  project's later sessions build with. The person's own caches stay as
  they were.
- **Gaps the spec lists for Linux**: a git config value that names a file
  inside the project, a submodule the person adds while a session runs,
  and a bind mount of the home folder outside `/mnt`, `/media`, `/sysroot`
  and `/.snapshots`.
- **Windows** (`docs/specs/sandbox-windows.md`): each session's proxy is a
  127.0.0.1 port that any local process can use, and the sandbox user's
  firewall rule does not filter loopback, so other 127.0.0.1 services on
  the machine stay reachable. Name lookups go through the system's
  resolver service, so DNS is a slow channel out. `~/.claude`'s session
  folders (`todos/`, `plans/`, ...) are the person's own, shared with
  their sessions; `shell-snapshots/` and `session-env/` stay read-only.
  Sessions are sandboxed only after the one-time setup (administrator);
  until then they run unsandboxed and say so. Closing loopback and DNS
  needs WFP filters for the sandbox user, a follow-up.
- **Without bubblewrap** sessions run unsandboxed and say so; policies never
  skip a question for them.
- **Unix sockets in visible folders.** Landlock does not limit connecting
  to a socket file, so an agent can reach a socket that something on the
  host listens on inside a folder the sandbox shows (the project, a
  toolchain folder). `/run` and `/tmp` are hidden, so the system's sockets
  (D-Bus, Docker) are not reachable, and abstract sockets are not (new
  network namespace). Language servers are refused `socket(AF_UNIX)` by
  seccomp (`socketpair` still works); agents are not yet, as their tools
  (the netproxy bridge, build tools) have not been checked without it.

## Covered elsewhere

Worker sessions (the main repository's git folder read-only, the
worktree's `commondir`, `gitdir`, `config.worktree` and `.git` file),
nested repositories, submodules, git include files and links on protected
names are attacked in `crates/sandbox-linux/tests/sandbox.rs`; dropping
home folder entries from `PATH` in its plan tests. The netproxy's parsing
(framing, pipelining, `Transfer-Encoding`) is in `crates/netproxy`.

The keychain attack the secrets spec asks for is the sum of lines above:
`DBUS_SESSION_BUS_ADDRESS` is not passed in, `/run` (and so
`/run/user/<uid>/bus`) is hidden, and abstract sockets are out of reach.

## Not covered yet

- macOS (4.B) runs these attacks once its sandbox exists. On Windows (5.A)
  the CI job runs the setup and a session as the sandbox user
  (`crates/sandbox-windows/tests/setup.rs`: writes, protected paths, the
  home folder, the internet); this suite's attacks still need a Windows
  port of its fake agent.
