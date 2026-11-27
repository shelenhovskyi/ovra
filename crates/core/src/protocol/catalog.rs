//! `catalog.*`: what a project gives its agents, read from its own files:
//! rules files, skills, subagents, slash commands, hooks and MCP servers,
//! for Claude, Gemini and Codex. It only reads them; the files are opened
//! with `fs.read_file` like any other. The person turns the servers of
//! `.mcp.json` on for Ovra's agents one by one (`docs/specs/project-mcp.md`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::agents::AgentKind;
use super::projects::ProjectId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum CatalogRequest {
    /// Everything the project's files give its agents. Answers
    /// `catalog.items`.
    #[serde(rename = "catalog.list")]
    List { project: ProjectId },
    /// Turns a server of the project's `.mcp.json` on or off for Ovra's
    /// agents. On pins its definition as written now; a server that needs
    /// what Ovra does not pass ([`ServerUse::blocked`]) is refused. Answers
    /// `ok`.
    #[serde(rename = "catalog.set_server")]
    SetServer {
        project: ProjectId,
        name: String,
        on: bool,
    },
    /// Turns one tool of a server that is on off or on again. Answers `ok`.
    #[serde(rename = "catalog.set_tool")]
    SetTool {
        project: ProjectId,
        server: String,
        tool: String,
        on: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CatalogResponse {
    #[serde(rename = "catalog.items")]
    Items(Catalog),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    /// By kind, then by path and name.
    pub items: Vec<CatalogItem>,
}

/// One rules file, skill, subagent, command, hook or server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogItem {
    pub kind: CatalogKind,
    /// What the agent calls it: `review`, `/ship`, `PreToolUse`, `CLAUDE.md`.
    pub name: String,
    /// What it says it is for: the frontmatter's `description`, a rules
    /// file's first line of text, a hook's command, a server's command.
    pub description: Option<String>,
    /// The file it is written in, relative to the project root.
    pub path: String,
    /// The agents that read it.
    pub agents: Vec<AgentKind>,
    pub loads: Loads,
    /// Its other settings, as written: frontmatter fields (`tools`,
    /// `model`), a hook's `matcher`, a server's `url`.
    pub fields: Vec<CatalogField>,
    /// A skill's other files (scripts, references), relative to the
    /// project root.
    pub files: Vec<String>,
    /// Why it may not work as written: a file that could not be read or
    /// parsed, a skill without a description.
    pub problem: Option<String>,
    /// For a server of `.mcp.json`: whether Ovra's agents get it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<ServerUse>,
}

/// Whether Ovra's agents get one server of `.mcp.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerUse {
    /// The person turned it on, and it is as it was then.
    pub on: bool,
    /// The person turned it on, but `.mcp.json` changed it since: it is off
    /// until they turn it on again.
    pub changed: bool,
    /// Why Ovra cannot give it to agents yet, in words; `None` if it can.
    pub blocked: Option<String>,
    /// The host it reaches, which turning it on adds to the project's
    /// netproxy allowlist.
    pub host: Option<String>,
    /// Its tools, as the last session that had it listed them.
    pub tools: Vec<ServerTool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerTool {
    /// The server's own name for it, without `mcp__<server>__`.
    pub name: String,
    pub on: bool,
}

/// The servers of a project's `.mcp.json` the person turned on, as Ovra
/// stores them (not sent over the wire).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerPins {
    pub servers: BTreeMap<String, ServerPin>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerPin {
    /// Its definition when it was turned on, as compact JSON: a session
    /// gets the server only while `.mcp.json` still says exactly this.
    pub definition: String,
    /// Its tools, as the last session that had it listed them.
    pub tools: Vec<String>,
    /// Tools the person turned off.
    pub tools_off: Vec<String>,
    /// The host Ovra added to the allowlist for it, removed with it.
    pub added_host: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogKind {
    /// Instructions every session reads (`CLAUDE.md`, `AGENTS.md`, ...).
    Rules,
    /// A folder with a `SKILL.md`, which the agent uses when it fits.
    Skill,
    /// A helper agent with its own instructions and tools.
    Subagent,
    /// A prompt saved under a `/name`.
    Command,
    /// A command that runs on an agent event.
    Hook,
    /// An MCP server that gives the agent more tools.
    McpServer,
}

/// When Claude sessions in Ovra load it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Loads {
    /// In every session.
    Always,
    /// Only once the person trusts the project (`agents.project_config`).
    WhenTrusted,
    /// Never: Ovra gives agents its own tools only.
    NotInOvra,
    /// Up to the agent; Ovra does not decide it.
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogField {
    pub name: String,
    pub value: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Request, Response};

    #[test]
    fn wire_format_is_stable() {
        let item = CatalogItem {
            kind: CatalogKind::Skill,
            name: "release".into(),
            description: Some("Cuts a release".into()),
            path: ".claude/skills/release/SKILL.md".into(),
            agents: vec![AgentKind::Claude],
            loads: Loads::WhenTrusted,
            fields: vec![CatalogField {
                name: "allowed-tools".into(),
                value: "Bash".into(),
            }],
            files: vec![".claude/skills/release/run.sh".into()],
            problem: None,
            server: None,
        };
        let server = CatalogItem {
            kind: CatalogKind::McpServer,
            name: "docs".into(),
            description: Some("npx -y docs-server".into()),
            path: ".mcp.json".into(),
            agents: vec![AgentKind::Claude],
            loads: Loads::NotInOvra,
            fields: Vec::new(),
            files: Vec::new(),
            problem: None,
            server: Some(ServerUse {
                on: true,
                changed: false,
                blocked: None,
                host: None,
                tools: vec![ServerTool {
                    name: "search".into(),
                    on: false,
                }],
            }),
        };
        insta::assert_json_snapshot!(serde_json::json!({
            "request": Request::Catalog(CatalogRequest::List { project: ProjectId("p1".into()) }),
            "response": Response::Catalog(CatalogResponse::Items(Catalog { items: vec![item] })),
        }));
        insta::assert_json_snapshot!(
            "server_use",
            serde_json::json!({
                "set_server": Request::Catalog(CatalogRequest::SetServer {
                    project: ProjectId("p1".into()),
                    name: "docs".into(),
                    on: true,
                }),
                "set_tool": Request::Catalog(CatalogRequest::SetTool {
                    project: ProjectId("p1".into()),
                    server: "docs".into(),
                    tool: "search".into(),
                    on: false,
                }),
                "item": server,
            })
        );
    }
}
