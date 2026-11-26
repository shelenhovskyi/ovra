# The macOS sandbox (4.B)

Status: built, not yet verified on a Mac. The owner chose on 2026-11-26 to
build Ovra's own macOS sandbox (`docs/specs/more-agents.md`, "Open
question"). Until the checklist below has passed on a real Mac,
`ovra-sandbox-macos` reports itself unavailable, so a Mac keeps today's
behaviour: Claude runs unsandboxed and says so, Gemini and Codex refuse to
start. Builds on `docs/specs/security.md` (what every platform's sandbox
must do) and ADR 0011 (`ProviderLists`).

## Design

- **Seatbelt through `/usr/bin/sandbox-exec`.** `wrap` returns a line that
  starts Ovra's own binary as the helper (`--ovra-sandbox-helper`), with
  the session's profile as an argument. Only `/usr/bin/sandbox-exec` is
  used, never one found on `PATH`. `sandbox-exec` is deprecated but still
  ships with macOS and is what other agent sandboxes use; if Apple removes
  it, the probe fails and sessions say why.
- **The profile** (`src/sbpl.rs`) denies everything and then allows. It
  has no deny rules: what a rule must not reach is a `require-not`
  condition inside it, so nothing depends on how Seatbelt orders rules.
  Folders a rule gives are matched as written (`subpath`, `literal`);
  folders it keeps out are matched by patterns that ignore case, because
  macOS volumes usually ignore case (`.GIT/hooks` is `.git/hooks`). Paths
  are written into the text; one with a `"`, a `\` or a control character
  is refused.
- **Reading.** Everything outside the hidden folders: the home folder,
  Ovra's data folder, the temporary folder (every session's netproxy
  socket), `/private/tmp`, `/private/var/tmp`, `/private/var/folders`,
  `/Volumes`, `/System/Volumes/Data` (the data volume under its own
  name) and `/.vol` (files by volume and inode number). Given back: the work dir, the profile's `read` and `write`, the
  agent's state (below), git's settings in the home folder, toolchain
  homes and `PATH` folders under the home folder (a toolchain's `bin`
  brings the toolchain), the program's own folder, the CA bundle.
  Never readable, whatever gives them back: `~/.ssh`, `~/.aws`, `~/.gnupg`,
  `~/.docker`, `~/.kube`, `~/.netrc`, `~/.config/gh`, `~/Library/Keychains`,
  `Cookies`, `Mail`, `Messages`, and `credentials`, `credentials.toml`,
  `credentials.json` in a readable toolchain folder. Metadata (whether a
  file exists, its size) is readable everywhere, because tools resolve
  every folder of a path.
- **Writing.** The work dir (unless research), the profile's `write`, the
  project's own package caches and the work dir's own temporary folder in
  Ovra's data folder
  (`CARGO_HOME`, `npm_config_cache`, `PIP_CACHE_DIR`,
  `npm_config_store_dir` and `TMPDIR` point at them, as on Windows), and
  the agent's state. Never: the protected paths of `ovra-sandbox` (`**/`
  at any depth), every `.git` at any depth, the `core.hooksPath` folder,
  config files git includes from the work dir, submodule folders that are
  not checked out, and the target of a protected name that is a link.
  Seatbelt checks every access, also of paths that do not exist yet, so
  unlike on Linux nothing is made before a session: a missing `CLAUDE.md`
  or `.claude` cannot be created, at any depth.
- **The agent's state** comes from the session's `ProviderLists`: its
  configuration folder read-only, `in_place` folders writable (Seatbelt
  cannot make folders private to the session), this work dir's
  transcripts folder writable without its `memory`, `files` and the state
  file writable. An account's own folder (`CLAUDE_CONFIG_DIR`) replaces
  the default one, with the same checks as on Linux.
- **Network.** `Blocked`: none. `Proxy`: the helper starts a bridge
  outside the sandbox on a free port of `127.0.0.1` that forwards to the
  session's netproxy socket, puts the port in the profile, and sets
  `HTTP_PROXY`, `HTTPS_PROXY` (both cases) and `NODE_USE_ENV_PROXY=1`; the
  profile lets the sandbox reach `localhost:<that port>` and nothing else.
  The proxy URL holds a random per-session token
  (`http://ovra:<token>@127.0.0.1:<port>`); the bridge relays a
  connection only when its first request carries it as
  `Proxy-Authorization: Basic`, strips that header, and answers anything
  else with 407. No DNS lookups: the netproxy resolves names. `Open`: any
  IP address, outbound only. Unix sockets only inside the session's
  temporary folder, in every mode. The bridge stops once the agent has
  ended.
- **Processes.** The helper calls `setsid` and then becomes
  `sandbox-exec` with `exec`, so the process Ovra started and stops is the
  sandboxed agent, and it has no controlling terminal. Signals and
  process information reach only the sandbox's own processes.
- **System services** (`mach-lookup`): user and group lookups,
  preferences (read only), logging, notifications, certificate checks and
  the keychain, which `claude` keeps its login in. No Launch Services and
  no `launchd` jobs, which would start programs outside the sandbox.
  `git-credential-osxkeychain` (by name, anywhere, any case) may neither
  run nor be read, so it cannot be copied and run from elsewhere, and
  git's `credential.helper` is emptied (`GIT_CONFIG_COUNT`), so git does
  not hand an agent the person's GitHub login from the keychain.
  Terminals: only ptys the sandbox opened itself; no `/dev/tty` (there is
  no controlling terminal anyway) and no `ioctl` on any other terminal, so
  no `TIOCSTI` into the person's shell.
- **Environment.** The same allowlist as on Linux; the helper starts
  `sandbox-exec` with only the kept names.
- **Language servers** (`wrap_server`): the project read-only, the
  server's cache folder (inside `<data>/lsp`) the only writable folder,
  the person's package caches read-only, no agent state, no network, no
  Unix sockets.
- **Refused for now:** server pods (macOS has no network namespace, so a
  pod's servers would listen on the Mac's loopback), the agent browser,
  and `forward` ports are left out (an agent cannot reach its work dir's
  servers).

## Known gaps

Sessions report these (`GAPS` in `src/lib.rs`):

- the keychain: `security find-generic-password` answers for any item
  whose access list trusts `/usr/bin/security`, among them the `gh` login
  (`gh:github.com`), and items can be added, changed and deleted. `claude`
  needs `security` for its own login, so a profile rule cannot close it;
  the owner must accept this before the sandbox is turned on. A copy of
  `git-credential-osxkeychain` the agent downloads (through an allowed
  host) is signed the same and still works, as `security` does;
- other apps' preferences, read through `cfprefsd` (`defaults read`);
- whether a file exists, and its size, anywhere on the Mac;
- the session's proxy and its token, for the person's other programs that
  can read the agent's environment (`ps -E`): through it they reach the
  netproxy, which adds the secrets bound to the project, for as long as
  the session runs. Other users of the Mac see the port, not the token;
- processes an agent leaves running after its session (no PID namespace;
  they stay sandboxed; a later bridge on the same port has another
  token);
- a dev build of Ovra inside a writable work dir (as on Linux);
- git config values that name project files (as on Linux);
- the agent's session folders in its settings (`todos`, `logs`, ...) are
  the person's own, not private to the session (as on Windows).

## Turning it on

`VERIFIED` in `crates/sandbox-macos/src/lib.rs` is `false`. For the
checklist, start Ovra with `OVRA_MACOS_SANDBOX=try` in its environment.
When every step passes, set `VERIFIED` to `true` in its own commit, which
names the macOS version and the Mac it was checked on.

## Checklist on a Mac

Run from a build of `main` (`just dev-headless`) with
`OVRA_MACOS_SANDBOX=try`, a project under the home folder that is a git
repository, and a fake or real Claude session. Each line is something to
do inside a sandboxed session (Bash tool, or
`/usr/bin/sandbox-exec -p "$(...)"` with a profile from the plan) and what
must happen.

1. The sandbox report says `Seatbelt` with the gaps above; Settings show
   no probe error.
2. `claude` starts, is signed in (keychain), answers, and a resume of the
   session works.
3. `cat ~/.ssh/id_*`, `cat ~/.SSH/id_*`, `ls ~`, `ls ~/Documents`,
   `cat ~/.aws/credentials`, `cat ~/.cargo/credentials.toml`, and other
   spellings of the same paths: `cat //Users/$USER/.ssh/id_*`,
   `cat /Users/./$USER/.ssh/id_*`, `cat /.nofollow$HOME/.ssh/id_*`,
   `cat /.resolve/1$HOME/.ssh/id_*`, and `ls /var/folders`: all refused.
4. `ls "$(getconf DARWIN_USER_TEMP_DIR)"` and `ls /tmp`: refused; the
   netproxy's socket folder is not readable.
5. `cat /System/Volumes/Data$HOME/.ssh/id_*`: refused.
6. In the work dir: editing a source file works; `echo x >> .git/config`,
   `touch .git/hooks/pre-commit`, `touch .GIT/hooks/x`, `mkdir -p
   sub/.git`, `echo x > sub/CLAUDE.md`, `echo x > .claude/settings.json`,
   `mkdir .claude` (when missing), `mv .git g`, `rm -rf .vscode`, and
   `mkdir .vſcode` (long s; does `ls -d .vscode` then show it?): all
   refused.
7. Hard links and clones: `ln .git/config x && echo pwn >> x` is
   refused, or `.git/config` is unchanged afterwards; `ln ~/.ssh/id_* x &&
   cat x`, `cp -c ~/.ssh/id_* x` and `cat /.vol/<dev>/<inode>` (from
   `stat -f '%d %i'`) are refused. If a link or clone works, the sandbox
   must stay off until a `file-link` rule closes it.
8. A protected name that is a link (`.claude -> notes`): writing through it
   is refused.
9. Writing `~/.claude/settings.json`, `~/.claude/agents/x.md`,
   `~/.claude.json` key `mcpServers` (allowed: the known limit), and the
   transcripts folder's `memory/`: only `~/.claude.json` succeeds.
10. Network: `curl https://example.com` with the project allowlist empty is
    refused by the netproxy (an entry in the audit log); after adding
    `example.com` it works. `curl --noproxy '*' https://example.com`,
    `nc 1.1.1.1 443`, `nslookup example.com` and `curl
    http://127.0.0.1:<Ovra's port>` all fail.
11. Another session's proxy: from session A, `curl -x
    http://127.0.0.1:<session B's port>` fails. Outside the sandbox, in
    Terminal: `curl -x http://127.0.0.1:<port> https://example.com` and
    the same with a wrong token (`-x http://ovra:x@127.0.0.1:<port>`) get
    407; with the session's token (from `HTTPS_PROXY`) it works.
12. `open -a Calculator`, `osascript -e 'tell app "Terminal" to do script
    "id"'`, `launchctl submit -l x -- /usr/bin/touch /tmp/x`: none starts
    anything.
13. `defaults write com.apple.Terminal x 1`: refused.
14. `printf 'protocol=https\nhost=github.com\n\n' | git credential fill`
    and `.../git-credential-osxkeychain get` give no token; copying it
    (`cp "$(git --exec-path)/git-credential-osxkeychain" x`, `cat ... > x`,
    `cp -c`, `ln` into the work dir) and running the copy, and running it
    as `GIT-Credential-OSXKeychain`, are all refused; a downloaded copy is
    the known gap (write down whether it answers). `security
    find-generic-password -s gh:github.com -w` is the known gap (write
    down whether it answers). `cat /dev/ttys000` with another Terminal
    open is refused; `defaults read com.apple.Terminal` is the known gap.
    With Ovra started from Terminal (`just dev-headless`): `ps -o
    tty= -p $$` shows no terminal, `exec 3<>/dev/tty` fails, and `perl -e
    'require "sys/ioctl.ph"; ioctl(STDERR, &TIOCSTI, "x")'` (and the same
    on `/dev/tty`) fails: nothing appears in that Terminal's shell.
    With `Open` network: `curl --unix-socket /var/run/docker.sock
    http://x/version` fails.
15. `env` shows none of `GITHUB_TOKEN`, `SSH_AUTH_SOCK`,
    `CLAUDE_CODE_OAUTH_TOKEN`; `HTTPS_PROXY` names the bridge.
16. Builds work through the netproxy: `cargo build` of a crate with one
    dependency, `npm install` of one package, `git fetch` over HTTPS.
17. Stopping the session in Ovra ends `claude`; the bridge process is gone
    within a second.
18. A research session cannot write its folder; a worker cannot write the
    main checkout or `git commit`.
19. A language server starts (rust-analyzer or the TypeScript server),
    reads the project and cannot write it.
20. The attack tests in `tests/security` that are not Linux-only pass.

Record each result in the commit that turns it on. A step that fails is a
bug in `ovra-sandbox-macos`, not a gap to add, unless the owner says so.
