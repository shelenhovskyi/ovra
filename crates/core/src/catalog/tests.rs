use std::path::Path;
use std::sync::Arc;

use crate::fakes::{MemoryFs, MemoryProjects};
use crate::protocol::agents::AgentKind;
use crate::protocol::catalog::{CatalogItem, CatalogKind, CatalogRequest, CatalogResponse};
use crate::protocol::projects::{ProjectId, ProjectsRequest, ProjectsResponse};
use crate::protocol::{ErrorCode, Request, Response};
use crate::{Core, test_path};

const ROOT: &str = test_path!("/work/a");

async fn fixture(files: &[(&str, &str)]) -> (Core, ProjectId) {
    let fs = Arc::new(MemoryFs::default());
    for (path, text) in files {
        fs.put(Path::new(ROOT), path, text);
    }
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .fs(fs)
        .build();
    let added = core
        .handle(Request::Projects(ProjectsRequest::Add {
            root: ROOT.into(),
        }))
        .await;
    let Ok(Response::Projects(ProjectsResponse::Project(project))) = added else {
        panic!("{added:?}");
    };
    (core, project.id)
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
