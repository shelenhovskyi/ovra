# Agent resources and soft balancing

Status: approved by the coordinator thread on 2026-11-25 within the plan (the
owner asked for it and agreed to the proposal). Technical decisions below are
the implementer's.

## Goal

Show how much memory, CPU and disk each agent session takes, and keep the
computer usable: when memory is low or the CPU stays busy, workers that would
start wait in the queue and the UI says why. Running agents are never stopped
automatically; that would lose their work.

## What is counted

Per session, three process trees, each counted as a whole (the root and every
descendant):

- **agent**: the `claude` process the runtime started (through the sandbox,
  so the root may be the sandbox's own process) and every command it runs.
  The runtime reports the root's pid through `AgentHost::process`.
- **servers**: the pod of the session's work dir (`DevServers::pod_pid`),
  only for isolated sessions. Pods of the main checkout are the person's
  and are not charged to the direct session.
- **browser**: the session's Chromium (`Browser::pid`).

Memory is resident set size summed over the tree (shared pages count more
than once, so it is an upper bound; the hover text says "about"). CPU is the
tree's share since the last sample, 100 = one core. Disk is a worker's
worktree size, `Worker::disk`, counted by `Worktrees::usage`
(`docs/specs/worktree-setup.md`); direct sessions show none (their folder is
the project itself).

## Service

`ResourceMonitor` in `crates/core/src/services.rs`, implemented by the new
crate `crates/resources` (`ovra-resources`, `sysinfo`, MIT) and wired in by
`daemon` and `app`. Its one call blocks; the core runs it on
`spawn_blocking`.

- `sample(roots) -> ResourceSample`: the machine (`SystemLoad`: total and
  available memory, CPU busy share 0-100 across all cores) and one `Usage`
  per root (`None` when that pid is gone). Threads are not counted as
  processes.

The fake in `fakes.rs` returns what a test sets. Without a wired monitor the
core samples nothing and never holds a worker back.

## Sampling

While any session of any project has not ended, the core samples every 2 s
(fake clock in tests), and once more right after an agent process reports
its pid, so a new agent shows at once. Each sample sends `resources.sample`
per project that has such sessions: its sessions' usage, the machine's load
and the current pressure; a project whose last session ended gets one last,
empty sample. Nothing is sampled while no session runs. Only the
2 s samples count for the CPU mean and for starting workers after a
shortage.

## Pressure and the queue

- **Low memory** starts when available memory falls below the larger of
  1 GiB and 10 % of total, and ends once it is back above the larger of
  1.5 GiB and 15 %.
- **Busy CPU** starts when the mean over the last 30 s is at least 90 %, and
  ends once that mean is below 75 %.

While either holds, `resources.sample` names it and queued workers do not
start, even with a free place: a new `spawn_agent` waits `queued`. When the
pressure ends, queued workers start one per sample (2 s apart) until none
waits for anything but a place, so they do not all start at once and bring
the pressure back. A person's own start is never refused
for pressure (it is their decision); the panel shows the pressure next to the
capacity count. The cap of working agents (`slots.rs`) is unchanged and
applies as before.

## UI

- Off by default (owner, 2026-11-25: usage numbers distract): Settings >
  Claude > "Show resource usage" turns the line below on, kept per user in
  the page's storage. Balancing and the reasons below work either way.
- Each running session in the agents list gets one quiet line:
  `1.2 GB · 35% CPU` (plus `· 2.4 GB on disk` for a worker once its
  worktree was counted). Hover shows the split: agent and its commands, dev
  servers, browser, process counts, worktree size, and that memory is
  approximate.
- A queued row says `Waiting: low memory` or `Waiting: CPU busy` under
  pressure, and `Waiting for a free agent` otherwise.
- The capacity line adds `· memory low` or `· CPU busy` while a pressure
  holds.

## Not now

Stopping or pausing running agents, per-agent limits (cgroups, job
objects), GPU, network usage, history charts, telling the coordinator in
its tool replies why a worker waits.
