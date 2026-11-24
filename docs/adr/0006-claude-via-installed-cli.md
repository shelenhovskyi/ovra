# 0006. Run Claude through the user's installed `claude` CLI

- Status: accepted
- Date: 2026-11-24
- Supersedes: the sidecar part of ADR 0004 (for now)

## Context

ADR 0004 ran Claude through the Claude Agent SDK in a TypeScript sidecar.
The SDK terms say: "Unless previously approved, Anthropic does not allow third
party developers to offer claude.ai login or rate limits for their products,
including agents built on the Claude Agent SDK." So an SDK sidecar can only
use the user's API key or a cloud provider (Bedrock, Vertex, Foundry), and
most people would rather use their Claude subscription.

The Claude Code legal page allows "an end user signing in to the unmodified
Claude Code binary with their own Claude subscription", and forbids
developers to "collect, store, or intermediate Claude.ai credentials or
session tokens". The SDK itself is "a library that runs the Claude Code
binary": it drives `claude -p` over stdio.

A spike (2026-11-24, Claude Code 2.1.331) confirmed that the core can drive
the CLI directly, without the SDK:

- `claude -p --input-format stream-json --output-format stream-json --verbose`
  takes user messages and control requests on stdin and streams JSON lines
  (`system/init`, `assistant`, `user`, `stream_event` with
  `--include-partial-messages`, `result` with cost and usage).
- With `--permission-prompt-tool stdio` every tool call that needs approval
  arrives as a `control_request` (`subtype: can_use_tool`, tool name, input,
  suggested rules); the host answers `allow` (optionally with changed input)
  or `deny`.
- `--session-id <uuid>` and `--resume <id>` keep and continue sessions;
  `--fork-session` branches one.
- Subagent messages carry `parent_tool_use_id`; `--forward-subagent-text`
  adds their text.
- `--setting-sources`, `--settings`, `--mcp-config --strict-mcp-config`,
  `--agents`, `--permission-mode`, `--max-budget-usd`, `--model` cover the
  rest of what the sidecar would have set.

## Decision

- In v1, every Claude session is a `claude -p` process from the user's own
  installation, started by `crates/agent-runtime` in Rust and spoken to over
  stream-json on stdio. There is no `agent-host` sidecar and no bundled
  JavaScript runtime.
- Authentication is whatever the user set up in their own Claude Code
  (subscription login, API key or cloud provider). Ovra never reads, stores,
  passes or proxies credentials. It never uses `--bare` (which ignores the
  subscription login) and never changes `CLAUDE_CONFIG_DIR` (which would hide
  the login).
- The binary is run as published, unmodified. Ovra finds it on `PATH` (or a
  path the user sets), checks its version against a tested range and tells
  the user how to install, update or sign in when needed.
- Safe defaults, because `-p` skips Claude Code's own workspace trust dialog:
  - `--setting-sources user` until the user trusts the project in Ovra; only
    then `user,project,local`. A cloned repository's `.claude/` hooks and
    settings never run before that.
  - `--strict-mcp-config` with the MCP servers Ovra passes.
  - `--permission-prompt-tool stdio`: approvals go to the core, which applies
    project policies and asks the person through the approval UI. A
    `PreToolUse` hook enforces the policies that must also hold in modes
    that skip prompts.
  - A budget (`--max-budget-usd`) per task when the user sets one.
- Branding: Ovra says "Claude" or "Powered by Claude", never "Claude Code".
- `AgentRuntime` in the core stays the only seam. The Agent SDK sidecar of
  ADR 0004 (API key or cloud provider) can be added behind it later, for
  example after Anthropic approves subscription use or for team plans.
  Other agents still connect through ACP (task 2.G).

## Consequences

- The user must install Claude Code and sign in once. Ovra's onboarding checks
  for it.
- Usage counts against the user's own plan limits. Ovra can show them
  (`rate_limit_event`, `result.total_cost_usd`) but not raise them.
- The control messages (`initialize`, `can_use_tool`, `hook_callback`,
  `interrupt`) are the protocol between the SDK and the CLI. Only the flags
  are documented, the message shapes are not, so a Claude Code update can
  change them. Mitigations: a tested version range checked at start,
  contract tests on recorded sessions (fixtures from task 2.H), and a
  fallback through the documented `--permission-prompt-tool` MCP route
  (`mcp__ovra__approve`).
- The terms do not expressly cover a third-party app launching the user's
  own CLI. Before a public release, ask Anthropic to confirm.
- No Node packaging on three OSes; phase 2 gets shorter.
- Sandboxing (phase 4) wraps the `claude` process.
- `claude-agent-acp` is built on the SDK, so the same auth limits apply to
  it; it is not needed for Claude while the CLI path works.
