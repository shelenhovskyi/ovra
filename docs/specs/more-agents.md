# Gemini CLI and Codex as Ovra agents

Status: proposed 2026-11-26; steps 8 to 10 of
`docs/research/provider-agnostic.md` (migration plan), under ADR 0011 (all
six decisions accepted as recommended). Builds on the generic ACP adapter
(`docs/specs/acp-adapter.md`, on `main`).

## Problem

The ACP adapter runs a scripted fake agent in tests, but no person can
start a real one: the core knows only `AgentKind::Claude`, nothing routes a
session to `AcpAgents`, and ADR 0011 lets an agent without an enforced
policy run only in isolated mode, which today only a coordinator can start.

## Goals

- A person starts a Gemini CLI or Codex session from the task form, in its
  own worktree, and reviews its work in Reviews with Keep and Discard, as a
  worker's.
- Settings > Agents shows one card per provider: found or not, its version,
  signed in or not, and what to do, in that vendor's own words.
- Claude sessions do not change.

Not goals here: Gemini or Codex as coordinator, direct or research
sessions, accounts other than the default one, resume, usage and cost, a
macOS sandbox (see "Open question").

## Spike (2026-11-26)

- `gemini --acp` (0.71.1, Apache-2.0) answers `initialize` with
  `protocolVersion: 1`, `loadSession`, stdio, http and sse MCP servers, and
  four auth methods (Google login, Gemini API key, Vertex AI, gateway).
  Without a sign-in `session/new` answers `-32000 "Gemini API key is
  missing or not configured."`.
- `codex-acp` (0.16.0, Zed's adapter, Apache-2.0; standalone, links Codex
  itself) answers `initialize` with v1, `loadSession`, session list,
  resume and close, auth methods ChatGPT login, `CODEX_API_KEY` and
  `OPENAI_API_KEY`; without a sign-in `session/new` answers `-32000
  "Authentication required"`. State lives in `CODEX_HOME` (`~/.codex`).
- Both are the vendors' own programs, which the person installs and signs
  in to themselves (`gemini`, `codex login`). Ovra never installs them,
  never reads their credential files, and never calls ACP `authenticate`.

## Design

1. **Providers.** `AgentKind` gains `Gemini` and `Codex`. A router
   `AgentRuntimes` in `ovra-core` holds one runtime per provider, forwards
   `start`, `setup`, `project_config` and `tool_call` by provider, and
   joins `providers()`. The daemon and app build it from `ClaudeCli` and
   `AcpAgents` (new edges in `scripts/crate-graph.json`). `setup` takes the
   provider; `AgentSettings.paths` keeps a chosen executable per provider
   next to `claude_path`.
2. **What the core refuses.** A provider without `enforced_policy` starts
   no direct session; one without `ide_tools` and `read_only_role` no
   coordinator; research and other accounts only where the capabilities
   say so. Each refusal names the provider and what to do instead.
3. **One agent in its own copy.** `orchestration.start` takes `agent`
   (default `claude`) and `solo`: a solo task has no coordinator and one
   worker, `agent`, started at once in a worktree on branch
   `ovra/<task>/agent`, with the person's prompt. It is reviewed, kept,
   discarded, cleaned up and capped like any worker. `Task.coordinator`
   becomes optional (`null` for a solo task). The task form gets a fourth
   mode, "One agent, own copy", and an agent picker shown only when more
   than one provider is ready; for a provider that can only run isolated,
   the form offers only that mode and says why in one line. Claude can use
   the mode too.
4. **Descriptors.** In `ovra-acp-client`, as data:
   - Gemini: program `gemini`, args `--acp`, home folder `.gemini`
     (read-only, with `tmp/` and `history/` private to the session, and
     its sign-in files rewritable for token refresh), variables
     `GOOGLE_CLOUD_PROJECT`, `GOOGLE_CLOUD_LOCATION`,
     `GOOGLE_GENAI_USE_VERTEXAI`, `GOOGLE_GENAI_USE_GCA`, hosts
     `generativelanguage.googleapis.com`, `cloudcode-pa.googleapis.com`,
     `oauth2.googleapis.com`, `www.googleapis.com` (token refresh and
     user info), plus Vertex's regional host when its variables say so.
   - Codex: program `codex-acp`, no args, home folder `.codex` with
     `sessions/`, `log/` and `tmp/` private and `auth.json` rewritable,
     no variables, hosts `api.openai.com`, `chatgpt.com`, `auth.openai.com`.
     Like Claude's, API key variables are never passed into the sandbox; the
     person signs in through the vendor's flow. The netproxy gets each
     provider's base list, so a Gemini session cannot reach Anthropic or
     OpenAI.
5. **Setup and sign-in.** `setup` runs `<program> --version` outside the
   sandbox and reports `NotFound`, `Broken` or `Ready`. Sign-in is learned
   from the agent: a `session/new` error that says it needs a login ends
   the session as failed with the vendor's sign-in hint ("Run `gemini` in
   a terminal once and sign in", "Run `codex login`"), and the provider's
   card turns `SignedOut` until the next successful start.
6. **Rules files.** The trust card's "rules files" list says which file
   each ready provider reads: Claude `CLAUDE.md` (and `AGENTS.md` when only
   that exists), Gemini `GEMINI.md`, Codex `AGENTS.md`. Ovra recommends
   `AGENTS.md` in that card's one line of help and never writes rules
   files. Project config readers for `.gemini/settings.json` and
   `.codex/config.toml` join the trust fingerprint.
7. **IDE tools for ACP agents (MCP bridge).** A worker's tools reach an
   ACP agent as a stdio MCP server in `session/new`: Ovra's own binary in
   bridge mode (`ovra-daemon --mcp-bridge`, the app likewise), which the
   agent starts inside its sandbox. The bridge connects to a Unix socket
   in the session's private folder, bound into the sandbox, and presents a
   per-session random token; the core answers only that session, with its
   role, exactly as `orchestration/policy.rs` does for Claude. It gets the
   independent reviewer's verdict before it lands. Until it lands, ACP
   workers run without IDE tools and their turn's end asks for review.

## Open question for the owner

ACP agents run only inside Ovra's sandbox, and macOS has none yet, so on a
Mac Gemini and Codex refuse to start. Options: wait for a macOS sandbox
(Seatbelt), built as its own `platform:macos` task, which would also
sandbox Claude on a Mac (recommended); or let them run on a Mac in their
worktree with only their own built-in sandbox, which loosens ADR 0011.

## Tests

- Core: the router sends each session to its provider and refuses unknown
  ones; direct and coordinator starts are refused without the capability;
  a solo task starts one worker, reviews, keeps and cleans it.
- `ovra-acp-client`: descriptor snapshots (lists, hosts), the sign-in
  error mapping, setup against fake programs.
- Netproxy: each provider's base list (snapshots).
- UI: the picker shows only with two ready providers; an isolated-only
  provider offers only "One agent, own copy"; Settings > Agents cards.
- e2e: a solo task with the fake ACP agent, from the form to Keep.

## Size

About eight commits of under ~400 lines: protocol and services, router and
refusals, solo tasks, descriptors and netproxy lists, setup and sign-in,
UI form and Settings, rules files, then the bridge with its review.
