# 0011. Agents from more than one vendor behind one neutral model

- Status: accepted (owner, 2026-11-25: all six decisions as recommended)
- Date: 2026-11-25
- Extends: ADR 0006 (Claude stays as it is); revisits the ACP part of
  ADR 0004

## Context

Ovra runs one kind of agent: the user's installed `claude` CLI (ADR 0006).
The `AgentRuntime` trait was meant to hold other agents too, but the data
around it is Claude's: policies, approvals, audit, direct mode and the
transcript match Claude's tool names (`Bash`, `Edit`, `Write`); accounts
are Claude accounts with `CLAUDE_CONFIG_DIR`; the sandbox, netproxy,
secrets and environment allowlist hold Claude's folders, hosts and
variables; project trust reads only `.claude/`; Ovra's IDE tools are an
in-process server only the Claude CLI can host; the UI says "Claude" in
about 85 places. The audit is in `docs/research/provider-agnostic.md`.

Meanwhile ACP (Agent Client Protocol) became the common protocol of agent
CLIs: Gemini CLI, GitHub Copilot CLI, opencode, Goose and Cursor CLI speak
it natively, Codex through an adapter. It covers sessions, streamed
updates, tool calls with a neutral `kind`, permission requests and MCP
servers per session. It lacks a policy hook that holds in auto modes, cost
and billing source, tool restriction and in-process MCP. Running Claude
through ACP (`claude-agent-acp`) would lose those features and use the
Agent SDK, which ADR 0006 avoids.

## Decision

- **One neutral model in the core, adapters per protocol.** The core and UI
  know agents only as providers with capabilities; adapters translate.
  - The native Claude adapter (`ovra-agent-runtime`, ADR 0006) stays and
    keeps every feature it has.
  - A generic ACP adapter (new crate `ovra-acp-client`) serves every ACP
    agent. Each agent is a descriptor (data): id, display name, how to find,
    check and start the binary, how to check sign-in, its config-folder
    variable, the state folders and hosts it needs, its project config and
    rules files, known tool names, and its capabilities.
  - A native adapter for another agent is added only when ACP misses
    something essential for it (a candidate: `codex app-server`).
- **Providers.** `AgentKind` becomes an open provider id (`claude` for all
  existing sessions and settings). `agents.providers` lists the adapters'
  descriptors and capabilities: `approvals`, `enforced_policy`,
  `ide_tools`, `read_only_role`, `resume`, `fork`, `interrupt`,
  `text_deltas`, `subagents`, `usage`, `cost`, `billing`, `accounts`. The
  core refuses a role or request a provider cannot serve; the UI hides
  what it cannot do.
- **Neutral tools.** Tool calls and permission requests carry a `kind`
  (ACP's: read, edit, delete, move, search, execute, think, fetch, other)
  and normalized `paths`, `command`, `url`, `diff`, next to the raw name and
  input. Policies, direct mode, audit, approvals and the transcript match on
  them. Policy rules written in Claude's syntax keep working.
- **Accounts** belong to a provider (`Account {provider, id, name,
  config_dir}`), with the config-folder variable from the descriptor
  (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`). Every rule of ADR 0006 about
  credentials holds for every vendor: run the vendor's unmodified binary,
  sign-in only through the vendor's own flow, never read, store or pass on
  tokens, never imitate a vendor's client, never bundle or install an agent.
  Each new provider gets a terms check like
  `research/claude-cli-usage-policy.md` before it ships.
- **Sandbox, netproxy, secrets, environment** take the session's provider's
  lists (state folders, hosts, variables) instead of Claude constants. A
  session reaches only its own vendor's hosts.
- **IDE tools** reach non-Claude agents as a stdio MCP server: Ovra's own
  binary in a bridge mode, talking to the core over a per-session socket.
  The core decides by the caller's role as it does now.
- **Project trust and protected paths** cover every known agent's project
  config and rules files (`.codex`, `.gemini`, `GEMINI.md`,
  `.github/copilot-instructions.md`, `.cursor`, `opencode.json`, ...).
- **Rules files.** Ovra recommends `AGENTS.md` as the project's rules file,
  never writes rules files itself, and shows on the trust card which file
  each agent reads.
- **UI wording.** Generic places say "agent" ("New task", Settings >
  Agents); places about one session or setup use the provider's display
  name. Vendor facts (billing warnings, sign-in hints) live in that
  provider's texts. No vendor logos; no vendor product names as Ovra
  feature names.
- **Agents without `enforced_policy`** run only in isolated mode until the
  sandbox covers every OS; the UI says that the agent decides when to ask.
- First new provider: Gemini CLI through the generic ACP adapter; Codex
  next.

## Consequences

- Claude keeps working unchanged through every step; the refactors land
  first, each with snapshot tests that show Claude's command lines, sandbox
  plans and allowlists do not change.
- Hot spots change in their own small tasks first: the protocol
  (`agents.rs`, `agents.ts`), `services.rs` (`AgentSpec`, `SandboxProfile`),
  `scripts/crate-graph.json` (new `ovra-acp-client`).
- A new crate boundary (`ovra-acp-client`), an MCP surface reachable over a
  socket, and new sandbox rules need the reviews and owner decisions
  CLAUDE.md lists.
- Features only Claude reports (cost, billing source, subagents, the
  policy hook) become optional in the UI; a provider without them shows
  less, never wrong data.
- ACP v2 is a draft; the adapter starts on v1 and isolates the version in
  one module.
- Each provider adds a live suite (like `just agents-live`) and a fake for
  tests; the fake ACP agent is shared by all ACP providers.
- `docs/architecture.md`, the glossary and ADR 0006's "Other agents" line
  are updated when this is accepted.
