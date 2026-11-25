# 0011. A task board for agents of one task

- Status: proposed
- Date: 2026-11-25

## Context

A task's workers only hear from their coordinator, and the coordinator only
reads their transcripts (`docs/specs/orchestration.md`). A worker cannot
ask a neighbour "are you adding this helper too?", say which files it is
taking, or ask the person a question without stopping. The owner wants
agents of one task to talk to each other, done well
(`docs/research/agent-messaging.md`). On 2026-11-25 the owner chose a
shared board where workers ask each other directly and the person and the
coordinator see everything, over relaying every question through the
coordinator.

## Decision

- Each task has one board kept by the core: entries that agents, Ovra and
  the person write, in order, persisted with the task. Every agent of the
  task and the person read the same board; there are no private channels
  and no board across tasks.
- Agents write to it only through new IDE tools (`claim`, `release`,
  `ask`, `answer`, `post`, `read_board`; `docs/specs/task-board.md`).
  Workers get them, in addition to `request_review` and `open_in_ui`.
- The person writes and answers only through the UI, with a new protocol
  domain `board.*` (`protocol/board.rs`, `ui/src/lib/protocol/board.ts`).
- Board entries reach an agent as a note in its next message, inside a
  `<board-entry>` mark with the author's name that only the core writes.
  An entry's text and paths are agent-written, so every `<` in them is
  escaped (`provenance::escaped`), as a coordinator's text is today; Ovra's
  own entries follow the rules of Ovra's notes (no foreign text outside
  `<untrusted>`, only plain file names).
- The board grants nothing. Permission requests, review decisions, network
  allows and "Always allow" stay answerable only from the person's UI. An
  entry is text only, size-capped, rate-limited and counted in the task's
  budget. A question never wakes an idle or ended worker.

## Consequences

- `crates/mcp-server/src/tools.json` grows by six tools; the owner approved
  the direction, and the exact schemas get a human review with the spec.
- Agents have one more source of text that is data, not instructions; role
  prompts say so, and approval cards show a board entry that came just
  before a tool call.
- The coordinator is no longer the only path between workers, so it sees
  the board instead of relaying.
- Claude Code's own agent teams stay unused: they do not run with
  `claude -p` (ADR 0006).
