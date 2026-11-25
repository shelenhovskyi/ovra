# Windows sandbox (5.A)

Status: proposed, waiting for the owner (it creates a Windows account and
needs administrator rights once). Builds on `docs/specs/security.md`, whose
contract (`Sandbox`, profiles, protected paths, environment allowlist,
private package caches, the netproxy as the only way out) this keeps.

## What was tried

Three spikes on GitHub's `windows-latest` runner (2026-11-25, branch
`claude/project-thread-2o3xkq`, `spike/win-sandbox`). Each ran one probe
program: reach the network, read a planted secret in the home folder,
write inside and outside a granted folder, run git, Git Bash, cmd and
PowerShell, and attack a protected folder (write, delete, rename, change
its ACL, take ownership).

| Probe                                  | AppContainer | Other local user + firewall rule | Restricted token, same user |
| -------------------------------------- | ------------ | -------------------------------- | --------------------------- |
| Internet (TCP to 1.1.1.1)              | blocked      | blocked                          | open                        |
| DNS                                    | blocked      | open (system resolver)           | open                        |
| A 127.0.0.1 port of another process    | blocked      | open                             | open                        |
| Loopback inside the sandbox            | works        | works                            | works                       |
| AF_UNIX socket in a granted folder     | works        | works                            | works                       |
| Read a secret in the user's home       | denied       | denied                           | denied                      |
| Read a granted folder in the home      | works        | (not tried)                      | works                       |
| Open `NUL`                             | **denied**   | works                            | works                       |
| git                                    | **fails**    | works                            | works                       |
| Git Bash (MSYS)                        | **fails**    | works                            | **fails** (see below)       |
| Deny ACE on a protected folder         | **ignored**  | (not tried)                      | holds                       |
| Change the ACL of a file from inside   | denied       | (not tried)                      | denied                      |
| Open the parent process                | (not tried)  | (not tried)                      | denied                      |

What this means:

- **AppContainer** is ruled out. `NUL` cannot be opened, so git dies at
  start (`could not open '/dev/null'`); MSYS cannot create its objects in
  `\BaseNamedObjects`, so Git Bash, which `claude` needs for its Bash tool
  on Windows, never starts; and deny ACEs for the container's SID are not
  applied, so a protected path inside the writable project cannot be kept
  read-only. OpenAI reached the same conclusion for Codex
  (https://openai.com/index/building-codex-windows-sandbox/).
- **A restricted token** keeps writes to granted folders and holds deny
  ACEs, so protected paths work. It needs the logon SID among its
  restricting SIDs and in its default DACL, or no child process starts.
  A fully restricted token (reads too) hides the home folder but breaks
  Git Bash (`couldn't create signal pipe`) and PowerShell (HKCU); a
  write-restricted token, as Codex uses, keeps them working but reads as
  the user, home folder included. It cannot limit the network at all:
  firewall rules cannot match a restricted token.
- **Another local user** hides the user's home folder, keychain
  (Credential Manager) and processes by plain Windows rules, and a
  firewall rule for that user blocks the internet. Loopback and DNS stay
  open: the firewall does not filter loopback, and name lookups go through
  the system's resolver service.
- **WSL2** was not tried: `claude` would have to be installed and signed
  in inside the Linux distribution, the project would sit on the slow
  `/mnt/c` share or inside WSL, and the Linux sandbox would run there. That
  is a different product shape, not a Windows sandbox.

## Proposal

The Codex model: a sandbox user plus a write-restricted token.

**Setup, once, with administrator rights.** The UI offers "Set up the
Windows sandbox"; Ovra starts itself elevated (one UAC prompt) with a setup
flag that:

- creates the local account `OvraSandbox` (member of `Users` only, a long
  random password, hidden from the sign-in screen);
- keeps that password with DPAPI for the person's own account, in Ovra's
  data folder, which the sandbox user cannot read;
- adds one firewall rule that blocks all outbound traffic of that user.

Until setup has run, `status()` is `Unavailable` with the reason "set up
the Windows sandbox (administrator, once)", and sessions run unsandboxed as
they do today. A removal flag deletes the account, the rule and every ACE
Ovra added (it keeps a list).

**Each session.**

1. `wrap` returns a line that starts Ovra itself with the helper flag
   (as on Linux) and a plan file in the session's private folder.
2. The helper, as the person, starts a second helper as `OvraSandbox`
   (`CreateProcessWithLogonW`), in a job object that kills everything when
   the session stops.
3. The second helper makes a write-restricted token: restricting SIDs
   Everyone, its logon SID and the project's own SID (a SID made from the
   project key, used only in ACEs), privileges removed, default DACL for
   the logon SID and the project SID. It bridges the netproxy port, then
   starts `claude` with that token and the kept environment.

Writes then need both the sandbox user's right and the project SID's
right, so one project's agents cannot write another project's folders even
though all of them run as `OvraSandbox`.

**Files.** Grants are ACEs for `OvraSandbox` and the project SID. They are
added the first time a project starts a session (with progress, since
NTFS writes the inherited ACE into every file) and kept: a later session
starts at once, and files the person creates later inherit them. Applying
ACEs never follows junctions or links (the spike's `icacls /T` followed a
junction to `C:\Users`).

| What                                                 | Right                          |
| ---------------------------------------------------- | ------------------------------ |
| `work_dir`, `write` folders, the session's temp      | modify (no delete-child)       |
| `read` folders                                       | read                           |
| `claude`'s own folder, toolchains on `PATH`          | read and execute               |
| `~/.claude.json`, `.credentials.json`, history       | modify (the known limit)       |
| `~/.claude` and this project's transcript folder     | read; the transcripts modify   |
| `~/.gitconfig`, `~/.config/git/{config,ignore,attributes}` | read                     |
| The project's package caches in Ovra's data folder   | modify                         |
| Protected paths (the shared list)                    | deny write, delete, ACL, owner |

Everything else in the home folder stays unreadable, because it belongs to
the person only. System folders stay readable, as for every user.

- **Protected paths**: the same list as Linux, with deny ACEs (they hold:
  write, create, delete, rename, `icacls /reset`, `takeown` all failed).
  Missing `.claude/`, `.vscode/`, `.idea/`, `.devcontainer/` are made empty
  first. A protected path that is a junction or link refuses the session.
  The work folder's grant has no delete-child right, so a protected child
  cannot be removed through its parent.
- **`~/.claude` session folders**: Windows has no per-session view of a
  folder. `claude` writes `todos/`, `shell-snapshots/` and the like
  directly in the person's `~/.claude`, which their own sessions read.
  `shell-snapshots/` and `session-env/` (sourced later) get a deny-write
  ACE; if `claude` cannot run without them, this becomes a known limit.
- **Package caches**: no bind mounts, so variables point the tools at the
  project's copy: `CARGO_HOME` (registry and git only; `cargo` itself is
  found on `PATH`), `npm_config_cache`, `PIP_CACHE_DIR`,
  `npm_config_store_dir`. The person's own caches are not readable.

**Network.** The netproxy keeps its 127.0.0.1 port. `HTTP_PROXY` and
`HTTPS_PROXY` point at it, `NO_PROXY` is cleared, `NODE_USE_ENV_PROXY=1`.
Known gaps, recorded in `tests/security/README.md`: other 127.0.0.1
services stay reachable, and DNS lookups still leave the machine through
the system resolver (a slow channel for data). Closing loopback needs WFP
filters for the sandbox user, a follow-up.

**Environment.** `ENV_KEPT` as on Linux, plus `SystemRoot`, `windir`,
`ComSpec`, `PATHEXT`, `USERPROFILE`, `HOMEDRIVE`, `HOMEPATH`, `APPDATA`,
`LOCALAPPDATA`, `ProgramData`, `ProgramFiles`, `ProgramFiles(x86)`,
`CommonProgramFiles`, `NUMBER_OF_PROCESSORS`, `PROCESSOR_ARCHITECTURE`,
`OS`, and `TEMP`/`TMP` set to the session's own folder. `USERPROFILE`
stays the person's, so `claude` finds its login.

**Crates.** `crates/sandbox-windows` (`ovra-sandbox-windows`), picked by
`for_this_os` on Windows. The plan (what to grant, deny, keep) is plain
data built from paths, compiled and unit-tested on every OS; the Win32
calls (`windows-sys`) are `cfg(windows)`.

**Tests.** Plan snapshots on every OS. On the Windows CI job (an
administrator, so setup can run): the setup, the helper chain, and the
4.F attacks against a fake agent.

## Questions for the owner

1. The model above (a Windows account and a firewall rule, made with one
   UAC prompt), or no administrator at all: a write-restricted token only,
   which reports the network as `Open`, leaves the home folder and
   Credential Manager readable, and so only stops writes.
2. Whether DNS and other 127.0.0.1 services being reachable is acceptable
   for a first version.
