# Git panel

Status: approved by the owner on 2026-11-25 (from the git/LSP research).

A sidebar view for the project's main checkout: what changed, staging files
and hunks, committing, branches, diffs, and blame and change markers in the
editor. It is for the person; agents keep working through their sessions
and the review inbox.

## Scope

In v1:

- Status: branch, upstream with ahead/behind, a merge or rebase in
  progress, and staged, unstaged, untracked and conflicted files.
- Stage and unstage files; stage and unstage single hunks of tracked files.
- Discard unstaged changes, with Undo (a snapshot is taken first).
- Commit what is staged, optionally amending the last commit.
- List local branches, switch, create a branch (and switch to it).
- Diff of one file (unstaged or staged), with per-hunk stage buttons.
- Editor: change markers in the gutter against the index, stage a hunk
  from the gutter, blame of the current line and a blame toggle.
- A short log of HEAD.

Not in v1: push, pull and fetch (they need the person's credentials and
prompts; the panel offers to open a terminal with the command typed but not
run), resolving conflicts (the panel shows them and opens the file), stash,
remote branches, submodules, `git init`.

## Protocol

Domain `git.*` (`crates/core/src/protocol/git.rs`,
`ui/src/lib/protocol/git.ts`). Requests name the project and paths relative
to its root. There are no `git.*` events: the panel and the editor refresh
on `fs.git_changed` (which also fires when the index or HEAD is written),
on `fs.changed` and after their own calls. Read-only git calls run with
`GIT_OPTIONAL_LOCKS=0` so they never hold the index lock.

The core (`crates/core/src/git.rs`) checks paths (inside the project, not
the root, no control characters, no `.git` segment anywhere), sizes and branch names before the
`GitService` sees them; it takes the discard snapshot through `Worktrees`
and refuses `git.switch` while a session writes the main checkout (direct
mode, ADR 0005). Creating a branch and committing stay allowed then: they do
not change files, and the review diff is against the session's baseline
snapshot.

## Service

`GitService` (`crates/core/src/services.rs`) is implemented by `GitRepo` in
`crates/worktree/src/repo/`, which reuses the crate's `Git` builder: no
terminal prompt, no inherited `GIT_*` variables, literal pathspecs, no file
monitor, submodules ignored. Diffs, blame and show also pass
`--no-ext-diff --no-textconv`. Only the git command line, no libgit2.

- `status`: one `git status --porcelain=v2 -z --branch --untracked-files=all`
  (plus reading `.git` for a merge or rebase in progress). When the project
  is a folder inside a repository, paths are made relative to the project
  and files outside it are left out (commit still commits everything that
  is staged, as git does).
- `stage`/`unstage`: `git add --all` and `git reset` (which also works
  before the first commit), paths on stdin
  (`--pathspec-from-file=- --pathspec-file-nul`), never on the command line.
- `stage_hunk`: checks the hunk first (only context, added and removed
  lines, counts matching its header: `Hunk::check`, in the core and again
  in the service) so a client cannot slip another file's patch in, then
  builds a one-file patch from the hunk and runs
  `git apply --cached` (`--reverse` to unstage); a patch that no longer
  applies is `conflict`.
- `discard`: tracked files from the index (`git checkout-index` or
  `git restore --worktree`), untracked files deleted without following
  symlinked folders.
- `commit`: `git commit -F -` with the message on stdin, `--amend` when
  asked. The person's hooks and signing config run, as in their terminal;
  a failing hook is `conflict` with its output. Commit and amend refuse a
  message-only commit with nothing staged (`bad_request`), except amend.
- `branches`: `git for-each-ref refs/heads` sorted by committer date.
- `switch`: `git check-ref-format --branch`, then `git switch <name>`
  (never `--force`, never `--discard-changes`).
- `create_branch`: `git switch -c` or `git branch`.
- `blame`: `git blame --porcelain` of the working-tree file, at most 4 MiB.
- `show`: `git cat-file` of `HEAD:<path>` or `:<path>`, at most 4 MiB, null
  for binary files.
- `log`: `git log -n <limit>` with a fixed format.

Tests run real git in temporary repositories (nextest group `git`) and stay
under the budgets in `scripts/test-budgets.json`.

## UI

`ui/src/git/`: a "Git" tab in the left sidebar, `Mod+Shift+G` (browser
stand-in `Alt+Shift+G`), command "Show git changes". It shows the branch
(with a switch/create menu), a commit box (Mod+Enter commits, "Amend"
checkbox), staged and unstaged/untracked lists with stage, unstage and
discard buttons and "Stage all", and a conflicts list. Selecting a file
shows its diff in the main area with per-hunk Stage/Unstage. Discard shows
an Undo that stays until the next discard. English only, keyboard reachable,
announcements through `aria-live`.

`ui/src/editor/`: gutter markers (added, changed, deleted) from a line diff
of the buffer against `git.show` of the index, so unsaved edits show too;
clicking a marker offers "Stage hunk" (saved files only) and "Revert";
inline blame of the cursor line (author, relative time, summary) and a
"Git: toggle blame" command.

## Risks and follow-ups

- Hooks run code from the repository; an agent in direct mode may have
  changed it. Later: mark the commit box when hook configuration files
  (`package.json`, `.pre-commit-config.yaml`, `.husky/`, `.lintstagedrc*`)
  changed since the last commit.
- Conflict resolution, stash, push/pull with credentials: later specs.
