# Agent actions in the git panel

Status: draft, 2026-11-25. Asked for by the owner: "let an agent commit my
changes, tell me what is new in the pulled commits, and keep the interface
simple".

## Problem

Writing commit messages, splitting a pile of changes into sensible commits
and reading what came in with a pull are chores an agent does well. Today
the person has to start an agent by hand, describe the task and copy the
answer back into the git panel.

## Goals

- One quiet ✨ button in the git panel opens a short menu of agent actions.
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

The ✨ button sits at the end of the branch row, the panel's first row. Its
menu shows only the items that apply right now:

| Item                    | Shown when                         | Result in the panel                                 |
| ----------------------- | ---------------------------------- | --------------------------------------------------- |
| Suggest commits         | there are changes                  | a proposal card: commits, files, messages           |
| Write commit message    | something is staged                | the message box is filled; nothing else changes     |
| Summarize what's new    | HEAD moved by a pull, merge or rebase (`ORIG_HEAD`) | a short summary card                |
| Explain conflicts       | there are conflicted files         | a short explanation card                            |

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
proposal away. A proposal whose files changed since it was made is marked
"Changed since suggested" and asks again before it commits.

**Write commit message.** Fills an empty message box, or replaces its text
with an Undo line when it is not empty. The person commits as usual.

**Summarize what's new / Explain conflicts.** A card with at most about
eight short lines, Dismiss, and "Open in Agents" to ask follow-up questions
in the same session.

## How it works

- The UI collects what the agent needs through the existing `git.*` calls
  (status, diffs of changed files) plus one new read-only call, and writes
  a prompt: a one-line title the Agents list shows ("Suggest commits for 7
  changed files"), the instructions, and the data, cut to fit the 64 KiB
  message limit (cut files are named, the agent can read them itself).
- It starts a **Research** session (`docs/specs/research.md`): the main
  checkout read-only, read tools only, its own small limit, no worktree.
  So the agent cannot change files or run commands, and nothing asks for
  approval. The session shows in the Agents column like any other, so its
  billing and transcript stay visible, and it can be stopped there too.
- The agent answers with a short sentence and, for commits, one fenced
  `json` block: `{"commits": [{"message": "...", "files": ["..."]}]}`. The
  git panel reads the session's last answer. A proposal is checked before
  it is shown: every file must be one of the changed files, no file twice,
  a message is non-empty and at most 4 KiB, at most 20 commits. Anything
  else is "The agent's answer could not be read" with "Open in Agents".
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
  name looks like a secret (`.env*`, `*.pem`, `*.key`, `id_*`, the secret
  patterns of `docs/specs/security.md`) are named in the prompt without
  their contents.

## Steps

| Step | Area                                   | What                                       |
| ---- | -------------------------------------- | ------------------------------------------ |
| GA.0 | protocol, `services.rs`, `crates/worktree` | `git.pulled`, fake, real, tests        |
| GA.A | `ui/src/git`                           | ✨ menu, prompts, answer parsing, cards    |
| GA.B | `tests/e2e`, UX round                  | fake-agent e2e for each action, `just ux`  |
