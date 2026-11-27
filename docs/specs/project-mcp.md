# A project's MCP servers, turned on per project

Status: approved by the owner 2026-11-27. It gives agents more reach (a
security loosening), which the owner accepted as described here.

Today every Claude session gets `--strict-mcp-config` and only Ovra's own
`ovra` server. A project's `.mcp.json` never loads, and "Skills and
instructions" shows its servers as "Not used in Ovra". People who rely on a
database, docs or issue-tracker server have no way to use it, and no way to
keep one server or one tool off (claude-code#6915).

## Behaviour

- **Off by default.** Nothing changes for a project until the person turns
  a server on. Agents never turn one on, and a cloned project's
  `.mcp.json` alone does nothing.
- **Skills and instructions > Tool servers.** Each server of `.mcp.json`
  gets a switch "Use in Ovra" with what it runs (the command and
  arguments, or the URL) and which variable names it needs. Turning one on
  shows, once, what it can do: "This server runs `npx -y @x/server` inside
  the agent's sandbox. Every tool call still asks you first."
- **Per tool.** Once a session started with the server, its tools are
  listed (from the session's `system/init`) with a switch each; a tool
  that is off is passed as `--disallowedTools mcp__<server>__<tool>`.
- **Pinned definition.** Ovra keeps the server's definition as it was
  when it was turned on (a hash of its JSON). When `.mcp.json` changes
  that server, it is off again until the person looks at it and turns it
  on again, like project trust today.
- **Every MCP tool call asks**, as any tool does (`can_use_tool`). "Always
  allow" works per tool as it does for others. Ovra's own policies still
  apply to the call's input.
- **Who gets it.** Direct sessions and workers. Coordinators keep only
  `ovra` (owner decision 2026-11-24); research sessions get none (they are
  read-only, ADR 0013).

## Security

- **stdio servers** are started by `claude` inside the session's sandbox,
  so they get its file rules, its protected paths and its netproxy. A
  server fetched with `npx`/`uvx` needs its registry host; the per-session
  package cache applies.
- **HTTP/SSE servers**: turning one on adds its URL's host to that
  project's netproxy allowlist, shown under Network like any other host.
  OAuth servers are out of v1 (Claude keeps their tokens in its home
  state, which the sandbox keeps read-only).
- **Variables** (`${VAR}` in `command`, `args`, `env`, `headers`) are not
  passed: the sandbox env allowlist stays as it is. A server that needs
  one says "Needs SECRET_NAME" and waits for the secrets work
  (`docs/specs/secrets-consent.md`).
- Ovra passes the pinned JSON itself (`--mcp-config`), still with
  `--strict-mcp-config`, so user-level servers from `~/.claude.json` stay
  out and nothing in `.mcp.json` loads that the person did not turn on.
- Needs the independent reviewer (agent-runtime launch arguments,
  netproxy allowlist).

## Not in v1

User-level servers (`~/.claude.json`), Gemini and Codex servers (ACP
passes `mcpServers` per session; with ADR 0011's adapter), OAuth, servers
that need variables.

## Shape

- Protocol `catalog`: `catalog.set_server {project, path, name, on}` and
  `catalog.set_tool {project, server, tool, on}`; `CatalogItem` gains
  `use: {on, changed, tools}` for MCP servers.
- Store: per project, `{name -> {hash, definition, tools_off}}` in
  `ProjectStore`.
- agent-runtime: `args()` takes the pinned servers and merges them into the
  `--mcp-config` JSON next to `ovra`, plus `--disallowedTools`.
