# Attack tests and threat model (task 4.F)

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

`src/main.rs` is the program inside the sandbox: the sandbox helper first,
as in the daemon and the app, then two probes a shell cannot do (connect
to a Unix socket path, or to an abstract one).

## Who attacks

- **The agent.** A `claude` process that follows a prompt injection in a
  file, a web page or a tool answer, or a model that misbehaves. It runs any
  command its tools allow, inside its session's sandbox.
- **The repository.** A cloned project that ships files meant to widen
  what an agent, or the person's own tools later, may do: git config and
  hooks, agent settings, editor tasks, links to secrets.

Ovra's core, its UI and the person's own terminals are trusted and never
sandboxed. The person decides what a project's allowlist and policy hold.

## What the tests check

| Attack                                                                                                                                                                            | Expected                                                  | Test                                                                    |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------- | ----------------------------------------------------------------------- |
| Read `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config/gh`, `~/.netrc`, Docker, kube, git and cargo credentials, `~/.npmrc`, browser profiles, the keyring folder                        | hidden                                                    | `an_agent_finds_no_secret_of_the_user_or_of_ovra`                       |
| Read Ovra's data folder (store, trust, policies, audit), also through a link the repository planted                                                                               | hidden                                                    | same                                                                    |
| Search every writable or hidden place for a planted secret; find a host process that holds one, or signal it                                                                      | nothing found                                             | same                                                                    |
| Find `GITHUB_TOKEN`, `GH_TOKEN`, `NPM_TOKEN`, AWS keys, `CLAUDE_CODE_OAUTH_TOKEN`, `SSH_AUTH_SOCK`, the gpg agent or `DBUS_SESSION_BUS_ADDRESS` in the environment of any process | not passed in                                             | same                                                                    |
| Write `.git/hooks`, `.git/config`, the `core.hooksPath` folder (or move it away), `.claude/`, `.mcp.json`, `CLAUDE.md`, `AGENTS.md`, `.vscode/`, `.envrc`, `.devcontainer/`       | read-only                                                 | `a_hostile_repository_cannot_widen_what_later_runs_outside_the_sandbox` |
| Fill a missing `.idea/`; poison `~/.cargo/registry` for the person's next build                                                                                                   | read-only; the write lands in the project's private cache | same                                                                    |
| Write `~/.claude/settings.json`, `CLAUDE.md`, `hooks/`, `agents/`, `commands/`, `skills/`, `plugins/`, this project's `memory/`                                                   | read-only                                                 | same                                                                    |
| Write `~/.bashrc`, `~/.profile`, `~/.gitconfig`, systemd user units, cargo config, the system                                                                                     | gone with the sandbox, or read-only                       | same: every file, folder and link is compared before and after          |
| Keep a capability, gain one through setuid, start a user namespace, mount, write a sysctl or the system                                                                           | refused                                                   | `rights_cannot_be_gained_inside_the_sandbox`                            |
| Leave a process running after the session (`setsid`, `nohup`)                                                                                                                     | killed with the sandbox                                   | `nothing_the_agent_starts_outlives_its_session`                         |
| Reach 127.0.0.1 (Ovra's daemon), 169.254.169.254, the internet over IPv4 and IPv6, an abstract Unix socket, without the proxy                                                     | no route                                                  | `the_netproxy_is_the_only_way_out_and_holds`                            |
| Use another session's proxy socket, or find its folder                                                                                                                            | not there                                                 | same                                                                    |
| Through the proxy: a host not listed (tunnel and plain), the daemon, `localhost` and `169.254.169.254` even when listed, `[::1]`, a base host on another port                     | 403, and each refusal reaches the core with its reason    | same                                                                    |

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
- **Windows** gives each session's proxy a 127.0.0.1 port that any local
  process can use (5.A).
- **Without bubblewrap** sessions run unsandboxed and say so; policies never
  skip a question for them.

## Covered elsewhere

Worker sessions (the main repository's git folder read-only, the
worktree's `commondir`, `gitdir`, `config.worktree` and `.git` file),
nested repositories, submodules, git include files and links on protected
names are attacked in `crates/sandbox-linux/tests/sandbox.rs`; dropping
home folder entries from `PATH` in its plan tests. The netproxy's parsing
(framing, pipelining, `Transfer-Encoding`) is in `crates/netproxy`.

## Not covered yet

- Request smuggling and searching the sandbox for a bound secret: added
  when 4.D part 2 (secrets through the netproxy) is finished.
- macOS (4.B) and Windows (5.A) run these attacks once their sandboxes exist.
