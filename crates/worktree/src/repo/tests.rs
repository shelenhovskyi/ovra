//! `GitRepo` against real git in temporary repositories, with few git
//! commands per test.

use std::fs;

use ovra_core::protocol::git::GitOperation;
use ovra_core::services::{GitService, Result};

use super::GitRepo;
use crate::tests::{Repo, git, put};

fn ok<T>(result: Result<T>) -> T {
    result.unwrap_or_else(|e| panic!("{:?}: {}", e.code, e.message))
}

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
