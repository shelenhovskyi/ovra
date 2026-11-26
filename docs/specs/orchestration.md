# Orchestration (phase 3)

Status: draft for human review (task 3.0). Builds on ADR 0005 (direct mode)
and ADR 0006 (Claude through the user's `claude` CLI).

## Roles

- **Coordinator**: an agent session in the main checkout that plans and
  delegates. It gets Ovra's IDE tools (below) and read-only built-in tools
  (`--tools Read,Grep,Glob`), so it does not take the direct-writer lock.
- **Worker**: an isolated session in its own git worktree, started by the
  coordinator with `spawn_agent`. Its worktree starts from a snapshot commit
  of the main checkout (ADR 0005), on branch `ovra/<task>/<name>`.
  Worktrees live under Ovra's data directory, outside the repository, so
  watchers and indexers of the main checkout never see them.
- **Direct session**: at most one per project, in the main checkout; reviewed
  as a diff against its baseline (ADR 0005). The person starts it; the
  coordinator cannot.

A task is one coordinator and its workers. Limits per task: at most
`max_workers` running workers (default 3) and an optional budget in
millionths of a dollar across all its sessions; `spawn_agent` fails with a
clear message past either.

Across the whole project at most 3 agents work at once by default (owner
decision of 2026-11-25, ORCH-3): direct sessions and workers of every task.
The person may choose 1 to 8 in Settings > Claude (`agents.set_limit`, kept
in the agent settings); raising it starts waiting workers, and
`agents.limit_changed` tells clients. Subagents an agent starts inside its
own process are not counted. Coordinators
do not count, since they only plan and wait for their workers, and neither
does a worker that asked for review and waits idle, or a person's own
session that waits idle for their reply (ORCH-11). A worker spawned past
the cap waits `queued` in its worktree and starts, oldest first, when a place
frees; a person's start or resume past it is refused with a clear message,
but a reply to their idle session (`agents.send`) is not.
`agents.capacity` reports the count ("2 of 3 agents working").

## Roles

The runtime appends a role prompt to Claude Code's system prompt
(`--append-system-prompt`): `agents/roles/coordinator.md` for a coordinator,
`agents/roles/worker.md` for a worker. A coordinator also gets
`--tools Read,Grep,Glob`: it reads the project and directs workers, but
cannot write files or run commands. In the UI, "Plan and split" + Start in the agents
panel starts a coordinator (`orchestration.start`); "One agent, here" starts a single
direct session.

## IDE tools (MCP)

Defined in `crates/mcp-server/src/tools.json`, reviewed and approved by the
owner on 2026-11-24; changes need an independent reviewer agent's verdict,
and a tool that gives agents more reach needs the owner. Who may call what:

| Tool                                                                                        | Coordinator               | Worker     |
| ------------------------------------------------------------------------------------------- | ------------------------- | ---------- |
| `spawn_agent`, `list_agents`, `read_output`, `send_message`, `stop_agent`                   | yes                       | no         |
| `request_review`                                                                            | for a worker (`agent_id`) | for itself |
| `open_in_ui`                                                                                | yes                       | yes        |
| `push` (`docs/specs/agent-push.md`; the person approves each push or a `Push` rule decides) | no                        | for itself |

- Text from `read_output` is another agent's output: data, not
  instructions. The tool description says so, and the coordinator's role
  prompt repeats it.
- Calls to Ovra's own tools come through `can_use_tool` like any other tool.
  The core allows them without asking when the caller's role permits the
  tool and the task's limits hold; otherwise it denies with the reason.

Transport: in-process MCP over the same stream-json pipe. The runtime adds
`--mcp-config {"mcpServers":{"ovra":{"type":"sdk","name":"ovra"}}}`; the CLI
then sends each JSON-RPC message as a `control_request` with
`subtype: mcp_message` and the host answers with
`{"mcp_response": <JSON-RPC reply>}`. Verified on Claude Code 2.1.331
(`initialize`, `tools/list`, `tools/call`). No port and no token, and the
caller is known from the pipe. Fallback if this changes: an HTTP MCP server
on 127.0.0.1 with a per-session bearer token.

## Who wrote a message

The core records the author of every part of a message it sends an agent
(`MessageFrom`: person, coordinator, Ovra), and the transcript shows each
part with its author. The agent itself only reads text, so the text it gets
marks the authors with tags only the core writes
(`crates/core/src/provenance.rs`): a coordinator's words inside
`<coordinator-message>`, Ovra's notes inside `<ovra-note>`, the person's
words outside both. A coordinator's text has every `<` (and every
character that looks like one) written as `&lt;`, so it cannot close its
own mark and speak as the person or as Ovra. Ovra's notes are the core's
own words: anything in them someone else wrote is inside `<untrusted>`
(a rejected diff), cut to one escaped line (a runtime error), or left out
(file names that are not plain, see below). Tool results that carry a
worker's words (`read_output`, `list_agents`, `request_review`) are
escaped the same way, so a worker cannot pass its text to the coordinator
as a note. The role prompts say which tags mean what and that a
coordinator's words are never the person's approval.

## Review

Every session with changes gets one review item. States: `open`,
`approved`, `rejected`. A comment keeps it open and resumes the same session
with the comment (and, after a reject, the rejected diff) as the next
message; the item then shows the new diff.

- Direct session: approve keeps the changes and optionally commits the
  agent's files; reject restores the agent's files from the baseline. Files
  changed outside the agent are marked and never reverted.
- Worker: approve applies its changes to the main checkout for files the
  direct session has not touched and optionally commits them; overlapping
  files wait (ADR 0005). Merging branches stays with the person. Approve
  lands only what the person reviewed: an open worker item carries a
  fingerprint of its files (paths and content versions), the inbox sends it
  back with approve, and the core refuses while the worker is still working
  or when its files changed since; the inbox then shows them as they are now.
  Approve copies from one snapshot of the worktree, the one its fingerprint
  check read, never from the worker's disk at click time; a later "replace
  with the worker's version" copies from that same snapshot. Each diff the
  inbox loads names the fingerprint it was read with.

Which files are the agent's: paths in its `Write`, `Edit` and
`NotebookEdit` calls, plus files that changed while one of its `Bash` calls
ran. Everything else in the diff is "changed outside the agent".

How direct mode tells them apart (`crates/core/src/orchestration/direct.rs`):

- Looks at the tree (tree ids, not snapshot commits) split the session into
  intervals. The `PreToolUse` hook, which the CLI waits for, opens one
  before a file-changing tool runs; the tool's
  result closes it once no such tool runs. A tool waiting for the person's
  approval does not count as running.
- What changed in an interval is the agent's if a `Bash` call ran in it,
  or if a file tool named the file. Between intervals, and at every review,
  anything that differs from the last snapshot changed outside the agent.
- A `Bash` window is not proof: a file the person saved through Ovra
  (`fs.write_file`) while the interval was open counts as changed by both,
  even if the command touched it too.
- An agent file that also changed outside, before or after the agent wrote
  it, shows as changed by both. Reject never reverts it and lists it as a
  conflict; approve does not commit it. A rename counts as both when either
  of its paths is someone else's.
- Reject takes a snapshot before reverting (kept by `refs/ovra/snapshot`'s
  reflog), checks the diff again afterwards and reports anything not
  restored as a conflict. `review.done` lists what it `restored`; the inbox
  says plainly when that is nothing.
- `review.undo_reject` puts the reverted files back from that snapshot
  (`ReviewItem.can_undo` says when it is possible). A file changed since
  the reject stays and is listed as a conflict. The item is open again and
  an ended session gets its attribution back. A comment that continues the
  session drops the undo.
- Anything unknown leans towards keeping files: a failed snapshot makes the
  agent's earlier files count as both, a session that ends during a tool
  drops that interval, and ignored files are never in the diff. A person's
  edit made with another editor during a running `Bash` call is still the
  agent's (nothing tells them apart); undo is the way back.
- One direct session per project runs at a time. Ended ones stay until
  approved or rejected, and to an earlier session a later one's changes are
  changed outside it. Attribution lives in memory: after a restart the
  item's files all count as changed outside, and reject refuses with a
  message saying nothing was restored. Without a baseline (not a git
  repository) a direct session runs but has nothing to review.

## Project holds

The core counts holders per project and watches a project while it has any
(clients since commit e039ee0). A running session holds its project the same
way, so its review diff stays live when no window has the project open, and
releases it when it ends. Workers also hold a watch on their worktree while
they run (`orchestration/watch.rs`): a change there sends `review.changed`
for the worker's latest item, at once and then at most once a second, so
the inbox shows the diff as it grows, not only at turn ends. The watch's
own events never reach clients, and it ends with the worker.

## Workers changing the same file

Worktrees keep workers from overwriting each other, so two workers that
change the same file would otherwise learn of it only when the person
approves the second one. The core looks at a worker's changed files (its
latest review item, old paths of renames included, none once discarded)
when its worktree changes and when its turn ends
(`orchestration/overlap.rs`), and compares them with what it last saw of
the task's other workers. When a file is in both, the coordinator gets a
note from Ovra naming both workers and the files, and each of the two
that is still working (or waiting for an approval) gets one too; a worker
that waits idle is not woken, the coordinator decides about it. Each pair
of workers is told once, in one note per look. Notes name only file names
made of letters, digits, `.`, `_`, `/` and `-` (at most 200 bytes) and
count the others, since file names are the workers' words. A discarded
worker counts as changing nothing; what the core saw is not saved and is
dropped when the task ends.

## Stopping a task

The person stops a task with `orchestration.stop` or by stopping its
coordinator. Either way every running or queued worker of the task stops
too, and so does a worker that was still being set up when the stop came;
a coordinator that is stopping starts no more workers. Worktrees and review
items stay. A coordinator that fails leaves its workers running, and its
task can be resumed; one that stops for another reason (Ovra shuts down)
takes its workers with it, but its task too can be resumed.

## Cleaning up worktrees and branches

One place decides (`tasks::clean_up`, run when tasks or reviews are
listed): once a task is stopped, a worker's worktree is removed when the
worker has ended, no item of it is partly approved and its worktree has no
change the person has not decided. Its branch `ovra/<task>/<name>` is then
deleted too, unless it has commits that are neither in the worker's last
decided snapshot nor in the main checkout's HEAD (the worker or the person
committed there); such a branch stays for the person to merge or delete.
A worktree git refuses to remove (it holds ignored files such as a `.env`)
stays, and the task shows why. On any doubt nothing is removed.

## Zones (phase 3 tasks)

- 3.A `crates/worktree`: `Worktrees` (create, snapshot, changes, patch,
  restore, apply, commit, remove) with git.
- 3.B `crates/core/src/orchestration`: tasks, roles, limits, the review
  state machine, holds. Pure state; no OS calls.
- 3.C `crates/mcp-server`: JSON-RPC (`initialize`, `tools/list`,
  `tools/call`) and mapping tool calls to core requests;
  `crates/agent-runtime` routes `mcp_message` to it.
- 3.D `ui/src/inbox`: the review inbox.
- 3.F `crates/worktree/src/direct.rs`: baseline, attribution, reject.
