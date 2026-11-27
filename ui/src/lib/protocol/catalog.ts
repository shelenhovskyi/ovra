// Mirrors crates/core/src/protocol/catalog.rs.

import type { AgentKind } from "./agents";

export type CatalogRequest =
  /** Everything the project's files give its agents. */
  | { method: "catalog.list"; params: { project: string } }
  /** Turns a server of `.mcp.json` on or off for Ovra's agents; on pins it as written now. */
  | { method: "catalog.set_server"; params: { project: string; name: string; on: boolean } }
  /** Turns one tool of a server that is on off or on again. */
  | { method: "catalog.set_tool"; params: { project: string; server: string; tool: string; on: boolean } };

export type CatalogResponse = { type: "catalog.items"; data: Catalog };

export type Catalog = {
  /** By kind, then by path and name. */
  items: CatalogItem[];
};

/** One rules file, skill, subagent, command, hook or server. */
export type CatalogItem = {
  kind: CatalogKind;
  /** What the agent calls it: `review`, `/ship`, `PreToolUse`, `CLAUDE.md`. */
  name: string;
  /** The frontmatter's description, a rules file's first line, a hook's or server's command. */
  description: string | null;
  /** The file it is written in, relative to the project root. */
  path: string;
  /** The agents that read it. */
  agents: AgentKind[];
  loads: Loads;
  /** Its other settings as written: `tools`, `model`, a hook's `matcher`, a server's `env` names. */
  fields: CatalogField[];
  /** A skill's other files, relative to the project root. */
  files: string[];
  /** Why it may not work as written. */
  problem: string | null;
  /** For a server of `.mcp.json`: whether Ovra's agents get it. */
  server?: ServerUse;
};

/** Whether Ovra's agents get one server of `.mcp.json` (docs/specs/project-mcp.md). */
export type ServerUse = {
  /** The person turned it on, and it is as it was then. */
  on: boolean;
  /** Turned on, but `.mcp.json` changed it since: off until turned on again. */
  changed: boolean;
  /** Why Ovra cannot give it to agents yet, in words. */
  blocked: string | null;
  /** The host it reaches, added to the project's netproxy allowlist while on. */
  host: string | null;
  /** Its tools, as the last session that had it listed them. */
  tools: ServerTool[];
};

export type ServerTool = { name: string; on: boolean };

export type CatalogKind = "rules" | "skill" | "subagent" | "command" | "hook" | "mcp_server";

/** When Claude sessions in Ovra load it; `agent` when the agent decides. */
export type Loads = "always" | "when_trusted" | "not_in_ovra" | "agent";

export type CatalogField = { name: string; value: string };
