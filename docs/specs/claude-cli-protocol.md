# The claude CLI protocol Ovra uses

Status: reference for task 2.C. Observed on Claude Code 2.1.331 (2026-11-24).
Decision: ADR 0006.

Only the flags are documented by Anthropic. The control messages below are the
protocol between the Agent SDK and the CLI, so `crates/agent-runtime` checks
the CLI version at start and has contract tests on recorded sessions.

## Launch

```
claude -p
  --input-format stream-json --output-format stream-json --verbose
  --permission-prompt-tool stdio
  --permission-mode default
  --setting-sources user            # user,project,local once the project is trusted in Ovra
  --strict-mcp-config --mcp-config <json from Ovra>
  [--tools Read,Grep,Glob]           # a coordinator: no built-in tool that writes or runs
  --session-id <uuid>               # new session; or --resume <id> [--fork-session]
  [--model <alias>] [--max-budget-usd <n>]
```

- cwd: the project root (direct mode) or the worktree (isolated mode).
- Environment: the user's own, minus the nested-session markers listed in ADR
  0006 (`CLAUDECODE`, `CLAUDE_CODE_ENTRYPOINT`, ...). Never `--bare` (it ignores the
  subscription login) and never a different `CLAUDE_CONFIG_DIR` (it hides
  the login). Ovra never reads or passes credentials.
- `-p` skips Claude Code's workspace trust dialog, hence `--setting-sources
  user` until the person trusts the project in Ovra.
- `--include-partial-messages` is left out until the protocol has a message
  for partial text; `--model` and `--max-budget-usd` until a session carries
  them.
- Before starting, `claude --version` must be at least 2.1.331 and below
  3.0.0 (`crates/agent-runtime/src/launch.rs`).

## stdin (one JSON object per line)

| Message                        | Shape                                                                                                                                                                                                                                 |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| initialize (first)             | `{"type":"control_request","request_id":"init","request":{"subtype":"initialize","hooks":{"PreToolUse":[{"matcher":null,"hookCallbackIds":["ovra_policy"]}]}}}`; user messages wait for its answer                                    |
| user message                   | `{"type":"user","message":{"role":"user","content":"..."}}`                                                                                                                                                                           |
| answer to a hook callback      | `{"type":"control_response","response":{"subtype":"success","request_id":"<id>","response":{}}}` (no opinion), or `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"..."}}` |
| answer to anything else        | `{"type":"control_response","response":{"subtype":"error","request_id":"<id>","error":"..."}}`                                                                                                                                        |
| answer to a permission request | `{"type":"control_response","response":{"subtype":"success","request_id":"<id>","response":{"behavior":"allow","updatedInput":{...}}}}`, or `{"behavior":"deny","message":"..."}`                                                     |
| interrupt the turn             | `{"type":"control_request","request_id":"rN","request":{"subtype":"interrupt"}}`                                                                                                                                                      |

Closing stdin ends the session after the current turn.

## stdout (one JSON object per line)

| `type`                                      | Use in Ovra                                                                                                                                                                                                                          |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `control_response`                          | reply to our `initialize` (lists commands, agents, models)                                                                                                                                                                           |
| `system`, `subtype: init`                   | session id, cwd, tools, MCP servers, model; status `working`                                                                                                                                                                         |
| `stream_event`                              | partial content (`--include-partial-messages`): text and tool input deltas, for live rendering                                                                                                                                       |
| `assistant`                                 | complete message; `content[]` of `text`, `thinking`, `tool_use` → `AgentMessage::AssistantText` / `ToolUse`                                                                                                                          |
| `user`                                      | `content[]` of `tool_result` → `AgentMessage::ToolResult`                                                                                                                                                                            |
| `control_request`, `subtype: can_use_tool`  | `tool_name`, `input`, `description`, `permission_suggestions`, `request_id` → `AgentHost::ask`, then answer on stdin                                                                                                                 |
| `control_request`, `subtype: hook_callback` | `callback_id`, `input` (`tool_name`, `tool_input`, ...) → the `PreToolUse` policy (`src/policy.rs`), answered at once                                                                                                                |
| `control_request`, `subtype: mcp_message`   | `server_name`, `message` (JSON-RPC) for the in-process `ovra` server of a session with IDE tools → `AgentHost::call_ide`, answered with `{"mcp_response": <reply>}` (an empty result for a notification); other servers get an error |
| `control_cancel_request`                    | `request_id` of a `can_use_tool` the CLI no longer needs; Ovra stops waiting for it                                                                                                                                                  |
| `system`, `subtype: permission_denied`      | a call denied by rules or mode, without asking                                                                                                                                                                                       |
| `rate_limit_event`                          | plan usage and reset time, for the UI                                                                                                                                                                                                |
| `result`                                    | end of turn: `total_cost_usd` (the session's total; Ovra reports the difference per turn), `usage` (this turn), `session_id`, `stop_reason` → `AgentMessage::TurnEnd`, status `idle`                                                 |

Every message has `session_id`; messages from subagents carry
`parent_tool_use_id` (→ `parent` in `AgentMessage`). Unknown types and fields
are ignored, so new CLI versions add without breaking.

## Fallback

If the stdio control messages change in a way the version check misses,
approvals can go through the documented MCP route:
`--permission-prompt-tool mcp__ovra__approve` with an Ovra MCP server that
answers `{"behavior":"allow"|"deny",...}`.
