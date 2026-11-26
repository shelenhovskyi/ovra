# The generic ACP adapter

Status: approved 2026-11-26 with the recommended option of every decision
(the owner's 30-minute rule for unanswered reversible questions); step 7 of `docs/research/provider-agnostic.md`
(migration plan), under ADR 0011. The crate stub (`crates/acp-client`) is on
`main`.

## Problem

Ovra can start only Claude sessions. ADR 0011 adds every agent that speaks
ACP (Agent Client Protocol) through one adapter, with each agent described
by data. This spec covers that adapter up to the point where a fake ACP
agent runs a whole session in tests. Gemini CLI's descriptor (step 9) and the
IDE tools bridge (step 8) are separate specs.

## Goals

- `AcpAgents` starts an ACP agent as a child process through the sandbox,
  speaks ACP v1 over its stdio, and reports to the core through `AgentHost`
  as the Claude adapter does: status, messages, text deltas, tool calls,
  permission requests, the end of a turn and of the session.
- A shared fake ACP agent (a small binary in the crate, behind a
  `fake-acp` feature like `fake-claude`) plays scripted sessions for unit,
  integration and e2e tests. No real agent runs in tests.
- The core routes each session to the runtime of its provider, so Claude
  sessions do not change at all.

Not goals for v1: Ovra's IDE tools for ACP agents (step 8, so ACP agents
cannot be coordinators or workers yet), resume and fork, accounts other than
the default one, usage and cost, ACP v2, `fs/*` and `terminal/*` served by
Ovra.

## Decisions

1. **Own JSON-RPC module, no ACP SDK.** About 300 lines of serde types for
   the v1 messages Ovra uses, in one module (`v1.rs`), so v2 later is a
   second module. The `agent-client-protocol` crate (Apache-2.0) would pull
   an async runtime layer and types for the whole protocol; the CDP client
   in `crates/browser` set the same precedent. Recommended.
   Alternative: the SDK crate, less code now, its release pace to follow.
2. **A router comes with the second provider.** While `AgentKind` has one
   variant, nothing can be routed, so the router lands with step 9: a small
   `AgentRuntimes` value (in `ovra-core`, next to the trait) that holds one
   runtime per `AgentKind`, forwards each call by `spec.agent` and joins
   `providers()`; the daemon and app then build it from `ClaudeCli` and
   `AcpAgents` (new edges in `scripts/crate-graph.json`). Until then the
   adapter is tested inside its crate, its driver over in-memory pipes and
   its process layer against the fake agent.
3. **Permissions go through the core as Claude's do.**
   `session/request_permission` becomes a permission request with the
   call's neutral `ToolCall` (ACP sends `kind`, `locations` and the raw
   input). Ovra answers with `allow_once` or `reject_once` only;
   "Always allow" stays Ovra's own rule, never the agent's
   `allow_always`, so the decision stays in Ovra's policy file. Policies
   apply where the agent asks; without `enforced_policy` the provider runs
   only in isolated mode (ADR 0011), which the core already enforces.
4. **Sandbox and lists come from the descriptor.** The descriptor holds
   the program name, its ACP arguments (`--acp`), the state folders and
   files, the hosts and the environment variables (`ProviderLists`, step 5),
   so the sandbox plan, the netproxy base list and the environment allowlist
   work unchanged. No descriptor is listed in v1 except in tests, so the UI
   shows nothing new until step 9.
5. **The agent's own files stay its own.** `fs/*` and `terminal/*` are not
   offered in `initialize` (client capabilities false), so the agent reads,
   writes and runs commands itself, inside the sandbox, as Claude does.
6. **Turn and session ends.** `session/prompt`'s response ends a turn
   (`end_turn`, `cancelled`, `max_tokens`, `refusal` mapped to the same
   states as Claude's `result`); `interrupt` sends `session/cancel`; stop
   closes stdin, then kills after the grace period the Claude adapter uses.
   A broken pipe or an unparsable line ends the session as failed with the
   stderr tail, as for Claude.

## Tests

- Unit: v1 message round trips (insta snapshots), the update to `AgentHost`
  mapping, the stop reasons, a permission request answered and refused.
- Integration (crate seam): `AcpAgents` against the fake ACP agent through
  the fake sandbox: a turn with text, a tool call that asks and is allowed,
  one that is denied, an interrupt, an agent that crashes.
- Core (with step 9): the router sends each session to its provider's
  runtime and refuses an unknown provider.
- e2e: none in v1; they come with the first real provider (step 9).

## Size

Three or four commits of under ~400 lines each: v1 types with the session
driver over pipes, the process layer through the sandbox, the fake agent and
integration tests. The adapter starts processes through the sandbox, so its driver and
the router get the independent reviewer's verdict before they land.
