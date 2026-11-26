// Mirrors crates/core/src/protocol/catalog.rs.

import type { AgentKind } from "./agents";

export type CatalogRequest =
  /** Everything the project's files give its agents. */
  { method: "catalog.list"; params: { project: string } };

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
};

export type CatalogKind = "rules" | "skill" | "subagent" | "command" | "hook" | "mcp_server";

/** When Claude sessions in Ovra load it; `agent` when the agent decides. */
export type Loads = "always" | "when_trusted" | "not_in_ovra" | "agent";

export type CatalogField = { name: string; value: string };
