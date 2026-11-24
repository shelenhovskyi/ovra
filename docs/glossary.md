# Glossary

- **Core** - the Rust library (`ovra-core`) that owns all state and logic.
- **Front end** - anything that drives the core: the Tauri window, a browser in
  headless mode, or a test.
- **Frame** - one JSON message on a transport (request, result, error or event).
- **Headless mode** - core + UI served by `ovra-daemon` to a regular browser.
- **Coordinator** - the agent that splits a task and spawns subagents (phase 3).
- **Subagent** - an agent working in its own git worktree and sandbox.
- **Inbox** - the review queue where a person approves or rejects diffs.
- **ACP** - Agent Client Protocol, how agents are connected.
- **MCP** - Model Context Protocol, how the IDE exposes tools to agents.
