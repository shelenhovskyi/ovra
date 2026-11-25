//! `GitRepo` against real git in temporary repositories, with few git
//! commands per test.

use std::fs;

use ovra_core::protocol::ErrorCode;
use ovra_core::protocol::git::GitOperation;
use ovra_core::services::{GitService, Result};

use super::GitRepo;
use crate::tests::{Repo, git, put, read, rels};

fn ok<T>(result: Result<T>) -> T {
    result.unwrap_or_else(|e| panic!("{:?}: {}", e.code, e.message))
}

fn code<T: std::fmt::Debug>(result: Result<T>) -> ErrorCode {
    match result {
        Ok(v) => panic!("expected an error, got {v:?}"),
        Err(e) => e.code,
    }
}

/// `lines` numbered lines, `1\n` to `<lines>\n`, with `changed` replaced.
#[tokio::test]
async fn status_lists_every_kind_of_change_with_branch_and_upstream() {
    let repo = Repo::new(&[
        ("a.txt", "a\n"),
        ("b.txt", "b\n"),
        ("old.txt", "o\n"),
        ("gone.txt", "g\n"),
    ]);
    let root = &repo.root;
    git(root, &["branch", "base"]);
    git(root, &["commit", "--quiet", "--allow-empty", "-m", "two"]);
    git(root, &["config", "branch.main.remote", "."]);
    git(root, &["config", "branch.main.merge", "refs/heads/base"]);
    put(root, "a.txt", "a2\n");
    put(root, "b.txt", "b2\n");
    git(root, &["add", "b.txt"]);
    git(root, &["mv", "old.txt", "new.txt"]);
    fs::remove_file(root.join("gone.txt")).unwrap_or_else(|e| panic!("{e}"));
    put(root, "c/d \u{e9}.txt", "u\n");
    fs::write(root.join(".git/MERGE_HEAD"), "").unwrap_or_else(|e| panic!("{e}"));

    let status = ok(GitRepo::new().status(root.clone()).await).unwrap_or_else(|| panic!("no repo"));
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.upstream.as_deref(), Some("base"));
    assert_eq!((status.ahead, status.behind, status.truncated), (1, 0, 0));
    assert_eq!(status.operation, Some(GitOperation::Merge));
    assert_eq!(status.head.map(|h| h.len()), Some(40));
    insta::assert_debug_snapshot!(status.files);
}

#[tokio::test]
async fn outside_a_repository_status_is_none_and_the_rest_not_found() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let g = GitRepo::new();
    assert_eq!(ok(g.status(dir.path().to_path_buf()).await), None);
    assert_eq!(
        code(g.log(dir.path().to_path_buf(), 5).await),
        ErrorCode::NotFound
    );
}

#[cfg(unix)]
#[tokio::test]
async fn discard_keeps_staged_changes_and_never_follows_a_symlinked_folder() {
    let repo = Repo::new(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    let root = &repo.root;
    let outside = repo.tmp.path().join("outside");
    put(&outside, "keep.txt", "mine\n");
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap_or_else(|e| panic!("{e}"));
    put(root, "a.txt", "a2\n");
    put(root, "b.txt", "staged\n");
    git(root, &["add", "b.txt"]);
    put(root, "b.txt", "unstaged\n");
    put(root, "u/new.txt", "u\n");

    let paths = rels(&["a.txt", "b.txt", "u/new.txt", "link/keep.txt"]);
    ok(GitRepo::new().discard(root.clone(), paths).await);
    assert_eq!(read(root, "a.txt").as_deref(), Some("a\n"));
    assert_eq!(read(root, "b.txt").as_deref(), Some("staged\n"));
    assert!(!root.join("u/new.txt").exists());
    assert_eq!(read(&outside, "keep.txt").as_deref(), Some("mine\n"));
}

#[tokio::test]
async fn commit_needs_staged_changes_except_to_amend() {
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let root = &repo.root;
    let g = GitRepo::new();
    assert_eq!(
        code(g.commit(root.clone(), "empty".into(), false).await),
        ErrorCode::BadRequest
    );

    put(root, "a.txt", "a2\n");
    ok(g.stage(root.clone(), rels(&["a.txt"])).await);
    let done = ok(g
        .commit(root.clone(), "Change a\n\nWhy.".into(), false)
        .await);
    assert_eq!(
        (done.commit.summary.as_str(), done.commit.author.as_str()),
        ("Change a", "Test")
    );
    assert!(done.output.contains("Change a"), "{}", done.output);

    let amended = ok(g
        .commit(root.clone(), "Change a, really".into(), true)
        .await);
    assert_ne!(amended.commit.id, done.commit.id);
    let log = ok(g.log(root.clone(), 10).await);
    let summaries: Vec<_> = log.iter().map(|c| c.summary.as_str()).collect();
    assert_eq!(summaries, ["Change a, really", "start"]);
    assert_eq!(log[0].id, amended.commit.id);
}

#[cfg(unix)]
#[tokio::test]
async fn a_failing_hook_is_a_conflict_with_its_output() {
    use std::os::unix::fs::PermissionsExt as _;
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let root = &repo.root;
    // `core.hooksPath` of the test repositories.
    let hook = root.join("no-hooks/pre-commit");
    put(
        root,
        "no-hooks/pre-commit",
        "#!/bin/sh\necho 'lint: 3 problems' >&2\nexit 1\n",
    );
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap_or_else(|e| panic!("{e}"));
    put(root, "a.txt", "a2\n");
    git(root, &["add", "a.txt"]);

    let err = GitRepo::new()
        .commit(root.clone(), "Change".into(), false)
        .await;
    let err = err
        .err()
        .unwrap_or_else(|| panic!("the hook let it through"));
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(err.message.contains("lint: 3 problems"), "{}", err.message);
}
