# Round 10 (2026-11-26): today's changes

Build: 7990e2b, headless, Chromium 158. Personas: newcomer, keyboard power
user, accessibility user, agent orchestrator. Scope: the git panel redesign
(Get and Send commits), Get started's last step, panel hairlines, Markdown in
the transcript and Jump to latest, files dropped or pasted into task boxes,
file icons and coloured git status, the Focus switch, the simplified trust
card.

46 findings: 16 major, 24 minor, 6 polish. 14 of the 16 majors are fixed in
the commits that follow this round; the two left need core work.

## Fixed

Agents (`ui/src/agents`):

- NEW-1, ORCH-4: a second "One agent, here" task was refused after the press
  with a core message. The box now says before Start which task has the
  project's folder, with "Close it" (it finished) or "Use Plan and split"
  (it works). The one-writer rule in the core is unchanged.
- NEW-2, A11Y-11: Get started's last step follows the task: "Claude answered"
  with Read, or "The task stopped with an error" with Open; opening it checks
  the step off. KEY-1: Open on a question moves to the request, Hide to the
  task box. NEW-10: done steps are no longer struck through.
- ORCH-1: the transcript stays at the end when a card opens under it, and the
  person's own message always brings it down; Jump to latest says "N new" in
  words. A11Y-2, KEY-5: Jump moves focus to the log. NEW-7, ORCH-9 (part):
  the transcript keeps at least 96 px.
- A11Y-1: the live region reads the agent's words, not its Markdown.
- ORCH-3: the worktree disk total in the header is opt-in like the other
  resource numbers, and the header's words give way before its buttons.
- KEY-4: the task and reply boxes show the accent focus ring.
- A11Y-5, KEY-6: after Trust project focus stays on the card; Escape folds
  the details. NEW-12: "code help (language servers)" and a shorter last
  paragraph (the sandbox facts stay).

Git (`ui/src/git`):

- KEY-2: Commit, Get, Send and Dismiss move focus to the next step.
- KEY-3: "Git: get new commits" and "Git: send commits" in the palette.
- A11Y-4, NEW-4, ORCH-8: a coloured letter, as in the tree, instead of a dot.
- NEW-5: "Changes since the last commit" while staging is hidden; "new file"
  instead of "untracked".

Shell, Focus, tree, icons:

- A11Y-3: the title bar's buttons never shrink, so Focus no longer covers the
  search field when narrow. A11Y-8 (part), KEY-10: the switch has
  `aria-pressed` and `aria-keyshortcuts`; its tooltip says what it hides.
- NEW-6: in Focus, "Focus" is a quiet word, not a pill, and the way back is
  "Exit Focus".
- NEW-13: the tree rings one row, not the whole panel.
- ORCH-11 (policy): no Claude logo for `CLAUDE.md` or `.claude`.

## Open, by area

Majors:

- NEW-3 (`crates/fs-index`, `ui/src/git`): nothing checks the remote, so "Get
  N commits" shows only after a fetch elsewhere and a reload. Needs a fetch
  through the Forge (network, the person's credentials) and the watcher to
  report `refs/remotes` and `FETCH_HEAD` changes.
- ORCH-2 (`crates/sandbox-linux`, core, `ui/src/git`, `ui/src/tree`): the
  sandbox's empty placeholder files (`CLAUDE.md`, `.envrc`, `.mcp.json`, ...)
  show as untracked while a session runs, and "Commit N files" would commit
  them. The core should say which files are placeholders.

Minors and polish:

- Agents: ORCH-5 (Cancel on the secret-file question drops the safe files
  too), ORCH-7 (Get started repeats the approval card), ORCH-10 (trust card
  hidden while a task is open), NEW-8 (`.ovra-attachments` not shown as
  ignored, no note why it exists), NEW-9 and A11Y-12 (Interrupt, Stop, Close),
  A11Y-6 (Markdown headings in the page outline), A11Y-9 (chip from the @
  menu not announced), KEY-8 (a half-typed mention is sent as text), KEY-9
  (no keyboard way to add tree files to a task).
- Focus: ORCH-6 (reviews still waiting count as done), A11Y-10 (no Add
  context button), KEY-7 (Alt+N, Alt+Shift+G, Mod+Shift+A say nothing),
  A11Y-8 (two texts below 4.5:1).
- Shell: A11Y-7 (hairlines 1.25:1, the title bar's one is a shadow that
  forced colours drop), NEW-11 (icon-only controls: sparkle, speaker).
- Not tested: a worker's push approval card (the fake worker never pushes).
