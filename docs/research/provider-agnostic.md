# One IDE, many agents: making Ovra provider-agnostic

Status: research, 2026-11-25. The owner accepted the proposal and all six
decisions in section 6 as recommended (same day); it is ADR 0011.
Builds on ADR 0004, ADR 0006, `docs/specs/claude-cli-protocol.md` and
`docs/specs/orchestration.md`.

The owner noticed that Ovra is tied closely to Claude and asked to unify it.
This page lists where the code, protocol, UI, sandbox and settings assume
Claude (section 1), what other agent CLIs and ACP offer (section 2), and a
proposal with a migration plan (sections 3 to 5).

## Summary

- The seam is already in the right place: everything that starts and talks
  to an agent goes through `AgentRuntime` / `RunningAgent`
  (`crates/core/src/services.rs`), and `crates/agent-runtime` is the only
  crate that speaks stream-json. The coupling is in the **data** around that
  seam: tool names (`Bash`, `Edit`, `Write`) in policies, approvals, audit,
  direct mode and the UI; Claude accounts and `CLAUDE_CONFIG_DIR`; `claude`'s
  state folders, variables and hosts in the sandbox, netproxy and secrets;
  `.claude/` in project trust; and about 85 lines of user-visible "Claude" text in the UI.
- ACP (Agent Client Protocol) is now the common denominator: Gemini CLI,
  GitHub Copilot CLI, opencode, Goose and Cursor CLI speak it natively,
  Codex through Zed's `codex-acp` adapter; 25+ agents in total. It covers
  sessions, streamed updates, tool calls with a `kind`, permission requests
  and MCP servers passed per session. It does not cover a policy hook that
  holds in auto modes, cost, billing source, tool restriction, or an
  in-process MCP server.
- Recommendation: **a neutral core model plus adapters** (option C below).
  Keep the native Claude adapter (ADR 0006 stays), add one generic ACP
  adapter that serves every ACP agent from a small per-agent descriptor, and
  add a native adapter only where ACP misses something essential. Capability
  flags tell the core and UI what each agent can do.
- First second provider: **Gemini CLI through the generic ACP adapter**
  (native `--acp`, the reference ACP implementation, a free tier that makes
  live tests cheap). Codex next, because most people who would want a
  second agent have a ChatGPT plan; it may need a native `codex app-server`
  adapter for full approvals.

## 1. Audit: where Ovra assumes Claude

Legend: **seam** = already behind an abstraction, only the Claude
implementation exists; **leak** = Claude assumptions in shared code.

### 1.1 Runtime and service traits

| Where                                        | What                                                                                                                                 | State                                                                                             |
| -------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------- |
| `services.rs` `AgentRuntime`, `RunningAgent` | `start`, `setup`, `project_config`, `config_dirs`; `send`, `interrupt`, `stop`                                                       | seam; one runtime "serves every session of the agents it supports"                                |
| `services.rs` `AgentSpec`                    | `agent: AgentKind`, `executable`, `config_dir` ("given to `claude` as `CLAUDE_CONFIG_DIR`"), `tools: ToolRole`, `trusted`, `resume`  | mostly seam; `config_dir` and its docs are Claude's                                               |
| `crates/agent-runtime` (`ClaudeCli`)         | `launch.rs` flags, `wire.rs` stream-json, `driver.rs`, `detect.rs` (`claude auth status`), `env.rs`, `project.rs` (`.claude/` trust) | seam; all of it is the Claude adapter and can stay so                                             |
| `StoredSession.agent_session`                | "Claude Code's session id"                                                                                                           | seam; any agent has a session id (ACP `sessionId`, Codex thread id)                               |
| IDE tools (`crates/mcp-server`, `launch.rs`) | served **in-process** over stream-json (`"type":"sdk"` server, `mcp_message` control requests)                                       | leak; only the Claude CLI can host an SDK MCP server. Other agents need a real MCP endpoint       |
| `orchestration/policy.rs` `IDE_TOOL_PREFIX`  | `mcp__ovra__` ("how Claude Code names the tools")                                                                                    | leak; naming differs per agent                                                                    |
| Coordinator role                             | `--tools Read,Grep,Glob` makes it read-only                                                                                          | leak; ACP has no tool restriction, other agents need a mode, policy denies or a read-only sandbox |

### 1.2 Protocol (`crates/core/src/protocol/agents.rs`, `ui/src/lib/protocol/agents.ts`)

- `AgentKind` has one variant, `Claude`; the UI type is `"claude"`. It is
  hard-coded at every start (`orchestration/tasks.rs` twice,
  `store/src/sessions.rs`). The request already carries `agents.setup {agent}`.
- `AgentSettings.claude_path`, `accounts: Vec<ClaudeAccount>`,
  `project_accounts`: accounts are Claude accounts by type and by name
  (`agents.accounts`, `agents.save_account`, `SessionInfo.account`).
- `Billing` (`Subscription` = "the user's Claude plan", `ApiKey`, `Cloud`)
  and `AgentSetup.api_key_in_env` (`ANTHROPIC_API_KEY`): the shape is
  general, the meaning is Claude's.
- `ConfigKind` (hooks, helpers, plugins, env, allow rules, agents, commands,
  skills) describes `.claude/settings.json`.
- `AgentMessage` is already neutral (user text, assistant text, tool use,
  tool result, turn end). `ToolUse` carries Claude's tool name and raw input.

### 1.3 Tool names in shared code (the biggest leak)

Claude's tool names and input fields (`Bash.command`, `Edit.file_path`,
`Write.content`, `WebFetch.url`, `Task`/`Agent`) are matched in:

- `crates/core/src/policy.rs`: policy rules are Claude permission rules
  (`Bash(git *)`, `Edit(src/**)`, `WebFetch(domain:...)`).
- `crates/core/src/orchestration/direct.rs`: direct-mode writer detection.
- `crates/core/src/audit/input.rs`: which tool names and fields the audit
  log keeps.
- `ui/src/approvals/describe.ts`, `body.ts`, `Approvals.svelte`: how an
  approval reads ("Run `git status`", "Edit src/a.rs" with a diff).
- `ui/src/agents/steps.ts`: transcript one-liners ("Wrote", "Writing").
- `crates/agent-runtime/src/policy.rs`: the `PreToolUse` hook.

Every other agent names tools differently (Codex: `shell`, `apply_patch`;
Gemini: `run_shell_command`, `replace`, `write_file`; ACP: free-form title
plus a `kind`). Without a neutral tool model, a second agent gets generic
approvals, no policies and no direct-mode tracking.

### 1.4 Sandbox, netproxy, secrets, environment

- `SandboxProfile.claude_dir` and `ProfileKind::Agent` ("`claude`'s
  settings, login and transcripts come back"); `crates/sandbox-linux/src/plan.rs`
  `CLAUDE_PRIVATE` (14 folders under `~/.claude`), `CLAUDE_FILES`
  (`.credentials.json`, `history.jsonl`), `holds_claude_state`,
  `claude_back`; the Windows plan has the same (44 mentions).
- Environment allowlist (`plan.rs`, `agent-runtime/src/env.rs`):
  `CLAUDE_CODE_USE_BEDROCK/VERTEX/FOUNDRY`, `ANTHROPIC_*`, plus the nested
  session variables of ADR 0006.
- `crates/netproxy/src/base.rs`: the base allowlist is Anthropic's hosts and
  the Bedrock/Vertex/Foundry gateways from `ANTHROPIC_*` variables.
- `crates/core/src/secrets.rs` `CLAUDE_DOMAINS`: hosts a secret is never
  added to.
- All of this is shared code with Claude constants in it: **leak**, but
  each one is a list that a per-agent descriptor can supply.

### 1.5 Protected paths and rules files (`crates/core/src/protected.rs`)

- Read-only in every sandbox: `.claude`, `.mcp.json`, `**/CLAUDE.md`,
  `**/CLAUDE.local.md`, `**/AGENTS.md`, editor folders, `.ovra`, ...
- Only marked in review: `.zed`, `.cursor`, `.gemini`, `**/GEMINI.md`.
  (Done 2026-11-25, step 2: other agents' config folders are now protected,
  their instruction files marked; see `docs/specs/security.md`.)
- Missing entirely: `.codex/` (Codex project config), `.github/copilot-instructions.md`,
  `.github/instructions/`, `.cursor/rules`, `opencode.json`, `.goosehints`,
  `.aider.conf.yml`.
- Project trust (`agent-runtime/src/project.rs`, `ProjectTrust.svelte`)
  reviews and fingerprints `.claude/settings*.json` and `.claude/agents|commands|skills`
  only; another agent's project config would load unreviewed.

### 1.6 UI and texts

- About 85 lines with user-visible "Claude" text in `ui/src` (comments and
  tests not counted): the start form
  ("Give Claude a task", "Reply to Claude", "Claude is working..."), the
  billing button ("Claude · plan"), the setup card (`ClaudeSetup.svelte`,
  `texts.ts`: "Claude is not installed", "Sign in to Claude"), accounts
  (`Accounts.svelte`, `account-texts.ts`), trust ("This project has its own
  Claude settings"), the transcript speaker label, palette commands
  ("New task for Claude", category "Claude"), Settings > Claude
  (`ClaudePane.svelte`) and the start screen's "Claude" column.
- Component and file names: `ClaudeSetup`, `ClaudePane`,
  `claude-accounts.svelte.ts`.
- Some texts must stay Claude's because they are facts about Claude:
  billing warnings about `ANTHROPIC_API_KEY`, "Signing in to claude.ai does
  not sign in the claude program". They move into the Claude adapter's
  descriptor, not into neutral wording.

### 1.7 Tests, fixtures, docs

- `crates/agent-runtime/src/bin/fake-claude.rs` and `fixtures/*.jsonl`
  (recorded stream-json), `OVRA_FAKE_CLAUDE_FIXTURE`, `just agents-live`,
  e2e `claude-driver.spec`. Fine as the Claude adapter's tests; a second
  agent needs its own fake (a fake ACP agent).
- `ScriptedAgent` in `crates/core/src/fakes.rs` is already agent-neutral.
- Docs: architecture "Agents" section, glossary ("Trusted project" = Claude
  configuration), ADR 0006, `claude-cli-protocol.md`, security spec.

### 1.8 What is already neutral

Review, worktrees, checkpoints (git snapshots), dev servers, the agent
browser, PR flow, LSP, file tree, search, terminals, `SessionMode`, status,
capacity and limits, the transcript message enum, `ScriptedAgent`, the
approval queue itself (`PermissionRequest` holds a tool name and input).

## 2. Other agent CLIs and protocols

Facts gathered 2026-11-25 from vendor docs and changelogs (sources at the
end). Versions move fast; each row needs a spike against the installed
binary before an adapter is written, as ADR 0006 did for Claude.

### 2.1 ACP (Agent Client Protocol)

- JSON-RPC 2.0 over stdio, the client (Ovra) starts the agent as a child.
  Created by Zed (2025), now its own project (`agentclientprotocol.com`),
  Apache-2.0 SDKs in Rust (`agent-client-protocol` crate), TypeScript, Go,
  Python. A registry launched 2026-01-28.
- v1 (stable): `initialize` (capabilities both ways), `authenticate`,
  `session/new {cwd, mcpServers}`, `session/load`, `session/prompt` (pending
  until the turn ends, returns a stop reason), `session/cancel`,
  `session/set_mode`; agent to client: `session/update` notifications (message
  chunks, thought chunks, `tool_call` / `tool_call_update` with `kind`
  read/edit/delete/move/search/execute/think/fetch/other, `locations`, diffs,
  plans, available commands, mode changes), `session/request_permission`
  (options `allow_once`, `allow_always`, `reject_once`, `reject_always`),
  optional `fs/read_text_file`, `fs/write_text_file`, `terminal/*`.
- v2 (draft, Nov 2026): `session/prompt` returns as soon as the message is
  accepted, the turn's end comes as an idle update; required
  `session/list|resume|close`; `session/load` removed; `fs/*` and
  `terminal/*` removed from the client surface; `set_mode` replaced by
  `session/set_config_option`; v1 and v2 do not mix on one connection.
- MCP: the client passes MCP servers per session (stdio command, HTTP or
  SSE). There is no in-process server; Ovra's IDE tools would be a stdio or
  HTTP MCP server the agent connects to.
- Missing for Ovra: a policy hook that holds in auto modes (the agent alone
  decides when to ask), cost and billing source (some agents send usage
  only), tool restriction for a read-only coordinator, subagent attribution.

Who speaks it (2026-11): Gemini CLI (`gemini --acp`, native, the reference
implementation), GitHub Copilot CLI (`copilot --acp`, public preview since
2026-01-28), opencode (`opencode acp`), Goose (`goose acp`), Cursor CLI
(`agent acp`), Codex (adapter `codex-acp`, Apache-2.0, by Zed), Claude
(adapter `claude-agent-acp`, built on the Agent SDK, so ADR 0006's auth
limits apply), plus Kimi, Qwen Code, Pi, Poolside and others in the
registry. Clients: Zed, JetBrains, Neovim plugins, Obsidian, `acpx`.

### 2.2 Agent CLIs

| Agent              | Headless / events                                                                                                                                                                           | Approvals                                                                                                                     | MCP                                                          | Rules file                                                                               | Sign-in                                                                                   | Own sandbox                     |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ | ---------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ------------------------------- |
| Claude Code        | `claude -p` stream-json in/out, partial messages, control requests (today's adapter)                                                                                                        | `can_use_tool` per call, `PreToolUse` hook holds in every mode                                                                | `--mcp-config`, in-process SDK servers                       | `CLAUDE.md`; `AGENTS.md` as fallback when no `CLAUDE.md` (reported from 2.1.277, verify) | claude.ai plan, API key, Bedrock/Vertex/Foundry; `CLAUDE_CONFIG_DIR` per account          | optional (`/sandbox`)           |
| OpenAI Codex CLI   | `codex exec --json` (JSONL events, one-shot); `codex app-server` (JSON-RPC over stdio: `thread/start`, `turn/start`, item notifications, schemas generated per version); `codex mcp-server` | app-server sends `item/commandExecution/requestApproval` and `item/fileChange/requestApproval`; `--ask-for-approval` policies | `config.toml` `[mcp_servers]`, `-c` overrides                | `AGENTS.md` (+ `AGENTS.override.md`)                                                     | ChatGPT plan (default) or API key; `CODEX_HOME` moves the config folder                   | yes: Seatbelt, Landlock/seccomp |
| Gemini CLI         | `gemini -p --output-format stream-json` (init, message, tool_use, tool_result, error, result); `gemini --acp`                                                                               | ACP permission requests; modes default / auto_edit / yolo / plan (read-only); trusted folders                                 | `settings.json` `mcpServers`; ACP per session                | `GEMINI.md`, configurable (`context.fileName`, e.g. `AGENTS.md`)                         | Google account (free tier), Gemini API key, Vertex AI                                     | optional (Docker/Seatbelt)      |
| GitHub Copilot CLI | `copilot -p` prompt mode; `copilot --acp` (stdio or `--port`)                                                                                                                               | `--allow-tool` / `--deny-tool`, ACP permission requests                                                                       | `~/.copilot/mcp-config.json`, `--additional-mcp-config`, ACP | `AGENTS.md`, `.github/copilot-instructions.md`                                           | GitHub account with a Copilot plan; `GH_TOKEN`; BYOK providers via `COPILOT_PROVIDER_*`   | no                              |
| opencode           | `opencode run`; `opencode serve` (HTTP + OpenAPI); `opencode acp`                                                                                                                           | per-tool `permission` config (ask/allow/deny), ACP permission requests                                                        | `opencode.json` `mcp`; ACP                                   | `AGENTS.md`                                                                              | many providers (`opencode auth login`); its Claude-subscription route was blocked in 2026 | no                              |
| Goose              | `goose run`; `goose serve`; `goose acp`                                                                                                                                                     | modes (auto, approve, smart approve, chat)                                                                                    | extensions are MCP servers                                   | `.goosehints`, `AGENTS.md`                                                               | any configured provider (API keys, local models)                                          | no                              |
| Cursor CLI         | `agent -p --output-format stream-json` (NDJSON, schema undocumented, closed source); `agent acp`                                                                                            | `--force` or ACP permission requests                                                                                          | `.cursor/mcp.json`; ACP                                      | `.cursor/rules`, `AGENTS.md`                                                             | Cursor account (`agent login`), `CURSOR_API_KEY`                                          | no                              |
| Aider              | `aider --message ... --yes-always`: plain text, no event stream                                                                                                                             | none to answer (auto-yes or interactive)                                                                                      | no                                                           | `CONVENTIONS.md` via `--read`                                                            | API keys of any provider                                                                  | no                              |

Takeaways:

- Six of eight speak ACP natively or through a maintained adapter. Only
  Aider has no structured channel; it would be a PTY-only agent (a terminal
  with review afterwards), like ADR 0006's fallback for Claude.
- `AGENTS.md` is the shared rules file; only Claude (`CLAUDE.md` first) and
  Gemini (`GEMINI.md` by default) prefer their own.
- Accounts generalize: Claude has `CLAUDE_CONFIG_DIR`, Codex `CODEX_HOME`;
  others keep one login per OS user (Gemini and Copilot need a check).
- Terms: each vendor's rules for running its CLI inside another product
  need the same check `research/claude-cli-usage-policy.md` did for Claude
  (run the unmodified binary, the user signs in through the vendor's own
  flow, Ovra never touches tokens). Anthropic blocked third-party tools in
  2026 that reused its OAuth tokens directly; running the vendor's own
  binary is the pattern that stayed allowed.

## 3. Options

### A. A provider trait with a native adapter per CLI

Every agent gets its own adapter speaking its own headless protocol
(stream-json, `codex app-server`, Gemini stream-json, ...).

- Plus: full fidelity per agent (Codex's approvals and sandbox, Claude's
  hooks).
- Minus: N undocumented protocols to keep up with; most headless JSON modes
  are one-shot or have no approvals; cost grows linearly with agents.

### B. ACP as the only transport, Claude included

Claude through `claude-agent-acp`, everyone else natively.

- Plus: one protocol, the whole registry for free.
- Minus: Claude would lose the policy hook, cost, billing source, subagent
  attribution, in-process IDE tools and tool restriction (ADR 0004's
  original objection); `claude-agent-acp` runs on the Agent SDK, which ADR
  0006 avoids for subscription sign-in; ACP v2 is still a draft. A step back
  for the agent most people use in Ovra today.

### C. Neutral core model + adapters (recommended)

The core speaks one neutral model; adapters translate.

- `ovra-agent-runtime` keeps the native Claude adapter unchanged in
  behaviour (ADR 0006 stays).
- A new `ovra-acp-client` crate is one generic ACP adapter. Each ACP agent
  is a small **descriptor** (data, not code): id, display name, how to find
  and check the binary, launch arguments, sign-in check, config-folder
  variable, state folders, hosts, rules and config files, known tool-name
  mapping, capabilities.
- A native adapter is added only when ACP lacks something that matters for
  an agent people use (likely Codex's `app-server` for its approval detail
  and sandbox integration).
- PTY-only agents (Aider, or Claude's interactive fallback) are a later
  third kind: a terminal plus a review of the diff afterwards.

This keeps today's quality for Claude, gets most agents through one code
path, and leaves room for native adapters where they pay off. It is also
what the plan already said (`architecture.md`: "Other agents connect through
ACP, with PTY + CLI as a fallback... All of them sit behind one
`AgentRuntime` trait"); the work is to make the data around the trait
neutral.

## 4. Proposed design (option C)

### 4.1 Providers and capabilities

- `AgentKind` becomes an open provider id (`"claude"`, `"gemini"`,
  `"codex"`), and `agents.providers` lists the installed adapters' descriptors
  with their capabilities. Stored sessions keep `"claude"`, so nothing
  migrates.
- Capability flags, decided per provider and reported in `agents.providers`:

| Flag              | Meaning                                                             | Claude                    | Generic ACP                             |
| ----------------- | ------------------------------------------------------------------- | ------------------------- | --------------------------------------- |
| `approvals`       | asks before each risky tool call                                    | yes                       | yes (`session/request_permission`)      |
| `enforced_policy` | a hook applies Ovra's policy even in auto modes                     | yes                       | no: sandbox and review only             |
| `ide_tools`       | can reach Ovra's MCP tools (coordinator, workers, servers, browser) | yes                       | yes, through the MCP bridge (4.4)       |
| `read_only_role`  | can run as a read-only coordinator                                  | yes (`--tools`)           | if it has a plan/read-only mode         |
| `resume`, `fork`  | continues or branches a past session                                | yes                       | v1 `session/load`, v2 `session/resume`  |
| `interrupt`       | stops a turn and stays alive                                        | yes                       | yes (`session/cancel`)                  |
| `text_deltas`     | streams partial text                                                | yes                       | yes (message chunks)                    |
| `subagents`       | reports nested agents' messages                                     | yes                       | no                                      |
| `usage`, `cost`   | reports tokens / money                                              | yes                       | per agent                               |
| `billing`         | says who pays                                                       | yes                       | no: shown as the provider's own sign-in |
| `accounts`        | one config folder per account                                       | yes (`CLAUDE_CONFIG_DIR`) | per descriptor (`CODEX_HOME`)           |

- The core refuses what a provider cannot do (no coordinator role without
  `ide_tools` and `read_only_role`), and the UI hides controls instead of
  showing ones that fail ("Go back" stays, it is git-based; "Resume" hides
  without `resume`).

### 4.2 A neutral tool model

- `ToolUse` and `PermissionRequest` gain a neutral `kind` (ACP's list:
  read, edit, delete, move, search, execute, think, fetch, other) and
  normalized fields: `paths`, `command`, `url`, `diff`. The raw name and
  input stay for display and audit.
- The Claude adapter maps its tools (`Bash` to execute, `Edit`/`Write`/
  `MultiEdit` to edit, `WebFetch` to fetch, `Task`/`Agent` to subagent),
  ACP agents send `kind` themselves, and a descriptor can map known names.
- Policies, direct-mode writer detection, audit field selection, approval
  texts and transcript one-liners match on `kind` and the normalized fields.
  Existing policies in Claude's rule syntax keep working: the parser turns
  `Bash(...)` into an execute rule, `Edit(...)` into an edit rule.

### 4.3 Accounts, sandbox, netproxy, secrets

- `ClaudeAccount` becomes `Account {provider, id, name, config_dir}`;
  `claude_path` becomes a path per provider. Old settings load as Claude's.
- `SandboxProfile.claude_dir` becomes `agent_state`: the folders and files
  the session's agent needs back (read-only, read-write, rewritten files),
  from the descriptor. The Linux and Windows plans consume lists instead of
  `CLAUDE_PRIVATE` constants. Pods and the browser still get none.
- The netproxy base allowlist, the environment allowlist and the secrets'
  never-decrypt hosts come from the session's provider, so a Gemini session
  cannot reach Anthropic and the other way round (a tightening).

### 4.4 Ovra's IDE tools for every agent

- The in-process SDK server stays for Claude. Other agents get the same
  tools as a stdio MCP server: a small bridge mode of Ovra's own binary
  (`ovra-daemon mcp-bridge <session>`) that the agent starts and that talks
  to the core over a per-session socket in the session's folder, with a
  per-session token. The core still decides by the caller's role, exactly as
  `orchestration/policy.rs` does today.
- This is an MCP and security change (a new socket the sandbox lets the
  agent reach) and needs the human review CLAUDE.md requires for
  `tools.json` and security code.

### 4.5 Project config, trust and rules files

- Trust review becomes a list of per-provider "project config" readers: the
  Claude one exists; descriptors name others (`.gemini/settings.json`,
  `.codex/config.toml`, `opencode.json`, `.cursor/`). The fingerprint covers
  all of them, so changing any agent's config asks again.
- Protected paths grow to every known agent's config and rules files:
  `.codex`, `.gemini`, `GEMINI.md`, `.github/copilot-instructions.md`,
  `.github/instructions`, `.cursor`, `opencode.json`, `.goosehints`
  (a tightening; may land on its own first).
- Rules files: Ovra recommends `AGENTS.md` as the project's one rules file
  and never writes rules files on its own. Where the project has only
  `AGENTS.md`, Claude reads it as a fallback in recent versions; where it
  has both, the trust card says which file each agent reads. A "make
  `CLAUDE.md` point to `AGENTS.md`" helper (`@AGENTS.md`) is a later UX
  option, not part of this change.

### 4.6 UI wording

- Generic places say "agent": "New task", "Reply", "Working...", Settings >
  **Agents** (one card per provider), the palette category "Agents".
- Places about one session or setup say the provider's display name from
  its descriptor: "Claude is working...", "Gemini is signed out",
  "Claude · plan". With one provider installed, the UI reads as it does now.
- Facts about one vendor (API-key billing warnings, sign-in hints) move
  into that provider's texts. ADR 0006's branding rules stay for Claude, and
  the same rule applies to every vendor: no logos, no vendor product names
  as Ovra feature names.
- The start form gets an agent picker next to the account picker, shown
  only when more than one provider is ready; the project remembers the last
  one.

### 4.7 Orchestration

- A coordinator needs `ide_tools` and `read_only_role`; workers need
  `ide_tools`. Mixed tasks (a Claude coordinator with Gemini workers) work
  because the IDE tools are the same; v1 of the change keeps workers on the
  coordinator's provider, and `spawn_agent` may take a provider later.
- The cap on parallel agents stays one cap for all providers.

## 5. Migration plan

Each step is small, lands on `main` on its own and keeps Claude working.
Shared hot spots go first, as CLAUDE.md asks.

1. **Accept ADR 0011** (owner), rename nothing yet.
2. **Protected paths for other agents** (tightening, `protected.rs` +
   sandbox plans). Can land now, independent of the rest.
3. **Protocol: providers** (hot spot task): open `AgentKind` id,
   `agents.providers` with capabilities, `Account.provider`, per-provider
   paths; serde keeps old settings and sessions. UI types in step.
4. **Neutral tool model** (core + Claude adapter + UI approvals/steps):
   `kind` and normalized fields; policy, direct mode and audit switch to
   them; snapshot tests for the Claude mapping.
5. **Provider descriptors for sandbox, netproxy, env, secrets**: replace the
   Claude constants with lists from the descriptor; Claude's descriptor
   reproduces today's lists exactly (snapshot tests prove no change).
6. **UI wording pass**: Settings > Agents, provider names from descriptors,
   rename `ClaudeSetup`/`ClaudePane` to provider-generic components with
   Claude's texts as data. Run `just ux` personas afterwards.
7. **`ovra-acp-client` stub** (crate graph edge, trait impl, fake ACP agent,
   one test) on `main`, then the generic ACP adapter against the fake.
8. **MCP bridge for IDE tools** (needs security review).
9. **Gemini CLI descriptor**: setup check, sign-in check, state folders,
   hosts, `.gemini/` trust reader; spike first on the installed `gemini`;
   live suite like `just agents-live`; a terms check like the Claude one.
10. **Codex**: spike `codex-acp` versus `codex app-server`; descriptor or a
    native adapter; `CODEX_HOME` accounts; terms check.
11. Later: more descriptors (Copilot, opencode, Goose, Cursor), PTY-only
    agents, provider choice per worker.

Steps 2 to 6 are refactors with no new agent and can run as parallel
threads once step 3 lands (4, 5 and 6 own different areas). Steps 7 to 9 add
the first new agent. Rough size: 2 to 6 about a day each; 7 to 9 about three
days.

## 6. Decisions for the owner

1. Option C (neutral model + native Claude + generic ACP + native where
   needed) instead of A or B. Recommended: C.
2. First new agent: Gemini CLI (native ACP, free tier, cheap live tests) or
   Codex (more demand, needs an adapter or `app-server`). Recommended:
   Gemini first to prove the generic path, Codex right after.
3. UI wording: "agent" in generic places, the provider's name where it is
   about one agent, Settings > Claude becomes Settings > Agents.
   Recommended: yes.
4. Rules files: recommend `AGENTS.md`, never write rules files, show which
   file each agent reads. Recommended: yes.
5. IDE tools for non-Claude agents through a stdio MCP bridge and a
   per-session socket (security review). Recommended: yes, as step 8.
6. Agents without a policy hook (`enforced_policy` false) may run, with
   the sandbox and review as the boundary and the UI saying so; or they may
   run only in isolated mode. Recommended: isolated mode only until the
   sandbox covers every OS, then both.

## Sources

- ACP: [agentclientprotocol.com overview](https://agentclientprotocol.com/protocol/overview),
  [v2 migration](https://agentclientprotocol.com/protocol/v2/migration),
  [v2 draft announcement](https://agentclientprotocol.com/announcements/acp-v2-draft),
  [Zed: ACP registry](https://zed.dev/blog/acp-registry),
  [Zed external agents](https://zed.dev/docs/ai/external-agents),
  [Rust SDK](https://docs.rs/crate/agent-client-protocol)
- Codex: [App Server](https://developers.openai.com/codex/app-server),
  [Unlocking the Codex harness](https://openai.com/index/unlocking-the-codex-harness),
  [Authentication](https://developers.openai.com/codex/auth),
  [codex-acp](https://zed.dev/blog/codex-is-live-in-zed)
- Gemini CLI: [headless mode](https://geminicli.com/docs/cli/headless),
  [ACP mode](https://geminicli.com/docs/cli/acp-mode/),
  [configuration](https://geminicli.com/docs/reference/configuration)
- Copilot CLI: [ACP server](https://docs.github.com/en/copilot/reference/copilot-cli-reference/acp-server),
  [changelog 2026-01-28](https://github.blog/changelog/2026-01-28-acp-support-in-copilot-cli-is-now-in-public-preview/)
- Cursor CLI: [output format](https://cursor.com/docs/cli/reference/output-format),
  [ACP](https://cursor.com/docs/cli/acp)
- opencode: [ACP](https://opencode.ai/docs/cli/acp/), [server](https://opencode.ai/docs/server/)
- Goose: [ACP clients](https://goose-docs.ai/docs/guides/acp-clients)
- Aider: [scripting](https://aider.chat/docs/scripting.html)
- AGENTS.md in Claude Code: [InfoWorld](https://infoworld.com/article/4224410/claude-code-now-also-accepts-instructions-in-openais-agents-md-format.html),
  [DevOps.com](https://devops.com/claude-code-adds-agents-md-fallback-cutting-instruction-file-sprawl/)
  (secondary sources; verify against Claude Code's changelog)
