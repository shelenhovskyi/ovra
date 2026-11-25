# First-run simplification plan (2026-11-25)

The owner opened Ovra on macOS for the first time and found it hard to
understand. This plan lists what a newcomer sees today and what changes, in
priority order. Security guarantees stay as they are: trust, approvals and
warnings about risky settings remain; only how they are presented changes.

## What a newcomer sees today

1. Six equal header buttons (Reviews, Audit log, Policies, Network, Servers,
   Browser) with no explanation. Most are advanced or only useful once an
   agent runs.
2. The right column stacks Approvals (empty, but takes space), the Claude
   setup card, the project trust card and the task form. The trust card,
   once opened, is a wall of `Bash(...)` / `mcp__...` rules, red and yellow
   warnings and repeated paragraphs, with "Trust project" at the bottom.
3. Two buttons, "Start task" and "Start", explained by one long paragraph
   that mentions git worktrees.
4. A saved path to `claude` that does not exist (`.../claude-work`) is only
   found out when a session fails, in a toast.
5. Empty main area: "Open a file from the tree on the left", no next step.
6. Footer says "Connected to ovra 0.1.0 (tauri)": developer information.
7. Signing in to the Claude desktop app does not sign in the `claude`
   command; nothing says so.

## Changes, by priority

### P0 - iteration 1

- **A. Header** (`ui/src/shell`, header buttons): keep Reviews visible (with
  its count). Move Audit log, Policies, Network, Servers and Browser into one
  "More" menu where every item has a one-line plain description. Shortcuts
  and palette commands keep working.
- **B. Claude setup and trust** (`ui/src/agents/setup`): trust card leads
  with a one-sentence summary ("This project has its own Claude settings: 9
  commands Claude may run without asking, 1 hook that runs code") and a
  single "Review…" action; full lists sit behind "Show details". Broad
  allow-rules and code-running hooks still get a visible warning line in the
  summary. Setup card explains that the desktop app or claude.ai sign-in does
  not count, and a saved path that leads nowhere offers "Find automatically".
- **C. Task form** (`ui/src/agents/AgentsPanel.svelte`): one "Start" button
  and a two-option choice in plain words: "Work here (one agent, in this
  folder)" or "Plan and split (up to 3 agents, each on its own copy; you
  review before anything is merged)". Only the chosen option's hint shows.
- **D. Core** (`crates/core`, `crates/agent-runtime`): `agents.set_path`
  refuses a path with no file (and says that aliases cannot be used); the
  session error names what to do.

### P1 - iteration 2

- **E. Right column order and empty states**: Approvals takes no space when
  empty; the column reads top to bottom as a first-run checklist (Claude
  ready, project settings, first task) until a session exists.
- **F. Main area empty state** with actions (open a file, search, give
  Claude a task) and the keys for them.
- **G. Plain words**: footer shows "Connected" (version in a tooltip);
  "session", "worktree", "core" and similar terms replaced in visible text.

### P2 - after each iteration

- Persona round (newcomer, agent-orchestrator, security-reviewer) on the
  first session; blocker and major findings fixed before the next iteration.

Out of scope here: choosing the project folder with a dialog (separate task).
