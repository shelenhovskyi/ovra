# Worktree setup

Status: approved by the owner on 2026-11-25 with the recommended option of
every decision below; task 7 of `docs/roadmap-next.md`.
Builds on `docs/specs/orchestration.md`, `docs/specs/security.md` and
`docs/specs/servers-and-browser.md`. Research: improvement report 1.5 and
`research/git-lsp-gaps.md` (setup scripts) in the project files.

## Problem

A worker's worktree today is a bare checkout of a snapshot commit. It has
no `node_modules`, no build output, no `.env`, and its package caches start
empty, so the first thing every worker does is a slow install, or it fails
because a file it needs is ignored by git. Old worktrees are removed safely,
but nobody sees how much disk they use.

## Goals

- A worker starts in a worktree that is ready to build and test.
- What gets copied in and what runs to prepare it is the person's choice,
  in files an agent cannot change.
- Workers of one project do not download the same packages again.
- Two worktrees never fight over a port.
- The person sees the disk each worktree uses, and nothing is lost when
  one is removed.

Not goals: setup for the main checkout (the person already set it up),
run scripts (servers are `.ovra/servers.json`), Windows and macOS until
their sandbox exists, moving a session between the main checkout and a
worktree. Where no sandbox exists, setup fails closed: it does not run,
and the worker's setup says why (an earlier draft ran it unsandboxed;
the implementation chose the safer way).

## Copying ignored files: `.worktreeinclude`

The same file Claude Code and Codex read, so one file serves all three.

- At the project root, gitignore syntax. A path is copied when it matches
  a line **and** git ignores it. Tracked files already come from git.
- Ovra copies before the setup command, outside the sandbox, from the main
  checkout into the new worktree. It never follows a symlink (a link is
  skipped and named in the log), never copies anything under `.git`, and
  never writes outside the worktree.
- A file is copied as a copy-on-write clone where the file system can
  (`clonefile` on APFS, `FICLONE` on btrfs and XFS, block cloning on
  ReFS), else as a plain copy. Plain copies stop at 256 MiB in total; the
  rest is skipped and listed.
- A folder line (`node_modules/`, `target/`) is cloned only with
  copy-on-write, which costs no space until a side changes. Without it the
  folder is skipped with a note, because a plain copy of `node_modules` is
  how other tools fill a disk in minutes. Worktrees live in Ovra's data
  folder, so cloning works only when that is on the same volume as the
  project; otherwise folders are skipped and the log says why.
- `.worktreeinclude` joins the protected paths (`PROTECTED_IN_WORK_DIR`):
  an agent in the main checkout could otherwise add `.env` to it and
  read the result in its next worker. This only narrows access.
- Copied files are the person's. `.env` copied into a worktree is visible
  to that worker's agent, which is what the person asked for by listing
  it. The setup dialog says so next to each secret-looking name (`.env*`,
  `*.pem`, `*.key`, `credentials*`) and points to project secrets
  (`docs/specs/secrets.md`) for tokens, which never enter the sandbox.

## Setup command: `.ovra/worktree.json`

```json
{
  "setup": [
    { "command": ["pnpm", "install", "--frozen-lockfile"] },
    { "command": ["sh", ".ovra/setup.sh"], "timeout": 900 }
  ]
}
```

- Steps run in order, each a program and its arguments without a shell,
  in the worktree's root. `timeout` is in seconds, default 600, at most
  3600. A script is just a step that runs a file; a script under `.ovra/`
  is protected like the list itself.
- **Where it runs.** In a server pod of the worktree (the agent's sandbox
  without the agent's state: `docs/specs/servers-and-browser.md`), with
  the project's package caches, through a netproxy with the project's
  allowlist. The steps run the project's own code (`postinstall`,
  `build.rs`), and that code may come from an agent's uncommitted work in
  the main checkout, so it gets no more than the worker will. A refused
  host shows in the log with the same "Allow" as for agents.
- **Environment.** The pod's allowlist (`ENV_PERSON` and `ENV_TOOLS`)
  plus:
  - `OVRA_WORKTREE_ROOT`: the worktree's path;
  - `OVRA_MAIN_ROOT`: the main checkout's path, for scripts that print or
    compare it. On sandboxed platforms it is **not readable** from inside;
    use `.worktreeinclude` to bring files over;
  - `OVRA_PORT`: the first of the worktree's ports (below).
- **When.** After the worktree is created and before the worker's first
  turn. The worker shows `preparing` with the current step and its log.
  A preparing worker counts toward the 3 agents that may work at once: it
  uses the machine like one.
- **Failure or timeout.** The worker does not start. The coordinator's
  `spawn_agent` result (or the worker's status, if it was queued) says
  which step failed with the last 50 lines of its log, so the coordinator
  can retry, start it anyway (`spawn_agent` gains `skip_setup`, a
  `tools.json` change that needs an independent security review), or tell the person. The person sees the same in the worker's row, with Retry and
  Start anyway.
- Without the file nothing runs, as today. A broken file is an error the
  Agents panel shows; the worker starts without setup and its first
  message says so.
- The Servers dialog's new-project flow (roadmap task 5) can offer to
  create the file with a guess from the lockfiles (`pnpm-lock.yaml`,
  `Cargo.lock`, ...); the person saves it.

## Ports: `OVRA_PORT`

- On Linux each pod has its own network namespace, so two worktrees both
  run `web` on 5173 without a clash; `servers.json` ports stay as they are.
- Where loopback is shared (Windows' sandbox, or sessions without a
  sandbox), each work dir gets a block of 10 ports. `OVRA_PORT` is its
  first, the block is `OVRA_PORT` to `OVRA_PORT + 9`. Ovra hands blocks out
  from 41000 to 48999, skips a block it cannot bind at that moment, keeps
  the block in the worktree's record and frees it at clean-up.
- `OVRA_PORT` is set everywhere the work dir runs code (setup, agent,
  pods), on every platform, so a project's scripts can use it whatever
  they run on. Using it is up to the project.

## Caches

- Package caches (`~/.cargo/registry`, `~/.npm`, `~/.cache/pip`,
  `~/.cache/pnpm`) are keyed by the **project**, not by the session's
  folder: every worktree of a project shares the project's copy. This is
  what `docs/specs/security.md` already says ("each project gets its own
  copy"); today's per-folder key is listed there as a gap, and this fixes
  it. Workers of one project could poison each other's cache, but they
  are equally sandboxed and their work is reviewed; other projects and
  the person's own caches stay out of reach as now.
- The key becomes a hash of the project's id, which also fixes two
  projects sharing one (`/a/b-c` and `/a/b_c`).
- pnpm's store sits in that cache and the worktrees in Ovra's data folder,
  usually the same volume, so `pnpm install` hard-links instead of copying.
- No shared build folder (`CARGO_TARGET_DIR`) between worktrees: cargo
  locks it, so parallel workers would wait on each other. Cloning
  `target/` through `.worktreeinclude` gives a warm start instead.

## Disk use

- Each worktree reports its size on disk, counted in the background after
  setup and after each turn, at most once a minute, in at most 2 s of
  walking (past that it says "more than N"). Cloned blocks count in full,
  so the number is an upper bound; the UI says "up to".
- The Agents panel shows it on each worker, and the project's total next
  to the capacity line.
- A new worktree is not created while the data folder's volume has less
  than 5 GiB free: the worker stays `queued` with that reason.

## Clean-up

The current rules stay (`docs/specs/orchestration.md`, `Worktrees::remove`):
a worktree goes when its task ended and its items are settled, and removal
refuses ignored files outside ignored folders, which no snapshot holds.
Added:

- `git worktree lock --reason "Ovra: <task>/<name>"` when the worktree is
  made, unlocked just before Ovra removes it, so the person's
  `git worktree prune` or `remove` cannot delete it under a running agent.
- Files Ovra copied in through `.worktreeinclude` and unchanged since
  (same size and hash as when copied) do not block removal: the original
  is in the main checkout. A changed one still blocks and is named.
- Ignored folders (`node_modules`, `target`) go with the worktree; they are
  rebuilt by setup.
- Leftovers: a folder under `<data>/worktrees/<project>` that no task
  knows (a crash, an old version) is listed with its size under "Leftover
  worktrees". The person removes it there; Ovra never does on its own.

Agents stay inside their worktree as today: the sandbox hides the main
checkout and Ovra's data folder, and `GIT_DIR` and `git -C` lead nowhere
they could write.

## Changes by area

| Step | Area                                    | What                                                                                                                                   |
| ---- | --------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| WS.0 | protocol, `services.rs`, `protected.rs` | `AgentStatus::Preparing`, setup progress and disk fields, `skip_setup`; `Worktrees::include` and `usage`; `.worktreeinclude` protected |
| WS.A | `crates/worktree`                       | include copy (clones, caps, no links), lock and unlock, size walk, removal that accepts unchanged copies                               |
| WS.B | `crates/servers`                        | run setup steps once in the work dir's pod, with timeout and log                                                                       |
| WS.C | `crates/core/src/orchestration`         | read `.ovra/worktree.json`, preparing state, port blocks, low-disk queueing, leftovers list                                            |
| WS.D | `crates/sandbox-linux`                  | cache key by project; `OVRA_PORT` and the two root variables on the sandbox's own line                                                 |
| WS.E | `ui/src/agents`                         | preparing row with log, Retry and Start anyway, sizes, leftovers                                                                       |

WS.0 lands first; WS.A to WS.D can then run in parallel; WS.E last.

## Tests

- Fakes only (`MemoryWorktrees`, fake `DevServers`): include rules (ignored
  only, links skipped, caps, folders without cloning), setup order,
  timeout with a fake clock, failure report, port blocks never overlap and
  are freed, removal with unchanged and changed copies.
- `crates/worktree` against real git in a temporary folder: lock, include
  copy, size walk (nextest group `git`).
- One e2e on fake agents: a worker shows `preparing`, then starts.

## Decisions for the owner

1. **`.worktreeinclude` may copy secret-looking files such as `.env` into
   a worker's worktree**, where its agent can read them, with a warning in
   the setup dialog. Recommended: yes, because the person lists them on
   purpose and it is the convention agents expect. The alternative is to
   refuse `.env*` and require project secrets.
2. **Package caches shared by the worktrees of one project** (fixes the
   documented gap). Recommended: yes.
3. **`OVRA_MAIN_ROOT` names the main checkout but is not readable inside
   the sandbox.** Recommended: yes; giving setup read access to the main
   checkout would also give it the main checkout's ignored secrets.
4. **A failed setup keeps the worker from starting** (the coordinator or
   the person chooses Retry or Start anyway). Recommended: yes; the other
   option is to start the agent with the failure in its first message.
