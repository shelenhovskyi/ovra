# Focus mode

Status: research, 2026-11-25. Nothing here is approved; the decisions at the
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

- **Recommendation:** build one switch, "Focus", that shows only the current
  work, holds every interruption in one quiet place, and offers a single
  "What next?" step instead of lists. Name it Focus, never after a diagnosis.
- **Why Ovra is a good fit:** in an agent-first IDE the person's main job is
  answering agents (approvals, reviews, questions). Agents are therefore the
  biggest source of interruptions, and Ovra controls all of them. Other IDEs
  can only hide panels; Ovra can decide _when_ the person is asked.
- **MVP (UI only, no protocol change):** the switch, a one-pane layout, held
  interruptions in a "Waiting on you" tray, "What next?", stilled motion and a
  "Where you left off" line when the person comes back.
- **Safety stays as it is:** holding an approval never allows it. The agent
  waits. Warnings still show at the point of decision.

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

## 5. Proposal

One switch, **Focus**. In the View menu, the palette ("Turn on Focus"),
Settings → Accessibility and one shortcut (to be chosen in the keys task;
candidate `Mod+Shift+Period`, matched on `KeyboardEvent.code`). The title bar
shows a small "Focus" chip while it is on; clicking it turns Focus off.
The state is per window and remembered.

### MVP

| #   | Feature                                   | What the person sees                                                                                                                                                                                                                                                                                                                                                               | Builds on                                     |
| --- | ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| F1  | **Switch**                                | One command turns everything below on or off together. No sub-settings in the MVP besides F5's.                                                                                                                                                                                                                                                                                    | View menu, palette, keys                      |
| F2  | **One pane**                              | Rail, sidebar, Agents column, terminal dock and status bar fold away. The current file or the current task fills the window, text centred at a readable width (about 100 characters). The title bar keeps the project name, "Search or run", the Focus chip and the Waiting chip. Everything is still one shortcut or palette command away.                                        | Existing fold commands                        |
| F3  | **Held interruptions ("Waiting on you")** | Approvals, questions, finished turns, review requests and failures go into one tray behind a quiet chip, "2 waiting". The chip changes its number without animation or colour flash; `--warn` tone only when an agent is blocked. No toasts, no banner, no system notifications while Focus is on. Agents keep working; an agent that needs approval waits.                        | Approval queue, notify, review inbox          |
| F4  | **What next?**                            | One button (chip click, or a shortcut) shows _one_ card: the item that unblocks the most. Order: an approval that blocks an agent, a question, a review that is ready, a failed agent, a finished turn. Card actions are the usual ones (Allow once, Deny, Open review); "Later" puts it back at the end. After acting, the card offers the next one or says "Nothing else waits." | Approvals card, inbox                         |
| F5  | **Still motion**                          | Loops stand still (activity line, thinking dots), status text updates at most every few seconds, no counters ticking. Also offered as its own switch in Settings → Accessibility, "Reduce motion in Ovra", for people who want it without Focus.                                                                                                                                   | `prefers-reduced-motion` rules in `tokens.md` |
| F6  | **Where you left off**                    | After a break (window in the background or no input for 10 minutes, and when Focus is turned off), one line at the top of the pane: "You were editing `sync.ts`, line 42. Meanwhile "Fix sync" finished; 2 things wait." Actions: "Go back" (opens the file at that line), "What next?", dismiss. Nothing leaves the machine.                                                      | Editor state, sessions                        |

### Next

| #   | Feature                  | Notes                                                                                                                                                                                                                           |
| --- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F7  | **Short answers first**  | In Focus, an agent's turn shows a three-line summary with the full transcript behind "Details". Needs the agent to write a summary first (role prompt change) or reuses `request_review`'s summary; check with the agents area. |
| F8  | **Steps, one at a time** | "Split into steps" in the composer: the coordinator proposes 3-7 steps; Focus shows the current step and "2 of 5" in text, not a progress ring. Uses existing orchestration.                                                    |
| F9  | **Note for later**       | A quick capture (shortcut) for a stray thought; it goes into the tray as "Your note" and is offered by "What next?" at the end.                                                                                                 |
| F10 | **Optional timer**       | Off by default (owner rule: such displays are opt-in). Shows elapsed time as text; at the end, a quiet line in the tray, never a sound or modal.                                                                                |
| F11 | **Reading comfort**      | Stronger current-line highlight, a little more line height in Focus. Editor settings, not new UI.                                                                                                                               |

### Later

- macOS Focus filter (`SetFocusFilterIntent`): turn Ovra's Focus on with the
  system's Work Focus. Needs native code in `app/` and is platform work.
- Phone companion: held items can be answered from the phone (already only
  contentless pushes); respect Focus there too.

### Not doing

- A mode named after a diagnosis, or copy that promises better focus,
  productivity or health.
- Streaks, scores, badges, confetti or "time saved" counters.
- Auto-allowing anything because the person is in Focus.
- Hiding safety facts: "No sandbox", trust warnings and the billing account
  still show on the card where the decision is made (design principle 3).

## 6. Copy

- Switch: "Focus". Palette: "Turn on Focus" / "Turn off Focus".
- Settings line: "Shows only the current work and holds interruptions until
  you ask for them."
- Chip: "2 waiting"; tooltip "2 things wait for you · What next?".
- Empty: "Nothing waits for you."
- Return line: "You were editing sync.ts, line 42. Meanwhile …".

Sentence case, verbs on buttons, no exclamation marks (patterns.md).

## 7. Shape of the work (no code yet)

- MVP is UI only: `ui/src/shell` (layout and chips), `ui/src/notify` (hold
  instead of show), `ui/src/approvals` and `ui/src/inbox` (the one-card view),
  `ui/src/settings` (two switches). Shared hot spots that change first in
  their own small task: `ui/src/shell/workspace.svelte.ts` (focus state, last
  edit place) and `ui/src/keys/commands.ts` (the commands).
- A new folder `ui/src/focus` could own the tray ordering and "What next?"
  logic so parallel work does not collide.
- No protocol, core or security change for the MVP. F7 and F8 touch agent
  prompts; F11 is editor settings.
- It is larger than a day, so a short spec in `docs/specs/focus-mode.md`
  goes to the owner before code.
- Tests: unit tests for ordering and holding (fake agents, fake clock for the
  10-minute idle), one e2e flow: turn on Focus, get an approval, see the chip,
  answer it through "What next?".

## 8. Decisions for the owner

| #   | Question                                                             | Recommendation                                                                                                      |
| --- | -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| D1  | Do approvals wait silently in Focus, or break through?               | Wait silently, with the chip in `--warn` while an agent is blocked. Agents only lose time; nothing runs unapproved. |
| D2  | Name and place                                                       | "Focus", in View menu, palette and Settings → Accessibility, with a title-bar chip while on.                        |
| D3  | Should "Reduce motion in Ovra" also exist on its own, outside Focus? | Yes, a separate switch; it costs little and helps people who only want that.                                        |
| D4  | "Where you left off" after 10 minutes without input                  | Yes, on by default inside Focus; the idle time could later be a setting.                                            |
| D5  | Scope of the MVP                                                     | F1-F6 as above; F7-F11 after the owner tries the MVP.                                                               |

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
