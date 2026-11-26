# Focus mode

Status: approved by the owner on 2026-11-26 (D1-D5 of
`docs/research/focus-mode.md` with the recommended options). Research and
mockup: `docs/research/focus-mode.md`, `docs/research/focus-mode/mockup.html`.

## What it is

One switch, **Focus**, that replaces the IDE in the window with one quiet
screen about the agents' work: what waits for the person, what is in
progress, what comes next and what is done. Code, the file tree, the
terminal, the rail and the side panels are hidden; **Exit Focus** brings them
back. It is meant for anyone who is easily pulled out of their work; it is
not named after a diagnosis and promises nothing about health or
productivity.

## Turning it on and off

- A **Focus** button in the IDE title bar, beside Reviews.
- Command `focus.toggle`, "Focus: agents only", category View, keys
  `Mod+Shift+.` (matched on `KeyboardEvent.code`, `Period`).
- In Focus the title bar shows the project, a "Focus" label and **Exit Focus**
  (the same command). Escape does not leave Focus: it belongs to the cards.
- Remembered per browser (`localStorage`, `ovra.focus`); on the start
  screen (no project) Focus waits and applies once a project opens.
- Turning Focus off restores the IDE as it was: nothing in the IDE is
  unmounted while Focus is on, so open files, drafts and terminals stay.

## The screen

One centred column (at most 680 px wide); parts with nothing in them are left out:

| Part           | Content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Header         | The project's name and a one-line count: "2 working · 1 next · 3 done".                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| Waiting on you | The existing approval cards (same component, same arming, keys and checks), then the agents' unanswered questions to the person from each task's board (answered as text, which allows nothing; when the agent gave answers to pick, each is a button that sends its exact text, the recommended one is marked with its reason but never picked for the person), then one card per session whose changes wait for review: its name, the agent's summary, "N files changed" and **See changes**. When nothing waits: "Nothing waits for you." |
| In progress    | One row per session that starts, works or waits for an approval, with the words `AgentSessions.status` gives ("Working", "Needs approval", "Workers are on it").                                                                                                                                                                                                                                                                                                                                                                             |
| Next           | Sessions waiting for a free agent or preparing a worktree, then the pending steps of each agent at work from its latest to-do list (Claude's `TodoWrite`), "Planned by <name>": at most 8 per agent, 160 characters each, shown as text.                                                                                                                                                                                                                                                                                                     |
| Done           | Sessions that finished, stopped or failed, with their words ("Done", "Failed: …").                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| New task       | At the bottom: a field for the next task, the way of working last chosen in the project ("One agent, here", "Plan and split", "Research") and **Start**; the task starts with the project's account without leaving Focus. Context chips, accounts and setup stay in the IDE.                                                                                                                                                                                                                                                                |

The status bar keeps the connection and phone pairing but not the open
file's facts, problems or language servers.

Rows show names and words only: no counters ticking, no moving lines, no
usage. Sessions the person closed are not shown.

## Quiet

While Focus is on, in-app toasts other than errors are not shown; the
cards on screen already say what waits. System notifications are unchanged
(they appear only while Ovra is in the background). The window title's
"(2) Ovra" count stays.

## Safety

- Approvals use the same component and rules as in the IDE: Allow once
  and Deny of equal weight, armed after 500 ms, nothing allowed unseen.
- **Always allow is not offered in Focus** (D3 of the research: it widens a
  policy for good and belongs where the rule is in view). The card says
  "Always allow is in the IDE." This only narrows what Focus can do.
- Keep and Discard are not offered in Focus: **See changes** leaves Focus
  and opens Reviews on that session's item.
- Agent text (titles, summaries) is shown as text, never as markup.

## Where you left off

After a break, one line under the header says what ended meanwhile and
what waits now: "While you were away, Fix sync finished and 2 things wait
for you." A break is ten minutes or more without the person: the window was
in the background, or nothing was typed or clicked in it. The line names at
most two sessions that finished, failed or stopped ("…, 3 more ended") and
counts what waits; when nothing ended and nothing new waits there is no
line. It stays until **Got it** or the next break, is announced politely
(`role="status"`), and does not move. Turning Focus on starts afresh: what
happened before is on screen already.

## Shared with the phone

One view model for Focus and the phone companion (D5):
`ui/src/focus/model.ts` reads sessions through `FocusSource` (which
`AgentSessions` satisfies) and imports nothing of the IDE, and
`ui/src/focus/away.svelte.ts` words the return. The phone's Home screen
uses both, so it sorts and words agents as Focus does; what a phone may do
stays limited in the core (`crates/core/src/companion.rs`).

## Tests

- Unit: the model sorts sessions into In progress, Next and Done and lists
  waiting reviews; closed sessions are left out. The return line names what
  ended, counts what waits and stays quiet after a short absence.
- Component: Focus shows the sections, hides Always allow, See changes leaves Focus,
  New task starts a task in Focus, the return line shows after a break and
  goes on Got it.
- e2e: turn Focus on with a fake agent waiting for approval, allow it from
  Focus, open the IDE again.
