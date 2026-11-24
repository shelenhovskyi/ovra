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
- The environment is the user's own with one exception: when Ovra itself runs
  inside a Claude Code session (an IDE terminal, a tool call), the variables
  that session sets for its children would make our `claude` think it is
  nested in that session or attach to its IDE. Ovra removes exactly these:
  `CLAUDECODE`, `CLAUDE_CODE_ENTRYPOINT`, `CLAUDE_CODE_SSE_PORT`,
  `ENABLE_IDE_INTEGRATION`, `CLAUDE_CODE_SESSION_ID`,
  `CLAUDE_CODE_CHILD_SESSION`, `CLAUDE_CODE_EXECPATH`, `CLAUDE_PID` and
  `CLAUDE_AGENT_SDK_VERSION` (`crates/agent-runtime/src/launch.rs`). Every
  other variable stays, including the user's configuration (`ANTHROPIC_*`,
  `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_VERTEX`, proxies, CA bundles).
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
- Ovra never bundles, downloads or installs `claude`; the user installs it.
  Sign-in happens only in Claude Code's own flow (`claude` or
  `claude auth login` in a terminal); Ovra has no Claude sign-in of its own
  and never reads OAuth tokens (`~/.claude/.credentials.json`, the keychain,
  `CLAUDE_CODE_OAUTH_TOKEN`), calls Anthropic's API with them or imitates
  Claude Code's headers.
- Billing is visible: onboarding and each session show where usage is billed
  (subscription, API key or cloud provider, from `apiKeySource` in
  `system/init`), with a warning when `ANTHROPIC_API_KEY` is set, because it
  silently wins over the subscription. Parallel agents are capped (3 by
  default, configurable), and agents start only from a person's action.
- Branding: features and menus say "Claude", "Claude Agent" or "Powered by
  Claude"; "Claude Code" never appears in a product, feature or panel name,
  only as a plain factual description ("Install Claude Code", "Ovra runs
  Claude Code installed on your computer"). No Claude or Anthropic logos.
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
- Anthropic's terms allow an end user to sign in to the unmodified binary
  with their own subscription, including inside another product
  (`/mnt/project-files/ovra/research/claude-cli-usage-policy.md`, 2026-11-24).
  Optionally ask Anthropic to confirm before a public release, and check the
  legal page, the Agent SDK overview and support article 15036540 for every
  release.
- Pricing risk: Anthropic announced (13 May 2026) and paused (15 June 2026)
  a separate credit for `claude -p`. If it returns, `-p` sessions stop using
  subscription limits. Fallback, kept ready behind `AgentRuntime`: the
  interactive `claude` in an Ovra terminal (PTY), with fewer structured
  events.
- No Node packaging on three OSes; phase 2 gets shorter.
- Sandboxing (phase 4) wraps the `claude` process.
- `claude-agent-acp` is built on the SDK, so the same auth limits apply to
  it; it is not needed for Claude while the CLI path works.
