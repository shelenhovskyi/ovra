# Checkpoints: go back to an earlier turn

Status: draft for the owner (2026-11-25), task 7 of `docs/roadmap-next.md`.
Builds on ADR 0005 (direct mode) and `docs/specs/orchestration.md`
("How direct mode tells them apart"). Research: improvement report 1.4 in
the project files.

## Problem

A direct session works in the main checkout. When a turn goes wrong, the
person can reject the whole session or fix files by hand, but cannot say
"undo the last two turns and keep the rest". Claude Code's own checkpoints
see only `Write`, `Edit` and `NotebookEdit`; changes made by `Bash`
(`sed -i`, a code generator, `git mv`) are missed, and other tools'
checkpoints are known to sometimes restore nothing.

## Goals

- A snapshot of the work dir before and after every turn of a direct
  session, `Bash` changes included.
- "Go back to before this message": the agent's changes from that turn on
  are undone, the person's own changes are kept, and the go-back itself
  can be undone.
- Tested end to end on a fake agent whose `Bash` call changes a file.

Not goals for v1: workers (their review already isolates them; the same
code can serve them later), projects that are not git repositories
(they have no baseline either), ignored files, rewinding the agent's
conversation (below), moving a session to a worktree ("hand off").

## Turns and snapshots

- A turn is one message to the session (the person's, a review comment,
  a coordinator's) up to its `result`.
- **Before** turn N: a snapshot taken when message N is sent, after any
  edits the person made while the agent was idle. **After** turn N: a
  snapshot at its `result`, or when the turn ends some other way (stop,
  crash).
- Snapshots are the ones review already uses (`Worktrees::snapshot`:
  tracked and untracked files, not ignored ones, kept by
  `refs/ovra/snapshot`'s reflog, which the direct agent cannot write on
  Linux because its `.git` is read-only). Because they read the whole
  working tree, a file `Bash` changed is in them like any other.
- Cost: one snapshot per turn edge, reusing the private index, so a turn
  adds two `git` runs over files that mostly did not change. The speed gate
  (`crates/worktree/examples/bench.rs`) gets a case for it.
- **Kept**: every checkpoint of a session while its review item is open, at
  most the last 100 turns (older ones drop, and the transcript says how far
  back one can go). When the item is approved or rejected, they are let go
  through `prune_snapshots` like the baseline.

## Going back

On each message in the transcript of an idle direct session: **Go back to
here**. It sets the work dir back to the **before** snapshot of that turn
for the agent's changes, and puts the message's text back in the composer
to edit and send again (or to clear).

- Only while no turn runs. During a turn the action says to stop the agent
  first.
- What is restored: every path that differs between that snapshot and now
  and whose changes since then are all the agent's, using the attribution
  direct mode already keeps (intervals around file tools and `Bash`
  calls). Files the agent created are deleted; files it deleted come back.
- What is kept: a path that changed outside the agent after the snapshot
  (the person, a formatter, git), or changed by both, is left as it is and
  listed as a conflict, as reject does. Nothing the person wrote is ever
  overwritten.
- First a snapshot of the current state is taken. **Undo go back** puts
  the restored files back from it, until the next turn starts. A file
  changed since the go-back stays and is listed.
- Ignored files are not restored: what `pnpm install` put in
  `node_modules` stays. The confirmation says so in one line.
- The review item needs nothing special: its diff is baseline to the
  working tree, so it shrinks by itself. Restored files count as the
  agent's, as they were.
- After a restart the attribution is gone (it is in memory, as for
  reject): files then count as changed outside, so going back restores
  nothing and says so. Keeping attribution in `SessionStore` would fix
  this for reject too; it is a separate follow-up.

## The conversation

The `claude` CLI has no documented way to continue a session from an
earlier message (`--resume` with `--fork-session` copies all of it), and
editing its transcript files would mean changing Claude Code's own state,
which ADR 0006 rules out. So v1 sets back **files only**:

- The session continues. The next message the agent gets starts with a
  note from Ovra: "The files were set back to how they were before your
  turn N; changes from turns N to M are undone. Files the person changed
  are kept: <list>." The person's edited message follows.
- The transcript in Ovra keeps the later turns, marked as undone (greyed,
  with a divider "Files set back to before this message").
- If the CLI gets a documented way to resume at a message, the same action
  can also rewind the conversation; that is a change behind
  `AgentRuntime`, not to this spec's file behaviour.

## Protocol

In `protocol/agents.rs` (and `ui/src/lib/protocol/agents.ts`):

- `agents.checkpoints { session }` returns the turns that can be gone back
  to: `{ turn, at, message (first line), files_changed }`, newest first.
- `agents.go_back { session, turn }` returns
  `{ restored: [path], conflicts: [path], can_undo }`; refused
  (`conflict`) while a turn runs, `not_found` for a dropped turn.
- `agents.undo_go_back { session }` returns `{ restored, conflicts }`.
- `SessionInfo` gains `undone_from: Option<u32>` so the transcript can
  mark undone turns, and `can_undo_go_back`.

No new `Worktrees` method: `snapshot`, `diff` and `restore` cover it.

## Changes by area

| Step | Area                                   | What                                                                  |
| ---- | -------------------------------------- | --------------------------------------------------------------------- |
| CP.0 | protocol (Rust and TS)                 | the three methods and two `SessionInfo` fields                        |
| CP.A | `crates/core` (agents, `direct.rs`)    | snapshots at turn edges, retention, go back, undo, the note to the agent |
| CP.B | `ui/src/agents`                        | "Go back to here" on messages, confirmation, undone turns, Undo       |
| CP.C | `tests/e2e`                            | one flow on the fake agent                                            |

## Tests

- Core on fakes (`MemoryWorktrees`, fake agent): a turn whose `Bash`
  writes `a.txt` and deletes `b.txt`, go back, both as before; a file the
  person changed after the turn is kept and listed; undo puts the agent's
  files back; go back while a turn runs is refused; the 101st turn drops
  the first; after a restart nothing is restored and the reason says so.
- `crates/worktree` against real git: snapshot, restore of a deleted and a
  created file, a rename.
- e2e: two turns, go back to the second message, its file is gone, the
  composer holds the message, undo brings the file back.

## Decisions for the owner

1. **Files only in v1; the conversation continues with a note.**
   Recommended: yes. The alternative, starting a fresh `claude` session
   with a summary of the turns before N, loses the agent's context.
2. **Direct sessions only in v1.** Recommended: yes; workers' changes are
   already isolated and reviewed. Workers can get it later from the same
   code.
3. **Ignored files are not part of checkpoints.** Recommended: yes; they
   are large and rebuilt by tools. The confirmation says so.
4. **Up to 100 checkpoints per session, dropped when its review is
   decided.** Recommended: yes.
