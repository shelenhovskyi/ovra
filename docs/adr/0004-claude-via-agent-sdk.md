# 0004. Run Claude through the Claude Agent SDK in a sidecar

- Status: accepted
- Date: 2026-11-24

## Context

The first plan connected every agent through ACP. For Claude, ACP exposes only
part of what Claude Code can do: no subagents, hooks, skills or full MCP
configuration, and no access to features such as artifacts. Ovra's value
depends on agents being as capable as in Claude Code itself.

## Decision

- Claude sessions run through the Claude Agent SDK
  (`@anthropic-ai/claude-agent-sdk`), which is Claude Code as a library.
- The SDK exists for TypeScript and Python only, so it runs in a sidecar,
  `agent-host/` (TypeScript), one process per session. The core starts and
  supervises it and talks to it over JSON lines on stdio.
- Permission prompts (`canUseTool`) and hooks are forwarded to the core, which
  applies project policies and asks the person through the approval UI.
- Ovra's own subagents are separate SDK sessions, each in its own git worktree
  (and, from phase 4, its own sandbox around the whole sidecar). The SDK's
  built-in subagents are allowed only inside one worktree.
- Other agents (Codex, Gemini CLI) connect through ACP; PTY + CLI is the
  fallback. All runtimes sit behind one `AgentRuntime` trait in the core.

## Consequences

- Ovra ships a JavaScript runtime with the sidecar (packaged as a single
  binary per OS, task 2.F).
- How the SDK authenticates inside Ovra, and what its terms allow for a
  third-party product, must be confirmed before task 2.C starts.
- Sandboxing wraps the sidecar process, because the SDK's tools run inside it.
