# Focus mode

Status: research, 2026-11-25, revised 2026-11-26. Nothing here is approved; the decisions at the
end need the owner. Builds on the design guide (`docs/design/`), the Quiet
style (`docs/specs/quiet-style.md`) and "Surfacing an approval" in
`docs/design/patterns.md`. Mockup: `docs/research/focus-mode/mockup.html`.

The owner asked whether any IDE has a mode for people with ADHD or similar
attention differences. None of the large IDEs has one. This page looks at what
exists, what research and accessibility guidance say, and proposes a Focus
mode for Ovra. It makes no medical or therapeutic claims: Focus mode is a way
of working that some people prefer, not a treatment, and it is useful to
anyone who is easily pulled out of their work.

## 1. Summary

Revised 2026-11-26 after the owner's feedback: Focus does not show code at
all. It is an agents-only view.

- **Recommendation:** one switch, "Focus", that replaces the IDE with one
  quiet screen: what waits for you (agent questions with a recommended
  answer, approvals, ready reviews), what is in progress, what comes next by
  the plan, and what is done. Code, tree, terminal and panels are hidden;
  "Open IDE" is one click away. Name it Focus, never after a diagnosis.
- **Why Ovra is a good fit:** in an agent-first IDE the person's main job is
  answering agents. Agents are the biggest source of interruptions, and Ovra
  controls all of them. Other IDEs can only hide panels; Ovra can decide
  _when_ the person is asked, and show the work without the code.
- **One model, three places:** the Focus view, the task board's "Waiting on
  you" and the phone companion's home screen are the same data and the same
  components, with different limits.
- **Safety stays as it is:** Allow is always an explicit tap, never
  recommended, never pre-selected, armed after 500 ms. "Always allow", Keep
  and Discard stay in the IDE, where the rule or the diff is in view.

## 2. What exists today

| Tool                                                                         | What it does                                                                                                                                                     | What it does not do                                                                                      |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| VS Code Zen Mode                                                             | Hides side bar, panels, status bar, tabs; optional full screen, centred layout, hidden line numbers; `zenMode.silentNotifications` hides notifications while on. | Hidden notifications are only muted, not collected or offered later. No idea of "what next" or resuming. |
| JetBrains Distraction-free / Zen mode                                        | Centres the code and hides all tool windows; Zen adds full screen. Tool windows still open on demand.                                                            | Same: a layout preset.                                                                                   |
| Neovim zen-mode.nvim, Qt Creator Zen mode                                    | Centred, single window.                                                                                                                                          | Layout only.                                                                                             |
| VS Code extensions (Pomodoro timers, current-line highlighters, "todo tree") | One piece each: a timer, stronger cursor-line contrast, a list of TODOs.                                                                                         | Not connected to each other or to notifications.                                                         |
| macOS Focus (and Focus filters, `SetFocusFilterIntent`, macOS 13+)           | System-wide do-not-disturb with schedules; apps can adapt their own content when a Focus is on.                                                                  | Apps must opt in; Ovra does not yet.                                                                     |
| Apple Assistive Access (iOS/iPadOS)                                          | A reduced, large-target mode for people with cognitive disabilities.                                                                                             | Not on macOS; shows the direction Apple takes: fewer choices, one task at a time.                        |
| Microsoft Viva Insights focus plan                                           | Books focus time and silences Teams chats during it.                                                                                                             | Calendar-level, not in the tool where work happens.                                                      |
| Goblin.tools Magic ToDo, Tiimo                                               | Break a vague task into small steps; visual countdown timers. Popular with people who describe themselves as having ADHD; evidence is mostly user reports.       | Outside the IDE.                                                                                         |

**Gap:** every IDE treats focus as "hide panels". None collects interruptions
and hands them back one at a time, none helps the person re-enter after a
break, and none has agents that it could ask to wait.

## 3. What research and guidance say

Findings are summarised neutrally; the studies are mostly small qualitative
studies or survey analyses, so they show patterns, not proof.

### Developers with ADHD

- A case study of software engineers with ADHD (Liebel and Langlois, ICSE
  2024) found estimating effort and keeping focus hard, and strengths in
  creativity and systems thinking.
- An analysis of the 2022 Stack Overflow survey (Verma, Cruz and Liebel, 2025)
  found engineers with ADHD report more interruptions caused by _waiting for
  answers_. In Ovra, the person is often the one an agent waits on, and the
  person waits on agents; both directions matter.
- "Get Me in the Groove" (UMass, mixed methods) found ADHD programmers report
  struggling more often with time management and design, and names pull
  request notifications among tools that fit their focus poorly.
- A 2026 paper on LLM support for engineers with ADHD points at interruptions,
  context switching, synchronous communication and long multi-stage tasks, and
  notes that support is rarely built into the tools.

### Interruptions and resuming (everyone)

- Gloria Mark's field studies: people switch screens every 45 s on average
  (2022), usually pass through about two other tasks before returning, and
  each interruption costs time and stress.
- Parnin and Rugaber (86 programmers, 10,000 sessions; survey of 414): getting
  back into an interrupted task often takes 15-30 minutes. Programmers go back
  to the last edited place, read their diff, re-run the program and use notes,
  and tools barely help. A follow-up (Parnin and DeLine) found cues showing
  recent activity doubled successful resumption compared with notes alone.

### Accessibility guidance

- **W3C COGA, "Making Content Usable for People with Cognitive and Learning
  Disabilities"** (supplemental to WCAG). Objectives that apply directly:
  help users focus and avoid distraction from the task; help users find what
  they need; use clear and understandable content (summaries, short text);
  help users avoid mistakes and recover; do not rely on memory; support
  adaptation and personalisation. Its patterns include "Ensure controls and
  content do not move unexpectedly", keeping critical content short, avoiding
  too much content at once, and helping people recover their place after a
  distraction.
- **WCAG 2.2:** 2.2.2 Pause, Stop, Hide (moving or auto-updating content over
  five seconds needs a way to stop it; one control for all of it is
  recommended), 2.2.4 Interruptions (AAA: interruptions can be postponed or
  suppressed), 2.3.3 Animation from Interactions (AAA; honour
  `prefers-reduced-motion`).
- **Apple HIG Accessibility and WWDC23 "Build accessible apps with Assistive
  Access":** reduce cognitive load, fewer options at once, avoid time
  pressure, keep interactions familiar, honour Reduce Motion.

### What follows for an agent-first IDE

1. **Interruptions are the product's own output.** Agents produce approvals,
   questions, finished turns and reviews. Ovra can queue them instead of
   pushing them (WCAG 2.2.4, COGA "avoid distraction").
2. **Waiting is safe to make visible but quiet.** An approval only blocks its
   agent; nothing runs while it waits. Holding it costs agent time, not safety.
3. **One step at a time.** A list of ten waiting items asks the person to
   plan; one "next" item does not (COGA "avoid too much content", Assistive
   Access).
4. **Resuming deserves a feature.** Ovra knows the last edited file and line,
   what agents did meanwhile and what waits. That is exactly the cue set
   Parnin found works.
5. **No time pressure, no scoring.** Timers, streaks and "productivity"
   numbers add pressure; the owner also decided that usage and resource
   displays are opt-in. Anything about time is opt-in and silent.

## 4. What Ovra already has

Checked in the code on `main`, 2026-11-25:

- Approvals already follow "one interruption per burst": the attention card in
  the Agents column, a `--warn` badge when it is folded, a banner in other
  views, one system notification per burst (`patterns.md`,
  `ui/src/notify/agent-events.ts` keeps one toast for all waiting approvals).
- Task rows sort "needs you" first; a task says "N changes wait for your
  review" (`ui/src/agents/AgentsPanel.svelte`). There is no single named
  "Waiting on you" place yet; it is spread over the attention area, the review
  count in the title bar and toasts.
- Quiet status lines, collapsed setup cards (`ProjectTrust`, `ClaudeSetup`),
  "Details" disclosures in Reviews and the agent's summary on review items.
- Motion follows the system's Reduce motion through `prefers-reduced-motion`
  (title bar, rail, live activity line); `tokens.md` says loops stand still.
  Settings → Accessibility has only the terminal screen-reader switch and the
  line "Motion, contrast and colours follow your system's settings." There is
  no Reduce motion switch inside Ovra.
- Panels can be folded (sidebar, Agents column) with shortcuts.

So most building blocks exist; Focus mode mainly reroutes what is already
there.

## 5. Proposal: Focus, agents only

The first draft of this page kept the editor and hid the rest. The owner
asked for less: no code at all, only the state of the work and the agents'
questions, short and with recommendations. This section replaces that
draft; the research above still applies.

One switch, **Focus**: View menu, the palette ("Turn on Focus"), Settings →
Accessibility and one shortcut (candidate `Mod+Shift+Period`, matched on
`KeyboardEvent.code`). The window shows one screen; "Open IDE" in the title
bar (same shortcut) goes back to the full IDE, and coming back restores
Focus. The state is per window and remembered.

### The screen

On the Mac, two columns; on the phone and narrow windows, one. From the top:

| Part               | What it shows                                                                                                                            | Where the data comes from                                              |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| Goal               | The task's title, "3 agents · started 40 min ago". With several tasks, one line per task, the one that needs you first.                  | Tasks (`orchestration`)                                                |
| **Waiting on you** | One card per item, oldest blocking first: agent questions, approvals, ready reviews. Nothing else is in this column.                     | Task board questions to `person`, the approval queue, the review inbox |
| In progress        | One row per running agent: name, what it does now in words ("running the tests", "waits for your answer"), elapsed time as plain text.   | Session status and steps                                               |
| Next               | What comes after, by the plan: planned steps and workers waiting for a free agent, with "after Retry backoff" when one waits on another. | See "The plan" below                                                   |
| Done               | Finished items with their outcome ("kept", "answered", "discarded").                                                                     | Tasks and the review inbox                                             |
| Composer           | "Tell the agents what to do next…": starts a task, the only way agents start (ADR 0006 rule).                                            | Existing composer                                                      |

No toasts, banners, counters or moving lines. Rows change their words when
an agent moves on; nothing slides or blinks. Desktop notifications are off
while Focus is on and the window is visible.

### Cards in "Waiting on you"

- **Question.** The agent's question in one sentence, two to four options,
  one marked "Recommended" with a one-line reason, and "Answer in your
  words". One tap answers; the card becomes one line "You answered …" with
  Undo for a few seconds, and the agent goes on.
- **Approval.** What the agent wants in plain words, the command or path in
  monospace, the facts that matter ("In the sandbox · no network", "Outside
  the project folder", "No sandbox"), and **Allow once** and **Deny** of equal
  weight. No recommendation: an agent does not advise on its own
  permissions, and Ovra does not guess risk beyond the facts it knows.
  Buttons arm after 500 ms, as on the desktop cards and the phone. "Always
  allow" is not here; the card says it is in the IDE.
- **Ready to review.** The agent's one-line summary, "2 files changed ·
  tests pass" and **See changes**, which opens the diff. Keep and Discard
  stay with the diff (see D3).

### The plan ("Next")

Ovra has no structured plan today: a coordinator spawns workers, and
workers over the limit wait as "Waiting for a free agent". Options:

1. **Queued workers only.** Works now, but shows "Next" only when the limit
   is reached.
2. **The coordinator's to-do list.** Claude keeps its plan with the
   `TodoWrite` tool, which arrives in the stream we already read; the core
   could keep the latest list per session as read-only data. No new tool,
   but it depends on the CLI's tool shape (check with the live suite).
3. **A `plan` entry on the task board.** The coordinator posts its steps
   with a board tool; Ovra marks steps done as workers finish. Clearest,
   but it changes the MCP tools (needs the owner and a human review of
   `tools.json`).

Recommended: 1 and 2 in the MVP, 3 later if 2 proves unreliable.

### Recommended answers

The task board's `ask(to: "person", question)` takes free text today. For
one-tap answers it needs optional `options` (2-4 short labels),
`recommended` (one of them) and `reason` (one line). Without them the card
shows the question and a reply box. The role prompt asks agents to give
options and a recommendation when they ask the person. This is a change to
an MCP tool other areas depend on, so it needs the owner (D2).

Agent text on the card is data: options, reasons and summaries are escaped
like every board entry and never become buttons that do anything but send
that answer back as text.

### How it fits with the rest

- **Task board (ADR 0012).** Focus is the person's side of the board: the
  board's questions to `person` and Ovra's notes feed "Waiting on you" and
  the rows; the full timeline, claims and overlaps stay in the IDE's Board
  tab. The board already decided that questions to the person are quiet (no
  toasts); Focus follows the same rule for everything.
- **"Waiting on you".** Today it is spread over the attention area, the
  title-bar review count and toasts. Focus makes it one list with one order
  (blocking approvals, questions, reviews); the IDE shows the same list in
  the Agents column, so the person sees the same thing in both.
- **Phone companion.** Its home screen ("Needs you", "Working") is this
  view on a small screen. Build the view model once in the UI (for example
  `ui/src/focus/`) and use it in both; the phone keeps its stricter limits
  (no Always allow, no Keep or Discard, no starting agents in the MVP),
  enforced in the core by its method allowlist, not by the view.

### MVP

| #   | Feature               | Notes                                                                                        |
| --- | --------------------- | -------------------------------------------------------------------------------------------- |
| F1  | Switch and "Open IDE" | View menu, palette, Settings, shortcut.                                                      |
| F2  | The screen            | Goal, Waiting on you, In progress, Next (options 1 and 2), Done, composer.                   |
| F3  | Quiet                 | No toasts, desktop notifications or moving lines while Focus is visible.                     |
| F4  | Cards                 | Question (free text until D2 lands), approval (explicit, armed), review (See changes).       |
| F5  | Where you left off    | After a break, one line at the top: "Meanwhile Fix sync finished and 2 things wait for you." |

### Next

- One-tap answers with a recommendation (after D2).
- A `plan` board entry if `TodoWrite` is not enough.
- Several projects at once: one section per project.
- "Reduce motion in Ovra" as its own switch in Settings → Accessibility, for
  people who want it without Focus.

### Not doing

- A mode named after a diagnosis, or copy that promises better focus,
  productivity or health.
- Streaks, scores, badges or "time saved" counters.
- Recommending, pre-selecting or auto-allowing a permission; Always allow,
  Keep or Discard without the rule or the diff in view.
- Hiding safety facts: "No sandbox", trust warnings and the billing account
  stay on the card where the decision is made (design principle 3).

## 6. Copy

- Switch: "Focus". Palette: "Turn on Focus" / "Turn off Focus". Button:
  "Open IDE".
- Settings line: "Shows only what agents do and what they need from you.
  Code stays one click away."
- Sections: "Waiting on you", "In progress", "Next", "Done".
- Empty: "Nothing waits for you. 2 agents working."
- Approval note: "Always allow is in the IDE."

Sentence case, verbs on buttons, no exclamation marks (patterns.md).

## 7. Shape of the work (no code yet)

- UI: a new folder `ui/src/focus` (view model shared with the phone, the
  screen, the cards), small changes in `ui/src/shell` (the switch, hiding
  the IDE), `ui/src/notify` (quiet while Focus is visible) and
  `ui/src/settings`. Hot spots first, in their own small task:
  `ui/src/shell/workspace.svelte.ts` (focus state) and
  `ui/src/keys/commands.ts` (commands).
- Core: the latest `TodoWrite` list per session as read-only data (plan
  option 2), a small `agents` protocol field. With D2: `options`,
  `recommended`, `reason` on board questions (protocol, MCP tool, UI).
- Larger than a day, so a short spec `docs/specs/focus-mode.md` goes to the
  owner before code.
- Tests: unit tests for the order of "Waiting on you" and for rows from fake
  sessions; one e2e flow with fake agents: turn on Focus, answer a
  question, allow an approval, open the IDE and come back.

## 8. Decisions for the owner

| #   | Question                                                                                              | Recommendation                                                                                                                          |
| --- | ----------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | Focus is agents only: no code, tree or terminal; "Open IDE" to leave.                                 | Yes, as the owner asked.                                                                                                                |
| D2  | Board questions to the person get optional options, a recommended one and a reason (MCP tool change). | Yes; until then cards show a reply box.                                                                                                 |
| D3  | Keep and Discard in Focus                                                                             | Not in Focus: "See changes" opens the diff in the IDE, where Keep and Discard are. Reviewing without seeing the diff is easy to regret. |
| D4  | Where "Next" comes from                                                                               | Queued workers plus the coordinator's `TodoWrite` list now; a board `plan` entry later if needed.                                       |
| D5  | One view model for Focus and the phone companion                                                      | Yes; the phone keeps its stricter limits in the core.                                                                                   |

## Sources

- VS Code Zen Mode settings: [microsoft/vscode#83067](https://github.com/microsoft/vscode/issues/83067), [#68291](https://github.com/microsoft/vscode/issues/68291)
- JetBrains: [IDE viewing modes](https://www.jetbrains.com/help/idea/ide-viewing-modes.html), [Distraction-free tip](https://www.jetbrains.com/guide/tips/distraction-free)
- [zen-mode.nvim](https://github.com/folke/zen-mode.nvim), [Qt Creator Zen mode](https://doc.qt.io/qtcreator/creator-how-to-use-zen-mode.html)
- Apple: [SetFocusFilterIntent](https://developer.apple.com/documentation/appintents/setfocusfilterintent), [WWDC22 Meet Focus filters](https://developer.apple.com/videos/play/wwdc2022/10121/), [HIG Accessibility](https://developer.apple.com/design/human-interface-guidelines/accessibility), [WWDC23 Assistive Access](https://developer.apple.com/videos/play/wwdc2023/10032)
- [Viva Insights focus plan](https://support.microsoft.com/en-us/viva/insights/focus-plan-for-viva-insights)
- [Goblin.tools Magic ToDo](https://warwick.ac.uk/services/library/using/productivity-tools/magic-todo-gobin-tools), [Tiimo focus timers](https://www.tiimoapp.com/timers)
- W3C: [COGA, Making Content Usable](https://www.w3.org/TR/coga-usable/), [WCAG 2.2 Pause, Stop, Hide](https://www.w3.org/WAI/WCAG22/Understanding/pause-stop-hide), [Animation from Interactions](https://www.w3.org/WAI/WCAG21/Understanding/animation-from-interactions)
- Liebel and Langlois, [Challenges, Strengths, and Strategies of Software Engineers with ADHD](https://arxiv.org/abs/2312.05029)
- Verma, Cruz and Liebel, [arXiv 2506.03840](https://arxiv.org/abs/2506.03840)
- [Get Me in the Groove](https://people.cs.umass.edu/~mendres/papers/GetMeInTheGroove.pdf) (UMass)
- [Extending LLM-based support for software engineers with ADHD](https://arxiv.org/abs/2610.00555)
- Gloria Mark: [Columbia Magazine](https://magazine.columbia.edu/article/cant-concentrate-heres-how-focus-digital-age), [Gallup interview](https://news.gallup.com/businessjournal/23146/too-many-interruptions-work.aspx)
- Parnin and Rugaber, [Resumption strategies for interrupted programming tasks](https://sites.cc.gatech.edu/reverse/repository/resumptionstrategies.pdf); Parnin and DeLine, [Evaluating cues for resuming interrupted programming tasks](https://www.microsoft.com/en-us/research/publication/evaluating-cues-for-resuming-interrupted-programming-tasks/)
