use std::path::Path;
use std::sync::Arc;

use crate::fakes::{MemoryFs, MemoryProjects};
use crate::protocol::agents::AgentKind;
use crate::protocol::catalog::{
    CatalogItem, CatalogKind, CatalogRequest, CatalogResponse, ServerTool, ServerUse,
};
use crate::protocol::netproxy::{AllowedHost, Allowlist};
use crate::protocol::projects::{ProjectId, ProjectsRequest, ProjectsResponse};
use crate::protocol::{ErrorCode, ProtocolError, Request, Response};
use crate::{Core, test_path};

use super::servers;

const ROOT: &str = test_path!("/work/a");

async fn fixture(files: &[(&str, &str)]) -> (Core, ProjectId) {
    let (core, project, _) = fixture_fs(files).await;
    (core, project)
}

async fn fixture_fs(files: &[(&str, &str)]) -> (Core, ProjectId, Arc<MemoryFs>) {
    let fs = Arc::new(MemoryFs::default());
    for (path, text) in files {
        fs.put(Path::new(ROOT), path, text);
    }
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .fs(fs.clone())
        .build();
    let added = core
        .handle(Request::Projects(ProjectsRequest::Add {
            root: ROOT.into(),
        }))
        .await;
    let Ok(Response::Projects(ProjectsResponse::Project(project))) = added else {
        panic!("{added:?}");
    };
    (core, project.id, fs)
}

async fn list(core: &Core, project: &ProjectId) -> Result<Vec<CatalogItem>, ErrorCode> {
    let request = Request::Catalog(CatalogRequest::List {
        project: project.clone(),
    });
    match core.handle(request).await {
        Ok(Response::Catalog(CatalogResponse::Items(catalog))) => Ok(catalog.items),
        Ok(other) => panic!("unexpected {other:?}"),
        Err(e) => Err(e.code),
    }
}

async fn items(files: &[(&str, &str)]) -> Vec<CatalogItem> {
    let (core, project) = fixture(files).await;
    list(&core, &project)
        .await
        .unwrap_or_else(|e| panic!("{e:?}"))
}

#[tokio::test]
async fn lists_every_kind_for_every_agent() {
    let items = items(&[
        ("CLAUDE.md", "# Ovra\n\nAlways write English.\n"),
        ("AGENTS.md", "Shared rules."),
        ("GEMINI.md", "Gemini rules."),
        (".claude/rules/tests.md", "Keep tests fast."),
        (
            ".claude/skills/release/SKILL.md",
            "---\nname: release\ndescription: Cuts a release when asked.\nallowed-tools: Bash\n---\nSteps.",
        ),
        (".claude/skills/release/scripts/tag.sh", "git tag"),
        (".claude/skills/bare/SKILL.md", "Just text."),
        (".claude/skills/notes.md", "not a skill"),
        (
            ".claude/agents/reviewer.md",
            "---\nname: reviewer\ndescription: Reviews diffs.\ntools: Read, Grep\nmodel: opus\n---\nYou review.",
        ),
        (".claude/commands/git/ship.md", "Commit and push the work."),
        (".claude/commands/notes.txt", "not a command"),
        (".gemini/commands/git/ship.toml", "description = \"Ships it\"\nprompt = \"...\""),
        (
            ".claude/settings.json",
            r#"{"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "./check.sh", "timeout": 30}]}],
                "Stop": [{"hooks": [{"type": "prompt", "prompt": "Did you run the tests?"}]}]}}"#,
        ),
        (
            ".mcp.json",
            r#"{"mcpServers": {"db": {"command": "npx", "args": ["db-mcp", "--ro"], "env": {"DB_TOKEN": "secret-value"}},
                "docs": {"type": "http", "url": "https://docs.example/mcp", "headers": {"Authorization": "Bearer x"}}}}"#,
        ),
        (
            ".gemini/settings.json",
            r#"{"mcpServers": {"search": {"httpUrl": "https://search.example/mcp", "disabled": true}}}"#,
        ),
        (
            ".codex/config.toml",
            "[mcp_servers.files]\ncommand = \"files-mcp\"\nenv = { KEY = \"v\" }\n",
        ),
    ])
    .await;
    insta::assert_json_snapshot!(items);
    let shown = serde_json::to_string(&items).unwrap_or_default();
    for secret in ["secret-value", "Bearer x", "\"v\""] {
        assert!(!shown.contains(secret), "{secret} is shown");
    }
}

#[tokio::test]
async fn claude_reads_agents_md_only_without_claude_md() {
    let alone = items(&[("AGENTS.md", "Rules.")]).await;
    assert_eq!(alone.len(), 1);
    assert_eq!(alone[0].agents, [AgentKind::Claude, AgentKind::Codex]);
}

#[tokio::test]
async fn files_that_cannot_be_read_say_why() {
    let items = items(&[
        (".mcp.json", "{not json"),
        (".codex/config.toml", "[mcp_servers"),
        (".gemini/commands/bad.toml", "description = "),
        (".claude/settings.json", "{broken"),
    ])
    .await;
    let found: Vec<_> = items
        .iter()
        .map(|i| (i.kind, i.path.as_str(), i.problem.is_some()))
        .collect();
    assert_eq!(
        found,
        [
            (CatalogKind::Command, ".gemini/commands/bad.toml", true),
            (CatalogKind::McpServer, ".mcp.json", true),
            (CatalogKind::McpServer, ".codex/config.toml", true),
        ]
    );
}

#[tokio::test]
async fn an_empty_project_has_nothing_and_an_unknown_one_is_an_error() {
    let (core, project) = fixture(&[("src/main.rs", "fn main() {}")]).await;
    assert_eq!(list(&core, &project).await, Ok(Vec::new()));
    let unknown = ProjectId("nope".into());
    assert_eq!(list(&core, &unknown).await, Err(ErrorCode::NotFound));
}

async fn set(core: &Core, request: CatalogRequest) -> Result<(), ProtocolError> {
    match core.handle(Request::Catalog(request)).await {
        Ok(Response::Ok(_)) => Ok(()),
        Ok(other) => panic!("unexpected {other:?}"),
        Err(e) => Err(e),
    }
}

fn server(name: &str, on: bool, project: &ProjectId) -> CatalogRequest {
    CatalogRequest::SetServer {
        project: project.clone(),
        name: name.into(),
        on,
    }
}

async fn uses(core: &Core, project: &ProjectId) -> Vec<(String, Option<ServerUse>)> {
    list(core, project)
        .await
        .unwrap_or_else(|e| panic!("{e:?}"))
        .into_iter()
        .filter(|i| i.kind == CatalogKind::McpServer)
        .map(|i| (i.name, i.server))
        .collect()
}

const SERVERS: &str = r#"{"mcpServers": {
    "db": {"command": "npx", "args": ["db-mcp"]},
    "keyed": {"command": "x", "env": {"TOKEN": "${DB_TOKEN}"}},
    "ovra": {"command": "x"},
    "docs": {"type": "http", "url": "https://Docs.Example:8443/mcp"},
    "plain": {"type": "http", "url": "http://docs.example/mcp"}
}}"#;

#[tokio::test]
async fn a_server_is_off_until_the_person_turns_it_on_and_pinned_as_it_was() {
    let (core, project, fs) = fixture_fs(&[(".mcp.json", SERVERS)]).await;
    let blocked = |name: &str, uses: &[(String, Option<ServerUse>)]| {
        uses.iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, u)| u.as_ref())
            .and_then(|u| u.blocked.clone())
    };
    let before = uses(&core, &project).await;
    assert!(
        before
            .iter()
            .all(|(_, u)| u.as_ref().is_some_and(|u| !u.on))
    );
    assert_eq!(blocked("db", &before), None);
    assert!(blocked("keyed", &before).is_some_and(|b| b.contains("DB_TOKEN")));
    assert!(blocked("ovra", &before).is_some());
    assert!(blocked("plain", &before).is_some_and(|b| b.contains("HTTPS")));
    for name in ["keyed", "ovra", "plain"] {
        let refused = set(&core, server(name, true, &project)).await;
        assert_eq!(
            refused.map_err(|e| e.code),
            Err(ErrorCode::BadRequest),
            "{name}"
        );
    }
    assert_eq!(
        set(&core, server("nope", true, &project))
            .await
            .map_err(|e| e.code),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(servers::for_session(&core, &project).await, []);

    set(&core, server("db", true, &project))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    let on = servers::for_session(&core, &project).await;
    assert_eq!(on.len(), 1);
    assert_eq!(on[0].name, "db");
    assert_eq!(
        on[0].definition,
        serde_json::json!({"command": "npx", "args": ["db-mcp"]})
    );

    // A session lists its tools; the person turns one off.
    let host_tools = [
        "mcp__db__query".to_owned(),
        "mcp__db__drop".to_owned(),
        "mcp__docs__x".to_owned(),
    ];
    servers::tools_listed(&core, &project, &host_tools);
    let tool = |name: &str, on: bool| CatalogRequest::SetTool {
        project: project.clone(),
        server: "db".into(),
        tool: name.into(),
        on,
    };
    set(&core, tool("drop", false))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    let shown = uses(&core, &project).await;
    let db = shown
        .iter()
        .find(|(n, _)| n == "db")
        .and_then(|(_, u)| u.clone());
    assert_eq!(
        db.map(|u| (u.on, u.tools)),
        Some((
            true,
            vec![
                ServerTool {
                    name: "drop".into(),
                    on: false
                },
                ServerTool {
                    name: "query".into(),
                    on: true
                },
            ]
        ))
    );
    assert_eq!(
        servers::for_session(&core, &project).await[0].tools_off,
        ["drop"]
    );

    // The file changes the server: off again until turned on again.
    fs.put(
        Path::new(ROOT),
        ".mcp.json",
        &SERVERS.replace("db-mcp", "db-mcp --write"),
    );
    let changed = uses(&core, &project).await;
    let db = changed
        .iter()
        .find(|(n, _)| n == "db")
        .and_then(|(_, u)| u.clone());
    assert_eq!(db.map(|u| (u.on, u.changed)), Some((false, true)));
    assert_eq!(servers::for_session(&core, &project).await, []);
    set(&core, server("db", true, &project))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        servers::for_session(&core, &project).await[0].tools_off,
        ["drop"]
    );
    set(&core, server("db", false, &project))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(servers::for_session(&core, &project).await, []);
}

#[tokio::test]
async fn an_https_server_puts_its_host_on_the_allowlist_while_it_is_on() {
    let (core, project, _) = fixture_fs(&[(".mcp.json", SERVERS)]).await;
    let hosts = || {
        core.project_store()
            .allowlist(&project)
            .unwrap_or_default()
            .hosts
    };
    set(&core, server("docs", true, &project))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        hosts(),
        [AllowedHost {
            host: "docs.example".into(),
            port: Some(8443)
        }]
    );
    set(&core, server("docs", false, &project))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(hosts(), []);
    // A host the person listed themselves stays when the server goes.
    let mine = Allowlist {
        hosts: vec![AllowedHost {
            host: "docs.example".into(),
            port: None,
        }],
    };
    core.project_store()
        .set_allowlist(&project, &mine)
        .unwrap_or_else(|e| panic!("{e:?}"));
    set(&core, server("docs", true, &project))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    set(&core, server("docs", false, &project))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(hosts(), mine.hosts);
}
