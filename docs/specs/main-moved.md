# When the project moved on under a worker

Status: implemented (2026-11-27). The owner asked for this item ("take
all", next-ideas list item 3); the options here are the recommended ones and
can be changed.

## Problem

A worker's worktree starts from a snapshot of the main checkout (its
`base`). While it works, the project moves on: the person edits and
commits, other workers' changes are kept. Keep then copies a worker's file
only while the project still holds that file as it was at `base`; any
other file is left out as "not copied" (`review.approve`). Today the person
learns this only after pressing Keep, and the only ways out are copying by
hand or replacing their own version.

## What changes

1. **Warning before Keep.** For an open worker item the core compares the
   main checkout as it is now (one look, uncommitted work included) with the
   worker's `base` and reports `ReviewItem::behind`:
   - `files`: how many files of the project changed since `base`, by anyone;
   - `clashes`: which of the worker's files are among them: Keep would leave
     exactly these out.

   `None` when the project did not change, and for direct and decided
   items. The inbox already lists again on `fs.changed` of the project, so
   the warning follows the person's edits. `review.list` takes one look of
   the main checkout per project, not one per item.

2. **Update from the project.** `review.update { id, conflicts }` brings the
   project's changes since `base` into the worker's worktree:
   - Refused while the worker is at work (like Keep), for direct or
     decided items, and for work already pushed (a pull request, the agent's
     own push): what came in would reach the branch unseen. Its dev servers
     stop first, as for Keep.
   - Refused while a file that came in is one the direct session changed
     and nobody decided: unreviewed agent work must not become a worker's
     base, out of sight in its diff.
   - Snapshots the worktree (`mine`) and the main checkout (`main`), then
     `Worktrees::merge(root, base, mine, main)`: `git merge-tree
     --write-tree` with `base` as the merge base, no hooks, committed on
     `main`. The worker's version and the project's version of a file both
     survive where they touch different lines.
   - Without `conflicts` and with conflicting files, nothing is written:
     the answer lists them. With `conflicts: true` the files are written with
     git's conflict markers, for the agent (or the person) to resolve.
   - Refused when a conflicting file cannot hold markers (a binary file, or
     one side deleted it; `Merged::whole`): the person copies the version
     they want.
   - Writes into the worktree only the files that differ between `mine` and
     the merge, through `Worktrees::apply` (from the merged commit, never
     through a symlink). All or nothing: if a file cannot be written (the
     worker changed it since `mine`, not a plain file) or the item changed
     meanwhile (an approve, another update), what was written goes back to
     `mine` and the request is refused.
   - The item's `base` becomes `main`, so its diff again shows only the
     worker's own changes and Keep copies them cleanly. The worktree's
     branch and index are not moved: the worker cannot commit, and a pull
     request picks with `base` as the merge base (`pr-flow.md`).
   - The person's next comment carries Ovra's note naming how many
     files came in and the files with conflict markers, so the agent reads
     them again before it changes them.
   - Answers `review.updated { item, files, conflicts, written }`.

3. **In the inbox.** An open worker item with `behind` shows one line
   above Keep (`ui/src/inbox/BehindNotice.svelte`). Without clashes it is
   quiet: "Your project changed in 3 files since this agent's copy was made. None of
   them are files it changed, so Keep copies all of its changes." With
   clashes it names them, says Keep would leave them out, and offers **Bring
   in project changes**. If the update finds conflicts, the inbox names them
   and offers **Bring in and mark conflicts**; after that, **Ask the agent to
   resolve them** puts a comment in the comment box for the person to send.
   The phone companion cannot update (it changes a worktree).

## Not in this step

- Telling a still-working worker that the project changed in its files
  (worker-to-worker overlaps already have notes, ADR 0012).
- Undo of an update: the worktree's earlier state is a snapshot that
  pruning keeps for only ten minutes.
- Merging the main checkout's commits into the worker's branch.
