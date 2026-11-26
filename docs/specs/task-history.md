# Agent history

Status: proposed 2026-11-26. The owner picked "history and search of past
tasks, with a way to continue any of them" from the list of improvements.

People ask "what did the agent do yesterday in this project?" and want to
pick an old task up again. Today the Agents panel lists only the open
project's sessions, a closed one is gone from sight, and nothing can be
searched.

## Behaviour

- **History** is a view in the main area, like Reviews and the Audit log.
  It opens from the Agents panel's header (a clock button, "History"), the
  More menu and the palette ("Agent history").
- It lists the agent sessions of **every project**, newest activity first,
  grouped by day: Today, Yesterday, then the date. A row shows the task's
  title, the project's name, its status in words and the time.
- One **search field** at the top filters as the person types. Every word
  must appear in the task's title, in what the person or the agent wrote,
  or in the project's name; case does not matter. A row found in the
  conversation shows the matching line under the title, the words marked.
- A **project** menu next to the field narrows the list to one project; it
  starts at "All projects".
- Pressing a row opens that session in the Agents panel, switching project
  first when it belongs to another one. A session the person had closed
  comes back to the panel's list. There the person reads it and, when the
  agent can continue it, types the next message as for any ended session
  (`agents.resume`). Sessions that cannot be continued say so in the panel
  as they do today.
- Nothing in History starts an agent: an agent starts only from the
  person's message in the panel.

## Core

- `SessionInfo` gains `started_ms` and `active_ms` (ms since the Unix
  epoch; `null` for sessions saved before them). `active_ms` moves with
  every message and status change.
- `agents.history { query, project? }` answers `agents.history`: at most
  200 entries `{ session, snippet }`, newest `active_ms` first, across all
  projects or one. `snippet` is a line of the conversation around the first
  match, at most 160 characters, with the match's UTF-16 range; `null` when
  the title or project name matched, or the query is empty. The search
  runs over what the core already keeps in memory; no index.
- Ended sessions kept per project go up from 20 to 50: history is only
  useful if it reaches back a few days. The oldest ended ones still go
  first.
- The phone companion cannot call it (`companion.rs`): history stays on the
  computer.

## Out of scope

- Full-text search inside tool output and diffs.
- Deleting history entries from the view (a later step; deleting is the
  owner's call).
- Keeping sessions beyond the per-project limit, or a separate archive.
