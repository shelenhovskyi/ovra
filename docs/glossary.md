# Glossary

- **Core** - the Rust library (`ovra-core`) that owns all state and logic.
- **Front end** - anything that drives the core: the Tauri window, a browser in
  headless mode, or a test.
- **Frame** - one JSON message on a transport (request, result, error or event).
- **Headless mode** - core + UI served by `ovra-daemon` to a regular browser.
- **Coordinator** - the agent that splits a task and spawns subagents (phase 3).
- **Subagent** - an agent working in its own git worktree and sandbox.
- **Direct mode** - one agent works in the project's main checkout; its
  changes are reviewed as a diff against a baseline snapshot (ADR 0005).
- **Isolated mode** - an agent works in its own git worktree.
- **Inbox** - the review queue (Reviews in the UI) where a person keeps or
  discards diffs.
- **Keep / Discard** - what a person does with a review item. The UI never
  says Approve or Reject for reviews; the protocol still names them
  `review.approve` and `review.reject`.
- **Service** - an OS-facing capability behind a trait in `ovra-core`
  (`FsService`, `ProjectStore`, `Opener`), implemented in its own crate.
- **claude CLI** - the user's installed Claude Code; v1 runs each Claude
  session as `claude -p` over stream-json (ADR 0006).
- **Claude Agent SDK** - Claude Code as a library; a possible later runtime
  behind `AgentRuntime` (ADR 0004).
- **ACP** - Agent Client Protocol, how other agents (Codex, Gemini CLI) connect.
- **MCP** - Model Context Protocol, how the IDE exposes tools to agents.
- **Trusted project** - a project whose own Claude configuration (`.claude/`
  settings, hooks, agents, commands, skills) the user let load; untrusted
  projects run `claude` with `--setting-sources user` (ADR 0006).
- **Language server** - a process that answers editor questions about one
  language (LSP); runs only for trusted projects, in the sandbox (ADR 0007).
