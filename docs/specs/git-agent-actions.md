# Agent actions in the git panel

Status: approved by the owner on 2026-11-25 (the agent proposes commits, the
person commits them with Commit all). Asked for by the owner: "let an agent commit my
changes, tell me what is new in the pulled commits, and keep the interface
simple".

## Problem

Writing commit messages, splitting a pile of changes into sensible commits
and reading what came in with a pull are chores an agent does well. Today
the person has to start an agent by hand, describe the task and copy the
answer back into the git panel.

## Goals

- One quiet "✨ Ask agent" button in the git panel opens a short menu of
  agent actions; its last line says each one starts an agent on the plan.
  No new panel, no new view, no settings.
- The agent only proposes. The person sees the proposal in the git panel and
  presses the button that acts on it. The agent never commits, stages,
  switches or pushes itself, and there is no push anywhere in this feature.
- Works on the person's existing agent: the same account, sandbox, billing
  line and agent limit as any other session.

Not goals: pushing or pulling from the panel (still a terminal, and PRs stay
`docs/specs/pr-flow.md`), splitting one file's hunks between commits,
resolving conflicts automatically, background or automatic runs (an action
starts only from a press, ADR 0006).

## The actions

The "✨ Ask agent" button sits at the end of the branch row, the panel's first row. Its
menu shows only the items that apply right now:

| Item                 | Shown when                                          | Result in the panel                             |
| -------------------- | --------------------------------------------------- | ----------------------------------------------- |
| Suggest commits      | there are changes                                   | a proposal card: commits, files, messages       |
| Write commit message | something is staged                                 | the message box is filled; nothing else changes |
| Summarize what's new | HEAD moved by a pull, merge or rebase (`ORIG_HEAD`) | a short summary card                            |
| Explain conflicts    | there are conflicted files                          | a short explanation card                        |

Each is also a palette command ("Git: suggest commits", ...).

While the agent works, the place of the result shows one line
("Suggesting commits…") with Stop. Errors (no agent set up, the agent limit
reached, the answer could not be read) show on that line in plain words,
with the same next step the Agents column gives.

**Suggest commits.** The card lists the commits in order: the message (its
first line in bold, the rest folded) and the files. The person can edit a
message in place or drop a commit. **Commit all** (the card's one primary)
makes them one by one: unstage what is staged, stage the commit's files,
`git.commit` with the message, as the person and with their hooks, exactly
as the Commit button does. A failing hook stops there and shows its output
like a normal commit; the commits made so far stay. Files the agent left out
stay as changes, and the card says how many. **Dismiss** throws the
proposal away. A proposal whose changes moved on since it was made (other files, or
more edits) says so, and the first press of Commit all only asks to press
"Commit anyway". Commit all stages whole files: a partly staged file goes
in whole, and the card says so.

**Write commit message.** Fills an empty message box, or replaces its text
with an Undo line when it is not empty. The person commits as usual.

**Summarize what's new / Explain conflicts.** A card with at most about
eight short lines and Dismiss (or Escape).

## How it works

- Prompts stay under the 64 KiB message limit counted in UTF-8: at most
  300 files listed, 40 KiB of diffs, 48 KiB of pulled commits (bodies and
  file lists go first, then older commits), and a last cut.
- The UI collects what the agent needs through the existing `git.*` calls
  (status, diffs of changed files) plus one new read-only call, and writes
  a prompt: a one-line title the Agents list shows ("Suggest commits for 7
  changed files"), the instructions, and the data, cut to fit the 64 KiB
  message limit (cut files are named, the agent can read them itself).
- It starts a **Research** session (`docs/specs/research.md`): the main
  checkout read-only, read tools only, its own small limit, no worktree.
  So the agent cannot change files or run commands, and nothing asks for
  approval. It uses the project's account, sandbox and research limit like
  any research session. The panel hides it from the Agents list (as a
  person's "close" does), because the result is in the git panel; Stop is
  on the panel's working line.
- The panel stops the session once it has its answer.
- The agent answers with a short sentence and, for commits, one fenced
  `json` block: `{"commits": [{"message": "...", "files": ["..."]}]}`. The
  git panel reads the session's last answer. A proposal is checked before
  it is shown: every file must be one of the changed files, no file twice,
  a message is non-empty and at most 4 KiB, at most 20 commits. Anything
  else is "The agent's answer could not be used" with the reason and Try
  again.
- The agent's text is shown as plain text, never as HTML, and commit
  messages go to `git.commit` as data (no shell).

New protocol (`git` domain): `git.pulled { project }` answers the commits
in `ORIG_HEAD..HEAD` (at most 100, newest first) with their full message
(cut to 2 KiB) and changed files with line counts, or none when there is
no `ORIG_HEAD`. Implemented in `GitRepo` with `git log` and
`--numstat`, read-only, like `git.log`.

## Safety

- The session is read-only by its mode; the agent has no tool to commit,
  stage or push, and the netproxy and sandbox apply as for any session.
- Every change to the repository is a person's press in the git panel, with
  the same calls and checks as doing it by hand (paths checked by the core,
  `git.switch` rules unchanged).
- Diffs go to the agent as the person's message, so they reach the model
  like anything an agent reads in the checkout. Binary files and files whose
  name looks like a secret (`.env*`, `.envrc`, `.git-credentials`,
  `*.pem`, `*.key`, `*.tfvars`, `id_*`, `service-account*.json`, ...; also
  the old name of a rename), and diffs holding a string that looks like a
  token (the audit log's list in `docs/specs/security.md`), are named in
  the prompt without their contents.
- Pulled commits and diffs can hold other people's text, so these research
  sessions start with `no_web`: read tools only, no web search through
  which a prompt injection could send a file out.

## Steps

| Step | Area                                       | What                                      |
| ---- | ------------------------------------------ | ----------------------------------------- |
| GA.0 | protocol, `services.rs`, `crates/worktree` | `git.pulled`, fake, real, tests           |
| GA.A | `ui/src/git`                               | ✨ menu, prompts, answer parsing, cards   |
| GA.B | `tests/e2e`, UX round                      | fake-agent e2e for each action, `just ux` |
