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

## IDE tools (MCP)

Defined in `crates/mcp-server/src/tools.json` (needs human review before
3.C). Who may call what:

| Tool                                                                      | Coordinator               | Worker     |
| ------------------------------------------------------------------------- | ------------------------- | ---------- |
| `spawn_agent`, `list_agents`, `read_output`, `send_message`, `stop_agent` | yes                       | no         |
| `request_review`                                                          | for a worker (`agent_id`) | for itself |
| `open_in_ui`                                                              | yes                       | yes        |

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
  files wait (ADR 0005). Merging branches stays with the person.

Which files are the agent's: paths in its `Write`, `Edit` and
`NotebookEdit` calls, plus files that changed while one of its `Bash` calls
ran. Everything else in the diff is "changed outside the agent".

## Project holds

The core counts holders per project and watches a project while it has any
(clients since commit e039ee0). A running session holds its project the same
way, so its review diff stays live when no window has the project open, and
releases it when it ends. Workers also hold a watch on their worktree while
they run.

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
