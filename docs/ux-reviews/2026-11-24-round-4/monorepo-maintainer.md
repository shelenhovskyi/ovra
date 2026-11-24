# Marta, the monorepo maintainer: Round 4 (2026-11-24)

Build: 81d16b1 (debug, `fake-agent` and `fake-claude`), headless, Chromium 158.0.8127.36, 1280x800.
Scope: terminals under heavy output, the Agents panel with a long session,
the review inbox with a large agent diff, snapshot cost in direct mode, and
how direct mode tells the agent's changes from hers. Re-check of MONO-19,
MONO-21 and MONO-24. Fixtures: `monorepo` (15,200 files), `small`; in some runs
40,000 extra untracked files under `huge/` and 20,000 ignored files under
`packages/pkg-001/dist/`. Agents: the scripted fake agent, and the real
Claude driver against `fake-claude` playing my own fixtures (a "codemod"
session with two parallel `Bash` calls, one with 400 extra `Read` calls and
replies). While the first `Bash` ran, the script wrote the agent's change set:
600 files modified in `pkg-000`..`pkg-011`, three 20,000-line files added in
`pkg-100`, and the 50 files of `pkg-150` deleted (653 changes).

## Summary

Marta can now hand a 653-file refactor to an agent in her real checkout,
open the inbox in about 120 ms, page through a 20,000-line diff without a
freeze, and approve and commit exactly the agent's 652 files in 0.9 s, with
her own edit to one of those files correctly kept out. The costs that remain
grow with her repo. Rejecting that change set took 50-54 s, with no progress
shown. The inbox does not update when she edits files after the agent stops,
so she approves a list and diff that are out of date. With 40,000 untracked
files every snapshot takes 2-3 s and her `.git` grows from 4.2 MB to 165 MB.
A fast terminal command loses about two thirds of its output, and nothing
says so. The terminal itself is fast, and the earlier tree, palette and
editor problems I re-checked are fixed.

## Re-check of round 3

| ID      | Old severity | Status     | Evidence                                                                                                                                                                                                                                                                                                                                 |
| ------- | ------------ | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| MONO-19 | major        | **fixed**  | Restore with 402 expanded folders and 20 tabs, active `packages/pkg-171/src/module-3.ts`: right after the restore, the row is selected and scrolled to (scrollTop 115,978). Reveal: selected, scrollTop 130,388. Alt+P `pkg100mod8` Enter: `pkg-100/src/module-8.ts` selected. Alt+[: back to `pkg-171/src/module-3.ts`, selected (r12). |
| MONO-21 | minor        | **fixed**  | Per keystroke including two frames: 28-58 ms with 15,200 files (`module-7`, `pkg171mod3`) and 32-49 ms with 55,200 files (`f-39999`). The roughly 33 ms floor is the two frames. The list ends with "Showing 50 of 1,000. Type more to narrow down." (r12, r13)                                                                          |
| MONO-24 | major        | **fixed**  | Alt+W on a dirty `module-3.ts` asks "Save changes to packages/pkg-171/src/module-3.ts before closing?". Ctrl+S while it is shown writes the file (`fs.write_file`, content on disk) and closes the tab, and the question does not follow her to the next tab (r12).                                                                      |
| MONO-23 | minor        | still open | Seen in passing: tab labels are still `module-3.ts - packages/pkg-009/src`, so about three tabs fit (r12 03-mono24.png).                                                                                                                                                                                                                 |

## Measurements

| What                                                                          | Result                                                                                                             |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `agents.start` (direct baseline) to the first approval, monorepo as built     | 199-801 ms                                                                                                         |
| Same with 40,000 untracked files                                              | `agents.start` 2.16 s; first approval 5.2 s; Allow to the next approval 6.8 s; last Allow to idle 2.2 s            |
| `review.list` with 653 files                                                  | 170 ms; with 40,000 untracked files 1.3-1.4 s                                                                      |
| Open the inbox (Alt+Shift+R) to listed files and first diff                   | 116 / 121 ms; DOM 3,470 elements; one long frame of 79 ms                                                          |
| 20,000-line added file in the inbox                                           | Shown in 1.3 s (with 40k untracked files), DOM 3,233; longest frame 64 ms                                          |
| ArrowDown through 30 files in the inbox                                       | 92-247 ms per file; with 40k untracked files each `review.diff` takes about 630 ms                                 |
| Approve and commit, 653 files                                                 | 0.9 s, no long frames; commit holds 652 files                                                                      |
| Reject, 653 files (613 the agent's)                                           | **50.6 s and 54.2 s**                                                                                              |
| Terminal (clean page): `find .` 16k lines / `git log --stat` / `seq 1 300000` | 0.8 s / 1.0 s / 1.4 s, longest frame 96-105 ms                                                                     |
| Terminal: Ctrl+C during `seq 1 50000000` to a new prompt                      | 1.4 s, no long frames; hidden panel during a flood: main thread answers in 12-63 ms                                |
| `seq 1 3000000`: lines that reach the page                                    | 1.0-1.5 million (34-51 %); 211-1,069 `info.resync` events                                                          |
| Long session (400 reads + 400 replies) streamed in                            | 2.2 s, DOM 81 to 3,743, 18 long frames of about 105 ms; typing in the reply box afterwards 79 ms for 33 characters |

A note on method: with the harness's logging WebSocket proxy, terminal
floods looked much worse (Ctrl+C to prompt 10 s, 93 long frames, the tree
unclickable for 30 s; r04). That came from the proxy, not from Ovra. All
terminal numbers above come from a second page without the proxy (r05-r09).

## Findings

### MONO-25 major: rejecting a large agent change takes almost a minute, and the inbox shows no progress

- **Area:** crates/core/src/orchestration/direct.rs (reject), crates/worktree; ui/src/inbox
- **Steps:** 1. Open `monorepo` and start a session (real driver, codemod fixture). 2. While its `Bash` runs, 600 files change, 3 are added and 50 are deleted. 3. After the agent is idle, edit 40 of those files yourself. 4. Reload the page and open the inbox ("654 files, 41 changed outside the agent"). 5. Reject…, then Reject the changes.
- **Expected:** a few seconds at most, like approve (0.9 s), and a visible "Rejecting 613 files…" while it runs.
- **Actual:** `review.reject` answered after **50.6 s** (r03, `-> review.reject` 16:36:43.130, `<- review.done` 16:37:33.750) and **54.2 s** (r14). Meanwhile the item still says "Waiting for review". Approve and "Reject the changes" are only dimmed. There is no message, spinner or count (r14 01-rejecting.png). The header still says "Reviews (1 waiting)". She can close the inbox and open, or start editing, one of the files being restored (`pkg-005/src/module-7.ts`, r14 02-open-during-reject.png). The result is correct: her 40 edits and `README.md` are kept, the 3 added files are removed, the 50 deleted files are back. From the code, before restoring, reject collects the rejected diff with one `worktrees.patch` (one `git diff`) for each of the agent's files, in a loop (`direct.rs`, around line 405).
- **Why it matters for Marta:** reject is the "undo the agent" button. A silent minute on a 600-file change looks like a hang. She is likely to edit or rebuild in the meantime, which races with the restore.
- **Suggested fix:** collect the rejected diff with a single `git diff <baseline> -- <paths…>` (or one `diff-tree` between two snapshot trees) instead of one call per file. Do the same wherever per-file patches are joined. In the inbox, show "Rejecting N files…" with the item state, and say in the editor or tree that files are being restored. Add a worktree test with about 500 files and a time budget.

### MONO-26 major: the inbox does not update when she changes files after the agent stops, so she approves a stale list and diff

- **Area:** crates/core/src/orchestration (when `review.changed` is sent), ui/src/inbox (`reviews.svelte.ts`, opening the dialog)
- **Steps:** 1. Run the codemod session to idle (653 agent files, `review.changed` and `review.list` at 16:35:00). 2. In her editor or shell, change `README.md`, `packages/pkg-199/src/module-0.ts` and the agent's `packages/pkg-000/src/module-0.ts`. 3. 1.5 s later, open the inbox. 4. Tick "Commit the agent's files" and press Approve and commit.
- **Expected:** her three files show up, with "Changed outside the agent" on two and "Also changed outside the agent" on the agent's file. The diff of `pkg-000/src/module-0.ts` shows her `// marta` line. The spec says a running session's review diff stays live.
- **Actual:** no `review.changed` is sent and opening the dialog does not re-fetch (r02 log: no `review.list` between 16:35:00.9 and the approve). The item shows "653 files" with no outside count. `README.md` and `pkg-199` are missing. The diff of `pkg-000/src/module-0.ts` is the agent's version without her line (r02 03-commit.png). After she approves, the result says "Approved. Left for you: packages/pkg-000/src/module-0.ts", a file that was never marked. The core's own decision was right: her file was not committed. Only reloading the page brings the right list ("654 files, 41 changed outside the agent", r03).
- **Why it matters for Marta:** she reviews big agent changes while still working in the same checkout. If the inbox does not show her own edits next to the agent's, the "changed outside" protection is invisible until after she has decided.
- **Suggested fix:** while a direct item is open, have the core send `review.changed` when the project watcher sees a change, debounced. The session already holds the project. Also re-fetch the list when the inbox opens, and re-fetch on `info.resync`. Test: change a file outside after `idle`, and expect a `review.changed` and the outside marker.

### MONO-27 major: fast terminal output is dropped without notice, and the same event loss can reach the agent and inbox panels

- **Area:** crates/core/src/core (`EVENT_CAPACITY` 1024, one broadcast channel for every event), crates/transport (ws.rs), ui/src/terminal, ui/src/agents, ui/src/inbox
- **Steps:** 1. Open `monorepo`, open a terminal (Ctrl+Backquote). 2. Run `seq 1 3000000; echo T1DONE`. 3. Count the numbers in the `terminals.output` frames that reach the page (r09; variant with a second terminal running `find .` 3 times, r08).
- **Expected:** every line reaches the terminal (with the pty slowed down if the page cannot keep up), or at least the terminal says that output was skipped.
- **Actual:** only 1,008,442 / 1,185,407 / 1,531,892 of 3,000,000 lines arrive, with 70-814 gaps. The daemon logs `client lagged behind core events` 211-1,069 times (skipped 14,080-18,618 events). The terminal shows the last 5,000 lines without gaps and gives no hint that anything is missing (r08 end.png). The same channel carries `agents.message`, `permissions.requested` and `review.changed`. The tree, editor and viewers reload on `info.resync`, but the Agents panel resyncs only after a reconnect, and the inbox and terminals not at all. In one flood run the Agents transcript showed the first turn twice ("You / Fix the typo / Claude / … / Run ls -la" repeated; r09 second pass). Two terminals flooding while an agent started still showed both approval cards (r06), so the loss is by chance, not systematic.
- **Why it matters for Marta:** builds and `git log` over a monorepo produce exactly this kind of output. A build log with lines silently missing in the middle, or an agent transcript that is out of order, is worse than a slow one. She cannot tell what she is not seeing.
- **Suggested fix:** keep terminal output out of the shared broadcast. Give each terminal a bounded queue with backpressure: stop reading the pty while the client is behind, the way a real terminal emulator does. Or coalesce chunks per terminal before they reach the channel. If anything is dropped, write a visible "[output skipped]" marker into the terminal. Make the Agents panel reload its transcripts on `info.resync` (replace, not append), and have the inbox re-fetch. Test with the fake pty and a slow client: no gaps, or one marker.

### MONO-28 major: with many untracked files every snapshot costs seconds and fills her `.git` with loose objects

- **Area:** crates/worktree (`git.rs` `working_tree`, snapshots), crates/core/src/orchestration/direct.rs; ui/src/agents (status)
- **Steps:** 1. Add 40,000 untracked, not ignored, files under `huge/` (40 folders x 1,000; generated output not yet in `.gitignore` is common in her repo). 2. Open `monorepo` and start the codemod session. 3. Allow both `Bash` calls, then open the inbox and click through files. 4. Look at `.git` afterwards.
- **Expected:** a snapshot of the working tree costs about what `git status` costs, and Ovra's bookkeeping does not change her repository's size noticeably.
- **Actual:** `agents.start` 2.16 s, first approval 5.2 s (0.2-0.8 s without the untracked files), Allow to the next approval 6.8 s, last Allow to idle 2.2 s, `review.list` 1.4 s, each `review.diff` about 630 ms (r11, r13). Throughout, the session only says "Working", so the wait looks like the agent thinking. Afterwards `.git` holds **40,579 loose objects, 165 MB** (4.2 MB in an untouched fixture), reachable from `refs/ovra/snapshot` and its reflog (4 entries). From the code, each snapshot copies the real index into a temporary one and runs `git add --all`, so every untracked file is hashed again each time and written as a blob.
- **Why it matters for Marta:** in direct mode the agent waits for the `PreToolUse` snapshot before every file-changing tool, so on her repo every edit or command costs seconds more. The repo growth shows up as `git gc` warnings and disk use she never asked for. Git also keeps a copy of untracked content she may have meant to delete, which matters for files like `.env` that she forgot to ignore.
- **Suggested fix:** keep one private index per project between snapshots, so git's stat cache also covers untracked files. Say "Snapshotting the working tree…" in the session status while it runs. When the baseline finds a large number of untracked files (for example over 5,000), say so, with the slowest folders, and offer to ignore them. After a review is approved or rejected, expire the snapshot reflog and prune, or write snapshot objects into a separate object directory (alternates) that Ovra owns. Add a worktree perf test with 10,000 untracked files.

### MONO-29 minor: the inbox file list does not scale to hundreds of files

- **Area:** ui/src/inbox (`Inbox.svelte`, `describe.ts`)
- **Steps:** 1. Produce a 653-file review (r01, r13). 2. Try to find the deleted files, the three big generated files, and everything in `pkg-011`.
- **Expected:** files grouped by folder, or a tree like the file tree, with counts per folder and kind. A filter, or type to find a file. Natural sort. The badge shown once when every file has it. Next change continues into the next file.
- **Actual:** one flat list of 653 full paths in a 235 px column. Each row wraps (`packages/pkg-000/src/module-` / `0.ts`) and is about 79 px tall, so the list is 51,339 px. Sorting is plain string order (`module-0, module-1, module-10, module-11…`), unlike the tree's natural order (MONO-7). In the scripted run every row carries the same yellow "Changed outside the agent" badge. Typing `pkg-011` on the list does nothing. N past the last change of a file only says "No more changes."; to get to the next file she has to Shift+Tab back to the list. The item line says "653 files" with no added, modified or deleted split. Each file's diff loads one request at a time (92-247 ms, fine).
- **Why it matters for Marta:** large mechanical refactors are exactly what she gives agents. Reviewing means skimming by package and spending time only on the odd files out: the 20,000-line generated files and the 50 deletions at the very end.
- **Suggested fix:** group rows by folder (collapsible, with counts), show the path as folder plus name with the middle truncated, sort naturally, add a filter box and type-ahead, and show "+600 modified, 3 added, 50 deleted". Show a badge that every file has once, in the item header. Let N at the last change move to the next file's first change. A "Viewed" check per file would help her split the review over several sessions.

### MONO-30 minor: after approve or reject the item shows no files, and the left-over list is one long sentence

- **Area:** crates/core/src/orchestration/review.rs (`review.list` for closed items), ui/src/inbox
- **Steps:** 1. Approve and commit the codemod review, or reject it after editing 40 of its files (r02, r03).
- **Expected:** the item still lists what was approved, committed or restored, with the commit id. Files left for her appear as a list she can open.
- **Actual:** the item turns into "Approved / 0 files" and "This review has no files" (r02 04-approved.png). After the reject the result is a single 1,383-character sentence, "Rejected. Left for you: packages/pkg-000/src/module-0.ts, packages/pkg-000/src/module-1.ts, …", in a 176 px block. The paths are not links.
- **Why it matters for Marta:** after a big approve she wants to check what went into the commit. After a reject, the 40 files left for her are her to-do list.
- **Suggested fix:** keep the file list, with its final state, for closed items, and show the commit id. Render conflicts as a list of buttons that open the file. Make the tree's "Changed only" view work from it.

### MONO-31 minor: in a long session the transcript gets no room, and it renders every entry

- **Area:** ui/src/agents (`AgentsPanel.svelte`, `Transcript.svelte`), ui/src/shell (right column layout)
- **Steps:** 1. Run the long fixture (400 `Read` calls with 30-line results, 400 replies, then `Bash`). 2. Look at the Agents panel while the `Bash` approval waits, then after it.
- **Expected:** the transcript fills the free height of the column and scrolls, its newest entry is visible, and a 1,600-entry transcript renders only what is on screen.
- **Actual:** while an approval waits, the transcript `<ol>` is 0 px tall (scrollHeight 152,778, clientHeight 0). The session block, with Interrupt and Stop, is below the fold, and scrolling does not bring it into view (r10 01/02 png). After the approval about 90 px are left, which fits two entries (r10 03-done.png). All entries are in the DOM (3,618 elements), and streaming them in caused 18 long frames of about 105 ms. Typing afterwards is fine (79 ms for 33 characters).
- **Why it matters for Marta:** for a long refactor she wants to glance at what the agent ran last without leaving the tree. Today she sees the approval card but not what led to it.
- **Suggested fix:** give the session view its own scroll area with the remaining height, or let the transcript open in the main area as a tab. Virtualize it with the same list component as the viewers (`Lines`). Pin the last entry while the session is working.

## What worked well

- Direct-mode attribution held at scale with the real driver. All 653 files changed during the agent's `Bash` counted as the agent's. Her edit to an agent file was not committed and not reverted. Reject restored the 50 deleted files, removed the 3 added ones and kept all 41 of her edits (`git status`: 42 lines, exactly hers plus the fixture's own change).
- Approve and commit: 0.9 s for 652 files, a clean commit with only the agent's files, and her other changes and staging untouched.
- The inbox opens in 116 ms with 653 files, the diff is virtualized (a 20,000-line file at about 3,200-3,900 elements), and arrow keys move through files with a diff per step in 92-247 ms. The header shows "Reviews (1 waiting)".
- The terminal itself is fast: 16,000 lines from `find .` in 0.8 s, Ctrl+C into a flood back to a prompt in 1.4 s, and a hidden terminal costs nothing while it floods.
- The round-3 problems re-checked here (MONO-19, MONO-21, MONO-24) are fixed, and Go to file now says "Showing 50 of 1,000".
