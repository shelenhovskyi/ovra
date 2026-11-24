# UX reviews by personas

Agents role-play the people Ovra is for, use the real app in headless mode and
report what is inconvenient, wrong or confusing. Findings go to the
coordinator, who turns them into tasks for the area that owns each one.
Reviewers never change feature code.

## When

- After each feature lands on `main` (a round on that feature).
- Before closing a phase (a full round, every persona, every feature).
- When the coordinator asks: re-check that earlier findings are fixed.

## Personas

One file each in `agents/personas/`: who they are, their goal, habits and what
they check. Current set: `newcomer`, `keyboard-power-user`,
`monorepo-maintainer`, `agent-orchestrator`, `accessibility-user`. Add one when
a new kind of user matters (for example a reviewer of agent diffs in phase 3).

## How a round runs

1. The coordinator starts one `ux-persona` subagent
   (`.claude/agents/ux-persona.md`) per persona, in parallel, naming the round
   folder and the features in scope.
2. Each agent builds headless mode once (`just build-headless`), writes small
   Playwright scripts that act out its persona and runs them with the harness:

   ```
   just ux path/to/script.ts --fixtures small,monorepo --seed small
   ```

   The script's default export receives a `Session` (`tests/e2e/ux/run.ts`):
   `page`, `open()`, `shot(name)`, `note(line)`, `roots` (fixture folders) and
   `core`. Screenshots and `session.log` (console, page errors, every frame)
   land in `target/ux/<timestamp>/`. Scripts are scratch work and are not
   committed; the report quotes the steps.
3. Each agent writes `docs/ux-reviews/<date>-<round>/<persona>.md` from the
   template below. The coordinator writes `summary.md` in the same folder:
   findings deduplicated, grouped by owning area, ranked.
4. Findings marked `blocker` or `major` become tasks for the owning thread.
   The next round re-checks them and marks them fixed or still open.

## Harness

`tests/e2e/ux/` runs the real UI against the real daemon. Parts of the core
that are still stubs on `main` (the project store, task 1.B) would stop a
review at the start screen, so `fake-core.ts` answers `projects.*`, `fs.*` and
`open.external` from real fixture folders (git repositories made by
`fixtures.ts`) and forwards everything else to the daemon. Pass `--real-core`
to use only the daemon; drop the stand-in once 1.B lands. Findings that come
from the stand-in itself are not findings.

## Severity

- **blocker** - the persona cannot finish the task, or loses data.
- **major** - finishes only with a workaround, or is misled in a way that costs time.
- **minor** - friction, inconsistency or a missing hint; the task still works.
- **polish** - wording, visuals, nice-to-have.

## Report template

```markdown
# <Persona name>: <round>

Build: <commit>, headless, Chromium <version>, <viewport>.
Scope: <features>. Fixtures: <names>.

## Summary

<Three sentences: could the persona do the job, the worst problems, overall feel.>

## Findings

### <ID> <severity>: <one-line problem>

- **Area:** <ui/src/tree | ui/src/palette | ui/src/keys | ui/src/shell | crates/... >
- **Steps:** 1. ... 2. ...
- **Expected:** ...
- **Actual:** ... (screenshot name or log line)
- **Why it matters for <persona>:** ...
- **Suggested fix:** ...

## What worked well

- ...
```

IDs are `<persona prefix>-<n>`: `NEW`, `KEY`, `MONO`, `ORCH`, `A11Y`.
