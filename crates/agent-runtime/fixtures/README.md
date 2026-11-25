# Recorded claude sessions

Contract tests in `src/tests.rs` replay these files against the driver. Each
line is one step:

- `{"cli": {...}}`: the CLI writes this line to stdout.
- `{"ovra": {...}}`: Ovra must write exactly this line to stdin next.
- `{"wait": {...}}`: wait for a core event that contains these fields.
- `{"core": {...}}`: a request to the core, as the UI would send it.

After the last step the CLI expects stdin to close with nothing more on it.
The session starts with the text of Ovra's first user line.

The files are real sessions of Claude Code 2.1.331, recorded by the live
suite (`just agents-live record`, `tests/agents/README.md`) and scrubbed by
`tests/agents/fixtures.ts`: the project is `/w`, its parent `/`, ids and
times are placeholders, and plan usage and the machine's own commands,
skills and plugins are left out. Do not edit them by hand; record again.

- `edit_allowed.jsonl`: an allowed Edit, a reply, an allowed Bash command, stop.
- `denied_outside_folder.jsonl`: a Write outside the project that the
  project's settings allow; Ovra's `PreToolUse` hook answers `ask`, the CLI
  sends `can_use_tool`, and the person denies it with a message.
- `interrupted.jsonl`: an allowed Bash command interrupted while it runs,
  then a reply in the same session.
- `subagent.jsonl`: a Task subagent; its messages carry `parent_tool_use_id`.

`src/bin/fake-claude.rs` (feature `fake-claude`, built by `just
build-headless`) plays a fixture as a stand-in `claude` executable, so e2e
tests (`tests/e2e/specs/claude-driver.spec.ts`) run the real driver on the
same files as the contract tests. `OVRA_FAKE_CLAUDE_FIXTURE` names one
fixture or this folder; from a folder each session plays the fixture whose
lines Ovra writes, which its prompt decides. The `wait` and `core` steps are
the e2e test's part (an approval, Interrupt, a reply). The
fixtures were recorded without `--include-partial-messages`, so it writes a
`stream_event` text delta per word before each `assistant` text of a
fixture that has none of its own.
