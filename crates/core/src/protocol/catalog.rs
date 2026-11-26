//! `catalog.*`: what a project gives its agents, read from its own files:
//! rules files, skills, subagents, slash commands, hooks and MCP servers,
//! for Claude, Gemini and Codex. It only reads; the files are opened with
//! `fs.read_file` like any other.

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
        };
        insta::assert_json_snapshot!(serde_json::json!({
            "request": Request::Catalog(CatalogRequest::List { project: ProjectId("p1".into()) }),
            "response": Response::Catalog(CatalogResponse::Items(Catalog { items: vec![item] })),
        }));
    }
}
