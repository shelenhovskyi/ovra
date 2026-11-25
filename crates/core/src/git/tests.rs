use std::sync::Arc;

use serde_json::json;

use crate::Core;
use crate::fakes::{MemoryGit, MemoryProjects, MemoryWorktrees, ScriptedAgent};
use crate::protocol::git::{GitResponse, RepoStatus};
use crate::protocol::{ErrorCode, ProtocolError, Request, Response};
use crate::test_path;

const ROOT: &str = test_path!("/work/a");

struct Fixture {
    core: Core,
    git: Arc<MemoryGit>,
    worktrees: Arc<MemoryWorktrees>,
}

async fn fixture() -> Fixture {
    let git = Arc::new(MemoryGit::repo());
    let worktrees = Arc::new(MemoryWorktrees::default());
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(ScriptedAgent::default()))
        .worktrees(worktrees.clone())
        .git(git.clone())
        .build();
    call(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    Fixture {
        core,
        git,
        worktrees,
    }
}

async fn call(core: &Core, json: serde_json::Value) -> Result<Response, ProtocolError> {
    let request: Request =
        serde_json::from_value(json).unwrap_or_else(|e| panic!("bad request: {e}"));
    core.handle(request).await
}

fn git(method: &str, params: serde_json::Value) -> serde_json::Value {
    let mut params = params;
    params["project"] = json!("p1");
    json!({"method": format!("git.{method}"), "params": params})
}

async fn code(core: &Core, json: serde_json::Value) -> ErrorCode {
    match call(core, json).await {
        Err(e) => e.code,
        Ok(r) => panic!("expected an error, got {r:?}"),
    }
}

#[tokio::test]
async fn status_answers_no_repo_outside_a_repository() {
    let f = fixture().await;
    let status = call(&f.core, git("status", json!({}))).await;
    assert_eq!(
        status,
        Ok(Response::Git(GitResponse::Status(RepoStatus {
            branch: Some("main".into()),
            head: Some("c0".into()),
            ..RepoStatus::default()
        })))
    );
    *f.git.status.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let status = call(&f.core, git("status", json!({}))).await;
    assert_eq!(status, Ok(Response::Git(GitResponse::NoRepo)));
    let wire = serde_json::to_value(status.unwrap_or_else(|e| panic!("{e}")))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(wire, json!({"type": "git.no_repo"}));
}

#[tokio::test]
async fn paths_must_stay_in_the_project_and_out_of_dot_git() {
    let f = fixture().await;
    for (paths, want) in [
        (json!(["../x"]), ErrorCode::Forbidden),
        (json!([".git/config"]), ErrorCode::Forbidden),
        (json!([".GIT/hooks/pre-commit"]), ErrorCode::Forbidden),
        (json!([""]), ErrorCode::BadRequest),
        (json!([]), ErrorCode::BadRequest),
    ] {
        let stage = git("stage", json!({"paths": paths}));
        assert_eq!(code(&f.core, stage).await, want, "{paths}");
    }
    let diff = git("diff", json!({"path": ".git/HEAD"}));
    assert_eq!(code(&f.core, diff).await, ErrorCode::Forbidden);
    call(
        &f.core,
        git("stage", json!({"paths": ["./src//a.rs", ".gitignore"]})),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(f.git.calls(), [format!("stage {ROOT} src/a.rs,.gitignore")]);
}

#[tokio::test]
async fn discard_takes_a_snapshot_that_undo_restores() {
    let f = fixture().await;
    f.worktrees.put(ROOT, "a.txt", "mine\n");
    let discarded = call(&f.core, git("discard", json!({"paths": ["a.txt"]}))).await;
    let Ok(Response::Git(GitResponse::Discarded { undo })) = discarded else {
        panic!("{discarded:?}");
    };
    assert_eq!(f.git.calls(), [format!("discard {ROOT} a.txt")]);
    // What the real service does to the file.
    f.worktrees.put(ROOT, "a.txt", "theirs\n");
    let undo = git("undo_discard", json!({"undo": undo, "paths": ["a.txt"]}));
    call(&f.core, undo).await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(f.worktrees.files(ROOT)["a.txt"], "mine\n");

    let bogus = git(
        "undo_discard",
        json!({"undo": "HEAD~1", "paths": ["a.txt"]}),
    );
    assert_eq!(code(&f.core, bogus).await, ErrorCode::BadRequest);
}

#[tokio::test]
async fn a_failed_snapshot_discards_nothing() {
    let f = fixture().await;
    // MemoryWorktrees has no working tree at ROOT yet: snapshot fails.
    let discard = git("discard", json!({"paths": ["a.txt"]}));
    assert_eq!(code(&f.core, discard).await, ErrorCode::NotFound);
    assert!(f.git.calls().is_empty());
}

#[tokio::test]
async fn commit_needs_a_message() {
    let f = fixture().await;
    for message in ["", "  \n"] {
        let commit = git("commit", json!({"message": message}));
        assert_eq!(code(&f.core, commit).await, ErrorCode::BadRequest);
    }
    let huge = git("commit", json!({"message": "x".repeat(64 * 1024 + 1)}));
    assert_eq!(code(&f.core, huge).await, ErrorCode::BadRequest);
    let committed = call(&f.core, git("commit", json!({"message": "Fix it\n\nWhy"}))).await;
    let Ok(Response::Git(GitResponse::Committed(c))) = committed else {
        panic!("{committed:?}");
    };
    assert_eq!(c.commit.summary, "Fix it");
    assert_eq!(
        f.git.calls(),
        [format!("commit {ROOT} \"Fix it\\n\\nWhy\" false")]
    );
}

#[tokio::test]
async fn branch_names_that_git_would_read_as_options_are_refused() {
    let f = fixture().await;
    for name in ["", "-f", "--orphan", "a\nb"] {
        let create = git("create_branch", json!({"name": name}));
        assert_eq!(
            code(&f.core, create).await,
            ErrorCode::BadRequest,
            "{name:?}"
        );
        let switch = git("switch", json!({"branch": name}));
        assert_eq!(
            code(&f.core, switch).await,
            ErrorCode::BadRequest,
            "{name:?}"
        );
    }
    assert!(f.git.calls().is_empty());
}

#[tokio::test]
async fn switching_waits_for_a_direct_session_but_creating_a_branch_does_not() {
    let f = fixture().await;
    f.worktrees.put(ROOT, "a.txt", "x\n");
    call(
        &f.core,
        json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "go"}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    let switch = git("switch", json!({"branch": "dev"}));
    assert_eq!(code(&f.core, switch).await, ErrorCode::Conflict);
    call(
        &f.core,
        git("create_branch", json!({"name": "dev", "switch": true})),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(f.git.calls(), [format!("create_branch {ROOT} dev true")]);
}

#[tokio::test]
async fn hunks_are_checked_before_staging() {
    let f = fixture().await;
    let hunk = |lines: serde_json::Value| {
        git(
            "stage_hunk",
            json!({"path": "a.txt", "hunk": {"old_start": 1, "old_lines": 1, "new_start": 1,
                "new_lines": 1, "header": "", "lines": lines}}),
        )
    };
    assert_eq!(code(&f.core, hunk(json!([]))).await, ErrorCode::BadRequest);
    assert_eq!(
        code(&f.core, hunk(json!(["-a", "+b\n+c"]))).await,
        ErrorCode::BadRequest
    );
    assert_eq!(
        code(&f.core, hunk(json!(["-a", "+b\rc"]))).await,
        ErrorCode::BadRequest
    );
    call(&f.core, hunk(json!(["-a\r", "+b\r"])))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    call(&f.core, hunk(json!(["-a", "+b"])))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(f.git.calls().len(), 2);
}

#[tokio::test]
async fn log_limits_are_clamped() {
    let f = fixture().await;
    for limit in [0, 10, 100_000] {
        call(&f.core, git("log", json!({"limit": limit})))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
    }
    call(&f.core, git("log", json!({})))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let want: Vec<String> = [1, 10, 200, 50]
        .iter()
        .map(|n| format!("log {ROOT} {n}"))
        .collect();
    assert_eq!(f.git.calls(), want);
}

#[tokio::test]
async fn without_a_git_service_requests_are_unavailable() {
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .build();
    call(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        code(&core, git("status", json!({}))).await,
        ErrorCode::Unavailable
    );
}
