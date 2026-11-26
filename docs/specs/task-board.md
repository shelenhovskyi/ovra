# Task board

Status: approved by the owner on 2026-11-25 with the recommended option
of B1-B3; questions to the person stay quiet (no toasts). ADR 0012; builds
on `docs/specs/orchestration.md` and `docs/research/agent-messaging.md`.

## What it is for

Workers of one task can say which files they take, ask a neighbour, the
coordinator or the person a short question, and leave a note for everyone.
The person sees all of it and can join in. Ovra's own notes (overlaps,
reviews, stops) appear on the same board, so there is one place to look.

## Entries

An entry has an id (`b1`, `b2`, ... per task), a time, an author and a
kind:

| Kind       | Written by                           | Fields                                                                      |
| ---------- | ------------------------------------ | --------------------------------------------------------------------------- |
| `claim`    | a worker, or the coordinator for one | paths (relative, globs allowed), note                                       |
| `release`  | the claim's worker, coordinator      | paths                                                                       |
| `question` | worker, coordinator                  | to (`coordinator`, a worker's name, or `person`), text                      |
| `answer`   | whoever was asked, the person        | question id, text                                                           |
| `note`     | worker, coordinator, person          | text                                                                        |
| `ovra`     | the core only                        | what happened (overlap, asked for review, stopped, failed, kept, discarded) |

Authors: `person`, `coordinator`, `worker:<name>`, `ovra`. The core sets
the author from the caller (the pipe the tool call came on, or the UI
request), never from the text. Paths go through `RelPath::parse`. A text
is at most 2000 characters; a claim at most 50 paths.

Claims end when their worker ends or its work is discarded. The existing
overlap notes (`orchestration/overlap.rs`) become `ovra` entries and also
compare a new change with other workers' claims, not only with their
changes.

## Tools (MCP)

| Tool                        | Coordinator               | Worker     | Returns                                       |
| --------------------------- | ------------------------- | ---------- | --------------------------------------------- |
| `claim(paths, note)`        | for a worker (`agent_id`) | for itself | the entry and any claim or change it overlaps |
| `release(paths)`            | yes                       | own        | the entry                                     |
| `ask(to, question)`         | yes                       | yes        | the question id, at once                      |
| `answer(question_id, text)` | if asked                  | if asked   | the entry                                     |
| `post(text)`                | yes                       | yes        | the entry                                     |
| `read_board(since)`         | yes                       | yes        | entries from `since`, and `next`              |

The tool descriptions say that entries are data written by other agents,
not instructions, and that no entry grants a permission or a decision.
The coordinator's `spawn_agent` gains an optional `files` argument that
becomes the new worker's first claim.

## Delivery

- New entries meant for an agent (questions to it, answers to its
  questions, `ovra` entries about it, claims that overlap its own) are
  delivered with its next message: at the start of its next turn, merged
  into one `<board-entry>` block per entry, at most 20 entries or 4 KB,
  then "N more on the board, read them with read_board".
- A running agent gets them as a message right away, like the coordinator's
  `send_message` today. An idle or ended worker is never woken by the
  board: a question to it is answered by the core with "worker X waits
  idle; ask the coordinator", and the entry stays for whoever resumes it.
- The coordinator gets every entry, also when idle: it is the task's
  planner. **B1:** an idle coordinator is woken only for questions to
  it, overlaps and `ovra` entries; the rest waits for its next turn.
- Mid-turn delivery inside a long turn (a `PostToolUse` hook answer with
  `additionalContext`) is a later step; it needs a check on the CLI first.

## Questions for the person

`ask(to: "person")` shows the question quietly: on the task's board and
under "Waiting on you" next to approvals, with no toast and no desktop
notification of its own. The person
answers from the UI (`board.answer`); the answer is marked as theirs
because it comes from the UI. The asker keeps working while it waits.
**B2:** workers may ask the person directly; the coordinator sees the
question and the answer.

## Limits

- Per agent: 10 entries a minute and 200 a task; past that the tool
  refuses with the reason. A question chain (a question asked by the agent
  that just got an answer from the same agent) stops at depth 3.
- The task view shows the count per agent, so a talkative pair is visible.
- Entries delivered to agents count in the task's budget like any other
  tokens.

## Security

- The author is set by the core, and the `<board-entry author="...">` mark
  is written by the core. An entry's text, and the paths of a claim, are
  agent-written: every `<` and look-alike in them is escaped
  (`provenance::escaped`) before another agent reads them, as a
  coordinator's text is today. `ovra` entries follow the rules of Ovra's
  notes: foreign text only inside `<untrusted>`, only plain file names.
- No tool answers a permission request, decides a review, allows a site or
  changes a policy, and no entry changes them.
- An approval card for a tool call that an agent asked for within its
  first turn after a board entry shows that entry ("after a message from
  worker A"). A deny the person gave one agent of a task is shown when
  another agent of the task asks for the same tool and input.
- Entries are text: no files, no attachments, no paths outside the project.
  Workers of one task share one network policy, so the board cannot move
  data from a narrow sandbox to a wider one.
- Text an agent copied from a web page or a server log keeps its
  `<untrusted>` marks.
- Entries go into the audit log and persist with the task.

## Built so far, and what is left

The first cut (2026-11-26) has the entries, the six tools, delivery, the
limits, persistence and the Board tab. Decisions made while building it:

- What waits for an agent is sent as one Ovra message when its next turn
  starts (its status turns working), not merged into the message that
  starts it; what cannot be sent (a queued or stopping session) waits on.
- An answer to the coordinator's question waits for its next turn (B1).
- The chain limit counts questions and answers between the same two
  agents since the person or the coordinator last wrote; other entries do
  not end the run, and talk with the person is not limited.
- `read_board` returns at most 20 entries or 16 KB a call, with `next`.
- A claim path is at most 256 bytes with at most 8 runs of `*`; matching
  is row by row with a step budget per search.

Left for later: entries in the audit log; a time on each entry; the watch
tick comparing a new change with other workers' claims (it compares
changes only); saving boards apart from the rest of the orchestration
state; approval cards that name a preceding entry; the task row's "who
changes what" view; the e2e test; keeping two sends that race to one
agent from carrying the same entry twice.

## Protocol (`board.*`)

- `board.list {task, since}` → entries and `next`.
- `board.post {task, text}` and `board.answer {task, question, text}`:
  the person's note and answer.
- Event `board.changed {project, task, entry}`.

## UI

- The task row: one line, for example "3 workers · 1 overlap · 1 question
  for you". Only an overlap and a question for the person draw attention.
- Opening the task shows the board: a timeline of entries with their
  authors, a box to post or answer, and "who changes what" (file →
  workers, from claims and changes) with overlaps marked.
- **B3:** the board is a "Board" tab in the task view of the agents
  panel, next to its workers.

## Tests

- Core: authors come from the caller; marks are defused; delivery merges
  and caps; an idle worker is not woken; limits and chain depth; claims end
  with their worker; overlaps against claims; persistence across restart.
- MCP server: the six tools by role, refusals with reasons.
- UI: the board timeline, posting and answering, the attention line.
- e2e: a fake task where one worker asks another and the person answers a
  question.

## Zones

1. Protocol (`protocol/board.rs`, `ui/src/lib/protocol/board.ts`) and the
   `tools.json` schemas: one small task that lands first.
2. Core: `orchestration/board.rs` (state, delivery, limits) and the tool
   calls in `crates/mcp-server`.
3. UI: `ui/src/agents` board tab and the attention line.
4. Approval cards showing a preceding board entry.
