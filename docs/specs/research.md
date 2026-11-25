# Research sessions

Status: approved by the owner on 2026-11-25 (project chat), including the
network choice: Claude's own web search only, no page fetching through the
netproxy. Decision record: ADR 0011.

## Problem

Asking an agent to study the code or look something up should not take a
worktree, the direct-writer lock or one of the three working places, and
should not flood the person with approvals for a job that changes nothing.

## What the person sees

- The agents form has a third choice next to "One agent, here" and "Plan and
  split": **Research** - "Reads the project and searches the web. Changes
  nothing."
- The session's transcript is the report. Under the last answer of an idle
  research session:
  - **Save as doc** writes the answer to `docs/research/<date>-<title>.md`
    (a new file; a number is added if the name is taken) and opens it.
  - **Turn into task** switches the form to "Plan and split" with the
    report quoted in the prompt; the person adds what to do and starts it.
- The capacity line counts research separately: "1 of 2 researching".
- Settings > Claude: "Research agents at once" (0 to 5, default 2), with
  the note that research uses the same Claude plan.

## Core

- `SessionMode::Research` (`"research"`), started with `agents.start` by a
  person only; resumable like a direct session.
- No baseline, review item, direct lock or server forwarding. The sandbox
  profile is the project root with `work_dir_read_only: true` and the
  usual netproxy network.
- Cap: `slots::research_count` counts research sessions that are not ended
  and not idle; `agents.start` and `agents.resume` take a research place
  under the orchestration lock, like `reserve`. `AgentCapacity` gains
  `research_working` and `research_limit`.
- `AgentSettings::research_limit` (`None` = 2, from 0 to 5), next to
  `max_working`. `agents.research_limit` answers `AgentLimit`;
  `agents.set_research_limit { limit }` stores it, answers it and sends
  `agents.research_limit_changed`.
- Tools: `AgentHost::before_tool` and `AgentHost::ask` refuse every tool
  outside `RESEARCH_TOOLS` (`Read`, `Grep`, `Glob`, `WebSearch`) in a
  research session; `ask` allows `WebSearch` there unless a policy rule
  decides it.
- Runtime: `--tools Read,Grep,Glob,WebSearch`, `--setting-sources user`
  and `agents/roles/research.md` as the appended system prompt; no IDE
  tools.

## Tests

- Core: a research start takes no direct lock and no baseline, its cap is
  separate from the worker cap, Write and Bash are refused, WebSearch is
  allowed, a deny rule still wins, the sandbox profile is read-only.
- Runtime: the arguments of a research session.
- Sandbox: Linux binds the work dir read-only and makes no folders in it,
  also for a project inside the home folder; Windows refuses it for now.
- UI: the mode choice, Save as doc (create then write), Turn into task.
- e2e: start a research session on the fake agent and save its report.
