# First-run round summary (2026-11-25)

Personas: newcomer, agent-orchestrator, security-reviewer, on main at
cbe74a1 (after iterations 1 and 2 of `plan.md`). 29 findings, no blockers.
A newcomer can now open a folder, start a task and answer approvals without
help; where they get lost is the end of a task. Experts keep their speed;
problems appear with several tasks at once.

## Iteration 3, by owning area

**Trust card and setup (`ui/src/agents/setup`)**

- SEC-1 major: the "Review and trust" command focuses the Trust button; one
  more Enter trusts. Focus the heading instead; trust buttons get the
  approvals' arming delay and ignore key repeat.
- SEC-3 major: risky project `env` names (`ANTHROPIC_*`, `*_BASE_URL`,
  `*_PROXY`, `NODE_OPTIONS`, `BASH_ENV`, `LD_PRELOAD`, `PATH`, ...) get a
  named warning line.
- NEW-6, NEW-7, SEC-6, SEC-7, SEC-10: say "once you trust", fewer
  specialist words, unreadable files named in the warning, the new hook's
  command in the changed warning, warnings not by colour alone.

**Approvals and notices (`ui/src/approvals`, `ui/src/notify`)**

- SEC-2 major: at narrow sizes the request scrolls fully into view and the
  area keeps a minimum height while something waits.
- ORCH-1 major: toasts above dialogs; the approvals command works while the
  review inbox is open.
- ORCH minor: the empty overlay must not swallow the next click.
- SEC-5: the window title counts waiting requests.
- NEW-8: an early click on Allow says it is not ready yet, not silence.

**Agents panel and task text (`ui/src/agents`, `crates/core/src/orchestration`)**

- NEW-1 major: after a task in the project folder, list the files it changed
  with links to their diffs.
- NEW-2 major: one consistent end state; a split task's end says how many
  changes wait in Reviews, with a Review button.
- NEW-5 major: no internal coordinator text (agent ids, tool names) in the
  transcript.
- ORCH-2 major: worker names carry their task; toasts use the final name.
- ORCH-4 major: once tasks exist, the task list comes before the setup and
  trust cards (which collapse to one line).

**Header and shell (`ui/src/shell`, `ui/src/tree`)**

- NEW-3/NEW-4 major: the project name opens a project switcher with "Open
  a folder…"; the open project survives a reload.
- ORCH minor: the More menu shows each item's shortcut and the palette's
  names.
- NEW-9: one word per idea (Approvals for "may Claude do this?", Reviews for
  "keep these changes?").

**Owner decision**

- ORCH-3 major: two split tasks ran 8 agents at once; the limit of 3 is per
  task. Asked the owner whether it is one limit for all tasks.

**Other threads**

- SEC-4 (account picker shows billing and sign-in state) goes to the Claude
  accounts thread.
