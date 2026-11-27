//! The project's agent catalog (`catalog.list`): its rules files, skills,
//! subagents, slash commands, hooks and MCP servers, read through the
//! project's `FsService` so the person can see them without opening hidden
//! folders. Only reads, and never shows an `env` or header value: only
//! their names.

mod frontmatter;
pub(crate) mod servers;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use serde_json::Value;

use crate::Core;
use crate::path::RelPath;
use crate::protocol::agents::AgentKind;
use crate::protocol::catalog::{
    Catalog, CatalogField, CatalogItem, CatalogKind, CatalogRequest, CatalogResponse, Loads,
};
use crate::protocol::fs::{EntryKind, FileBody};
use crate::protocol::{OkResponse, ProtocolError, Response};
use crate::services::FsService;

/// Files bigger than this are listed but not read.
const MAX_BYTES: u64 = 512 << 10;
/// Folder depth walked under `.claude/agents` and the others.
const MAX_DEPTH: usize = 4;
/// Files looked at per folder walk, and items listed per kind.
const MAX_FILES: usize = 200;

pub(crate) async fn handle(
    core: &Core,
    request: CatalogRequest,
) -> Result<Response, ProtocolError> {
    let project = match request {
        CatalogRequest::List { project } => project,
        CatalogRequest::SetServer { project, name, on } => {
            servers::set_server(core, &project, &name, on).await?;
            return Ok(Response::Ok(OkResponse::Ok));
        }
        CatalogRequest::SetTool {
            project,
            server,
            tool,
            on,
        } => {
            servers::set_tool(core, &project, &server, &tool, on)?;
            return Ok(Response::Ok(OkResponse::Ok));
        }
    };
    let root = core.root(&project)?;
    let reader = Reader {
        fs: core.fs_service(),
        root: root.clone(),
    };
    let mut items = list(&reader).await;
    let definitions = servers::definitions(core, root).await;
    let pins = core.project_store().servers(&project).unwrap_or_default();
    for item in &mut items {
        if item.kind == CatalogKind::McpServer
            && item.path == servers::FILE
            && let Some(definition) = definitions.get(&item.name)
        {
            item.server = Some(servers::server_use(&item.name, definition, &pins));
        }
    }
    Ok(Response::Catalog(CatalogResponse::Items(Catalog { items })))
}

struct Reader<'a> {
    fs: &'a dyn FsService,
    root: PathBuf,
}

impl Reader<'_> {
    /// `None` when there is no such file, else its text or why it has none.
    async fn text(&self, path: &str) -> Option<Result<String, String>> {
        let rel = RelPath::parse(path).ok()?;
        match self.fs.read_file(self.root.clone(), rel).await {
            Err(e) if e.code == crate::protocol::ErrorCode::NotFound => None,
            Err(e) => Some(Err(format!("Could not be read: {}", e.message))),
            Ok(file) if file.size > MAX_BYTES => Some(Err("Too big to read here.".into())),
            Ok(file) => Some(match file.body {
                FileBody::Text { text } => Ok(text),
                FileBody::Binary { .. } => Err("Not a text file.".into()),
                FileBody::TooLarge => Err("Too big to read here.".into()),
            }),
        }
    }

    /// The entries of a folder; none when it is missing or unreadable.
    async fn dir(&self, path: &str) -> Vec<(String, EntryKind)> {
        let Ok(rel) = RelPath::parse(path) else {
            return Vec::new();
        };
        match self.fs.list_dir(self.root.clone(), rel).await {
            Ok(listing) => listing
                .entries
                .into_iter()
                .map(|e| (e.name, e.kind))
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Every file under `dir` (links to folders followed, as the agents do),
    /// sorted, at most `MAX_FILES`.
    async fn files_under(&self, dir: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut folders = vec![(dir.to_owned(), 0)];
        while let Some((folder, depth)) = folders.pop() {
            for (name, kind) in self.dir(&folder).await {
                let path = format!("{folder}/{name}");
                let is_dir = match kind {
                    EntryKind::Dir => true,
                    EntryKind::File => false,
                    EntryKind::Symlink => !self.dir(&path).await.is_empty(),
                };
                if is_dir {
                    if depth < MAX_DEPTH {
                        folders.push((path, depth + 1));
                    }
                } else if found.len() < MAX_FILES {
                    found.push(path);
                }
            }
        }
        found.sort();
        found
    }
}

async fn list(reader: &Reader<'_>) -> Vec<CatalogItem> {
    let mut items = Vec::new();
    rules(reader, &mut items).await;
    skills(reader, &mut items).await;
    markdown(reader, ".claude/agents", CatalogKind::Subagent, &mut items).await;
    markdown(reader, ".claude/commands", CatalogKind::Command, &mut items).await;
    gemini_commands(reader, &mut items).await;
    match reader.text(".mcp.json").await {
        Some(Ok(text)) => match serde_json::from_str::<Value>(&text) {
            Ok(json) => servers(
                ".mcp.json",
                AgentKind::Claude,
                Loads::NotInOvra,
                json.get("mcpServers"),
                &mut items,
            ),
            Err(e) => items.push(unreadable(
                ".mcp.json",
                CatalogKind::McpServer,
                AgentKind::Claude,
                Loads::NotInOvra,
                format!("Not valid JSON: {e}"),
            )),
        },
        Some(Err(problem)) => items.push(unreadable(
            ".mcp.json",
            CatalogKind::McpServer,
            AgentKind::Claude,
            Loads::NotInOvra,
            problem,
        )),
        None => {}
    }
    for (path, agent, loads) in [
        (
            ".claude/settings.json",
            AgentKind::Claude,
            Loads::WhenTrusted,
        ),
        (
            ".claude/settings.local.json",
            AgentKind::Claude,
            Loads::WhenTrusted,
        ),
        (".gemini/settings.json", AgentKind::Gemini, Loads::Agent),
    ] {
        if let Some(Ok(text)) = reader.text(path).await
            && let Ok(json) = serde_json::from_str::<Value>(&text)
        {
            hooks(path, agent, loads, &json, &mut items);
            if agent == AgentKind::Gemini {
                servers(path, agent, loads, json.get("mcpServers"), &mut items);
            }
        }
    }
    if let Some(Ok(text)) = reader.text(".codex/config.toml").await {
        match toml::from_str::<Value>(&text) {
            Ok(config) => servers(
                ".codex/config.toml",
                AgentKind::Codex,
                Loads::Agent,
                config.get("mcp_servers"),
                &mut items,
            ),
            Err(e) => items.push(unreadable(
                ".codex/config.toml",
                CatalogKind::McpServer,
                AgentKind::Codex,
                Loads::Agent,
                format!("Not valid TOML: {}", e.message()),
            )),
        }
    }
    // Stable: each kind keeps the order its files were read in.
    items.sort_by_key(|item| item.kind);
    let mut per_kind = std::collections::HashMap::<CatalogKind, usize>::new();
    items.retain(|item| {
        let seen = per_kind.entry(item.kind).or_default();
        *seen += 1;
        *seen <= MAX_FILES
    });
    items
}

fn item(
    kind: CatalogKind,
    name: String,
    path: &str,
    agents: Vec<AgentKind>,
    loads: Loads,
) -> CatalogItem {
    CatalogItem {
        kind,
        name,
        description: None,
        path: path.to_owned(),
        agents,
        loads,
        fields: Vec::new(),
        files: Vec::new(),
        problem: None,
        server: None,
    }
}

fn unreadable(
    path: &str,
    kind: CatalogKind,
    agent: AgentKind,
    loads: Loads,
    problem: String,
) -> CatalogItem {
    let name = path.rsplit('/').next().unwrap_or(path).to_owned();
    CatalogItem {
        problem: Some(problem),
        ..item(kind, name, path, vec![agent], loads)
    }
}

async fn rules(reader: &Reader<'_>, items: &mut Vec<CatalogItem>) {
    use AgentKind::{Claude, Codex, Gemini};
    let has_claude_md = reader.text("CLAUDE.md").await.is_some();
    // Claude reads `AGENTS.md` only when there is no `CLAUDE.md`.
    let agents_md = if has_claude_md {
        vec![Codex]
    } else {
        vec![Claude, Codex]
    };
    let mut files = vec![
        ("CLAUDE.md".to_owned(), vec![Claude], Loads::Always),
        (".claude/CLAUDE.md".to_owned(), vec![Claude], Loads::Always),
        ("CLAUDE.local.md".to_owned(), vec![Claude], Loads::Agent),
    ];
    for path in reader.files_under(".claude/rules").await {
        if path.ends_with(".md") {
            files.push((path, vec![Claude], Loads::WhenTrusted));
        }
    }
    files.extend([
        ("AGENTS.md".to_owned(), agents_md, Loads::Agent),
        ("AGENTS.override.md".to_owned(), vec![Codex], Loads::Agent),
        ("GEMINI.md".to_owned(), vec![Gemini], Loads::Agent),
    ]);
    for (path, agents, loads) in files {
        let Some(text) = reader.text(&path).await else {
            continue;
        };
        let mut rules = item(CatalogKind::Rules, path.clone(), &path, agents, loads);
        match text {
            Ok(text) => rules.description = frontmatter::first_line(frontmatter::split(&text).1),
            Err(problem) => rules.problem = Some(problem),
        }
        items.push(rules);
    }
}

async fn skills(reader: &Reader<'_>, items: &mut Vec<CatalogItem>) {
    let mut folders: Vec<String> = reader
        .dir(".claude/skills")
        .await
        .into_iter()
        .filter(|(_, kind)| *kind != EntryKind::File)
        .map(|(name, _)| name)
        .collect();
    folders.sort();
    for folder in folders {
        let dir = format!(".claude/skills/{folder}");
        let path = format!("{dir}/SKILL.md");
        let Some(text) = reader.text(&path).await else {
            continue;
        };
        let mut skill = from_markdown(CatalogKind::Skill, folder, &path, text);
        skill.files = reader
            .files_under(&dir)
            .await
            .into_iter()
            .filter(|p| *p != path)
            .collect();
        if skill.problem.is_none() && skill.description.is_none() {
            skill.problem =
                Some("No description: Claude reads it to decide when to use the skill.".into());
        }
        items.push(skill);
    }
}

/// Subagents and commands: one `.md` file each, anywhere under `dir`.
async fn markdown(reader: &Reader<'_>, dir: &str, kind: CatalogKind, items: &mut Vec<CatalogItem>) {
    for path in reader.files_under(dir).await {
        let Some(stem) = path.strip_suffix(".md") else {
            continue;
        };
        let name = stem.rsplit('/').next().unwrap_or(stem);
        let name = if kind == CatalogKind::Command {
            format!("/{name}")
        } else {
            name.to_owned()
        };
        let Some(text) = reader.text(&path).await else {
            continue;
        };
        let mut found = from_markdown(kind, name, &path, text);
        // A command in a subfolder keeps its name; the folder only groups it.
        let folder = stem[dir.len()..]
            .trim_start_matches('/')
            .rsplit_once('/')
            .map(|(f, _)| f);
        if let (CatalogKind::Command, Some(folder)) = (kind, folder) {
            found.fields.insert(0, field("folder", folder));
        }
        items.push(found);
    }
}

/// A Claude file with a frontmatter header: its `name` and `description`
/// when it has them, its other fields as they are.
fn from_markdown(
    kind: CatalogKind,
    name: String,
    path: &str,
    text: Result<String, String>,
) -> CatalogItem {
    let mut found = item(
        kind,
        name,
        path,
        vec![AgentKind::Claude],
        Loads::WhenTrusted,
    );
    let text = match text {
        Ok(text) => text,
        Err(problem) => {
            found.problem = Some(problem);
            return found;
        }
    };
    let (header, body) = frontmatter::split(&text);
    for (key, value) in header {
        match key.as_str() {
            "name" if kind != CatalogKind::Command && !value.is_empty() => found.name = value,
            "description" if !value.is_empty() => {
                found.description = Some(frontmatter::shorten(&value))
            }
            _ if !value.is_empty() => found.fields.push(field(&key, &value)),
            _ => {}
        }
    }
    // Claude shows a command's first line when it has no description.
    if kind == CatalogKind::Command && found.description.is_none() {
        found.description = frontmatter::first_line(body);
    }
    found
}

async fn gemini_commands(reader: &Reader<'_>, items: &mut Vec<CatalogItem>) {
    const DIR: &str = ".gemini/commands";
    for path in reader.files_under(DIR).await {
        let Some(stem) = path.strip_suffix(".toml") else {
            continue;
        };
        // `git/ship.toml` is `/git:ship`.
        let name = format!("/{}", stem[DIR.len() + 1..].replace('/', ":"));
        let Some(text) = reader.text(&path).await else {
            continue;
        };
        let mut command = item(
            CatalogKind::Command,
            name,
            &path,
            vec![AgentKind::Gemini],
            Loads::Agent,
        );
        match text.map(|t| {
            toml::from_str::<Value>(&t).map_err(|e| format!("Not valid TOML: {}", e.message()))
        }) {
            Ok(Ok(toml)) => {
                command.description = toml
                    .get("description")
                    .and_then(Value::as_str)
                    .map(frontmatter::shorten);
            }
            Ok(Err(problem)) | Err(problem) => command.problem = Some(problem),
        }
        items.push(command);
    }
}

/// `hooks: {Event: [{matcher, hooks: [{type, command}]}]}`, one item per
/// command.
fn hooks(
    path: &str,
    agent: AgentKind,
    loads: Loads,
    settings: &Value,
    items: &mut Vec<CatalogItem>,
) {
    let Some(events) = settings.get("hooks").and_then(Value::as_object) else {
        return;
    };
    for (event, groups) in events {
        for group in groups.as_array().into_iter().flatten() {
            let matcher = group
                .get("matcher")
                .and_then(Value::as_str)
                .filter(|m| !m.is_empty() && *m != "*");
            for hook in group
                .get("hooks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let mut found = item(CatalogKind::Hook, event.clone(), path, vec![agent], loads);
                let kind = hook
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("command");
                found.description = ["command", "prompt", "url"]
                    .iter()
                    .find_map(|key| hook.get(key).and_then(Value::as_str))
                    .map(frontmatter::shorten);
                if let Some(matcher) = matcher {
                    found.fields.push(field("matcher", matcher));
                }
                if kind != "command" {
                    found.fields.push(field("type", kind));
                }
                if let Some(timeout) = hook.get("timeout").and_then(Value::as_u64) {
                    found.fields.push(field("timeout", &format!("{timeout} s")));
                }
                items.push(found);
            }
        }
    }
}

/// `{name: {command, args} | {url}}`, with only the names of `env` and
/// `headers`: their values are often keys.
fn servers(
    path: &str,
    agent: AgentKind,
    loads: Loads,
    servers: Option<&Value>,
    items: &mut Vec<CatalogItem>,
) {
    let Some(servers) = servers.and_then(Value::as_object) else {
        return;
    };
    for (name, server) in servers {
        let mut found = item(
            CatalogKind::McpServer,
            name.clone(),
            path,
            vec![agent],
            loads,
        );
        let text = |key: &str| server.get(key).and_then(Value::as_str);
        let command = text("command").map(|command| {
            let args = server
                .get("args")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str);
            std::iter::once(command)
                .chain(args)
                .collect::<Vec<_>>()
                .join(" ")
        });
        found.description = command
            .or_else(|| {
                ["url", "httpUrl", "serverUrl"]
                    .iter()
                    .find_map(|k| text(k))
                    .map(str::to_owned)
            })
            .map(|d| frontmatter::shorten(&d));
        if let Some(kind) = text("type") {
            found.fields.push(field("type", kind));
        }
        for key in ["env", "headers", "http_headers", "env_vars"] {
            let names: Vec<&str> = match server.get(key) {
                Some(Value::Object(map)) => map.keys().map(String::as_str).collect(),
                Some(Value::Array(list)) => list.iter().filter_map(Value::as_str).collect(),
                _ => continue,
            };
            if !names.is_empty() {
                found.fields.push(field(key, &names.join(", ")));
            }
        }
        if server.get("disabled").and_then(Value::as_bool) == Some(true)
            || server.get("enabled").and_then(Value::as_bool) == Some(false)
        {
            found.problem = Some("Turned off in this file.".into());
        }
        items.push(found);
    }
}

fn field(name: &str, value: &str) -> CatalogField {
    CatalogField {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}
