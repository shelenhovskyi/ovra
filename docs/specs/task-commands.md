# `/` commands, task templates and the model picker

Status: built 2026-11-27. The owner took it from the improvement list
(2026-11-27, item 10) with "take all"; like the other secondary features it
must not take room in the task form until the person asks for it.

## Problem

People repeat the same task texts ("Review the diff for bugs", "Write tests
for …") and the project already has commands and skills for its agents
(`.claude/commands`, `.claude/skills`), but the task box offers neither:
the person types them from memory. And every Claude session runs the
account's default model, although a quick question needs no Opus and a hard
one may.

## Design

1. **`/` at the start of a message box** (the new-task box and the reply
   box) opens a menu above the field, like the `@` menu. It filters as the
   person types (`/rev`), Up/Down move, Enter or Tab picks, Escape closes
   and leaves the text as typed. `/` anywhere else is plain text (paths,
   fractions), and so is a `/` after the menu was closed until the box is
   empty again.
2. **What it lists**, in this order:
   - the person's **templates**, by name; picking one replaces the box's
     text with the template's text;
   - the project's **commands and skills** from `catalog.list` that Claude
     loads in Ovra (`loads` is not `not_in_ovra`), by name with their
     description; picking one puts `/name ` in the box, which Claude runs
     as its own command. Ones that load only in a trusted project say so.
     Only offered while the box's agent is Claude.
   - **Save as template** while the box holds text other than the `/query`
     (offered only from the menu, so it costs no room).
3. **Templates** are the person's own: a name and a text, kept in the
   page's storage with the other agent preferences (`agentPrefs.templates`),
   for every project. Settings > Agents > Task templates lists them, renames,
   edits and deletes them, and adds new ones. No variables or per-project
   templates: project commands already cover what a team shares.
4. **Model picker, off by default.** Settings > Agents has "Choose the model
   for each task" (`agentPrefs.pickModel`). Only then, and only for Claude,
   the task form shows one more picker: "Default model", "Opus", "Sonnet",
   "Haiku", remembered per project. "Default model" sends nothing, so the
   account's own choice (`/model` in Claude, its settings) stays in charge.
5. **Core.** `agents.start` and `orchestration.start` take an optional
   `model` (`opus`, `sonnet` or `haiku`; nothing else parses). A session
   keeps it in `SessionInfo.model`, so a resume or a queued start uses it,
   and a coordinator's workers use their coordinator's. The runtime passes
   it to `claude` as `--model <alias>`. A model for another provider is
   refused (`bad_request`) until an ACP agent reports models.

## Not done

- Claude's built-in commands (`/compact`, `/context`): they belong to an
  interactive session, and the stream-json mode Ovra uses does not run them all.
- User-level commands and skills (`~/.claude`), which the catalog does not
  list yet.
- Models of Gemini and Codex, and full model ids instead of aliases.
