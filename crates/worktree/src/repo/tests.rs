//! `GitRepo` against real git in temporary repositories, with few git
//! commands per test.

use std::fs;

use ovra_core::protocol::ErrorCode;
use ovra_core::protocol::git::{GitChange, GitOperation, GitRev, Hunk};
use ovra_core::services::{GitService, Result};

use super::GitRepo;
use crate::tests::{Repo, git, put, read, rel, rels};

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
fn numbered(lines: u32, changed: &[u32]) -> String {
    (1..=lines)
        .map(|n| match changed.contains(&n) {
            true => format!("{n} changed\n"),
            false => format!("{n}\n"),
        })
        .collect()
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

    let renamed = ok(GitRepo::new()
        .diff(root.clone(), rel("new.txt"), true)
        .await);
    assert_eq!(
        (renamed.from.as_deref(), renamed.hunks.len()),
        (Some("old.txt"), 0)
    );
}

#[tokio::test]
async fn a_project_inside_a_repository_sees_only_its_folder() {
    let repo = Repo::new(&[("proj/a.txt", &numbered(20, &[])), ("other.txt", "o\n")]);
    let root = repo.root.join("proj");
    put(&repo.root, "other.txt", "changed\n");
    put(&root, "a.txt", &numbered(20, &[2, 18]));
    let g = GitRepo::new();

    let status = ok(g.status(root.clone()).await).unwrap_or_default();
    let paths: Vec<_> = status
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.unstaged))
        .collect();
    assert_eq!(paths, [("a.txt", Some(GitChange::Modified))]);

    // Two hunks; staging the first leaves the second unstaged.
    let diff = ok(g.diff(root.clone(), rel("a.txt"), false).await);
    insta::assert_debug_snapshot!(diff);
    ok(
        g.stage_hunk(root.clone(), rel("a.txt"), diff.hunks[0].clone(), false)
            .await,
    );
    let unstaged = ok(g.diff(root.clone(), rel("a.txt"), false).await);
    assert_eq!(unstaged.hunks, [diff.hunks[1].clone()]);
    let staged = ok(g.diff(root.clone(), rel("a.txt"), true).await);
    assert_eq!(staged.hunks, [diff.hunks[0].clone()]);
    let index = ok(g.show(root.clone(), rel("a.txt"), GitRev::Index).await);
    assert_eq!(index.text, Some(numbered(20, &[2])));

    // The same hunk again no longer applies; unstaging takes it out.
    let again = g.stage_hunk(root.clone(), rel("a.txt"), diff.hunks[0].clone(), false);
    assert_eq!(code(again.await), ErrorCode::Conflict);
    // A hunk whose lines do not match its header never reaches git.
    let mut forged = diff.hunks[1].clone();
    forged.lines.push("+more".into());
    let forged = g.stage_hunk(root.clone(), rel("a.txt"), forged, false);
    assert_eq!(code(forged.await), ErrorCode::BadRequest);
    ok(
        g.stage_hunk(root.clone(), rel("a.txt"), staged.hunks[0].clone(), true)
            .await,
    );
    let head = ok(g.show(root.clone(), rel("a.txt"), GitRev::Head).await);
    let index = ok(g.show(root.clone(), rel("a.txt"), GitRev::Index).await);
    assert_eq!((head.exists, &index.text), (true, &head.text));
    assert_eq!(read(&root, "a.txt"), Some(numbered(20, &[2, 18])));
}

#[tokio::test]
async fn untracked_files_diff_as_added_and_binary_files_have_no_hunks() {
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let root = &repo.root;
    put(root, "new.txt", "x\ny");
    fs::write(root.join("bin.dat"), b"\0\x01\x02").unwrap_or_else(|e| panic!("{e}"));
    let g = GitRepo::new();

    let diff = ok(g.diff(root.clone(), rel("new.txt"), false).await);
    let hunk = &diff.hunks[..];
    assert_eq!(hunk.len(), 1);
    assert_eq!(
        (
            hunk[0].old_start,
            hunk[0].old_lines,
            hunk[0].new_start,
            hunk[0].new_lines
        ),
        (0, 0, 1, 2)
    );
    assert_eq!(hunk[0].lines, ["+x", "+y", "\\ No newline at end of file"]);

    ok(g.stage(root.clone(), rels(&["bin.dat"])).await);
    let diff = ok(g.diff(root.clone(), rel("bin.dat"), true).await);
    assert!(diff.binary && diff.hunks.is_empty());
    let index = ok(g.show(root.clone(), rel("bin.dat"), GitRev::Index).await);
    assert_eq!((index.exists, index.text), (true, None));
    let head = ok(g.show(root.clone(), rel("bin.dat"), GitRev::Head).await);
    assert_eq!((head.exists, head.text), (false, None));
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

#[tokio::test]
async fn branches_are_created_checked_and_never_switched_over_changes() {
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let root = &repo.root;
    let g = GitRepo::new();
    ok(g.create_branch(root.clone(), "plain".into(), false).await);
    assert_eq!(
        code(g.create_branch(root.clone(), "plain".into(), true).await),
        ErrorCode::Conflict
    );
    assert_eq!(
        code(
            g.create_branch(root.clone(), "bad..name".into(), false)
                .await
        ),
        ErrorCode::BadRequest
    );
    assert_eq!(
        code(g.switch(root.clone(), "missing".into()).await),
        ErrorCode::NotFound
    );

    ok(g.create_branch(root.clone(), "feature".into(), true).await);
    put(root, "a.txt", "feature\n");
    git(root, &["commit", "--quiet", "-am", "on feature"]);
    ok(g.switch(root.clone(), "main".into()).await);
    put(root, "a.txt", "mine\n");
    let refused = g
        .switch(root.clone(), "feature".into())
        .await
        .err()
        .unwrap_or_else(|| panic!("switched over a change"));
    assert_eq!(refused.code, ErrorCode::Conflict);
    assert_eq!(
        refused.message,
        "you have changes to a.txt that this branch would overwrite: \
         commit them or discard them first"
    );
    assert_eq!(read(root, "a.txt").as_deref(), Some("mine\n"));

    let branches = ok(g.branches(root.clone()).await);
    let listed: Vec<_> = branches
        .iter()
        .map(|b| (b.name.as_str(), b.current))
        .collect();
    assert_eq!(listed[0], ("feature", false), "newest commit first");
    assert!(listed.contains(&("main", true)) && listed.contains(&("plain", false)));
    assert_eq!(
        branches[0].commit.as_ref().map(|c| c.summary.as_str()),
        Some("on feature")
    );
}

#[tokio::test]
async fn blame_groups_lines_by_commit_and_leaves_new_lines_uncommitted() {
    let repo = Repo::new(&[("a.txt", "1\n2\n3\n")]);
    let root = &repo.root;
    put(root, "a.txt", "1\n2 two\n3\n");
    git(root, &["commit", "--quiet", "-am", "two"]);
    put(root, "a.txt", "1\n2 two\n3\n4\n5\n");

    let blame = ok(GitRepo::new().blame(root.clone(), rel("a.txt")).await);
    let commits: Vec<_> = blame.commits.values().map(|c| c.summary.as_str()).collect();
    let summary = |id: &Option<String>| id.as_ref().map(|id| blame.commits[id].summary.as_str());
    let ranges: Vec<_> = blame
        .ranges
        .iter()
        .map(|r| (r.start, r.lines, summary(&r.commit)))
        .collect();
    assert_eq!(
        ranges,
        [
            (1, 1, Some("start")),
            (2, 1, Some("two")),
            (3, 1, Some("start")),
            (4, 2, None)
        ]
    );
    assert_eq!(commits.len(), 2);
}

#[tokio::test]
async fn before_the_first_commit_the_branch_is_listed_and_files_unstage() {
    let repo = Repo::new(&[]);
    let root = &repo.root;
    put(root, "a.txt", "a\nb\n");
    let g = GitRepo::new();
    let branches = ok(g.branches(root.clone()).await);
    let listed: Vec<_> = branches
        .iter()
        .map(|b| (b.name.as_str(), b.current, b.commit.is_none()))
        .collect();
    assert_eq!(listed, [("main", true, true)]);
    assert!(ok(g.log(root.clone(), 5).await).is_empty());
    assert_eq!(
        code(g.create_branch(root.clone(), "x".into(), false).await),
        ErrorCode::BadRequest
    );

    ok(g.stage(root.clone(), rels(&["a.txt"])).await);
    let status = ok(g.status(root.clone()).await).unwrap_or_default();
    assert_eq!(status.head, None);
    assert_eq!(status.files[0].staged, Some(GitChange::Added));
    ok(g.unstage(root.clone(), rels(&["a.txt"])).await);
    let status = ok(g.status(root.clone()).await).unwrap_or_default();
    assert_eq!(status.files[0].unstaged, Some(GitChange::Untracked));

    let blame = ok(g.blame(root.clone(), rel("a.txt")).await);
    assert_eq!(
        (
            blame.ranges.len(),
            blame.ranges[0].lines,
            &blame.ranges[0].commit
        ),
        (1, 2, &None)
    );
}

#[test]
fn hunks_take_the_lines_their_header_counts() {
    // A removed line that looks like a header, a blank context line printed
    // empty (`diff.suppressBlankEmpty`) and a missing final newline.
    let out = "diff --git a/x b/x\n--- a/x\n+++ b/x\n\
               @@ -1,4 +1,3 @@ fn main\n--- a/y\n\n x\n-y\n\\ No newline at end of file\n+z\n\
               \\ No newline at end of file\n";
    let diff = super::diff::parse("x", None, out.as_bytes());
    let hunk = Hunk {
        old_start: 1,
        old_lines: 4,
        new_start: 1,
        new_lines: 3,
        header: "fn main".into(),
        lines: [
            "--- a/y",
            " ",
            " x",
            "-y",
            "\\ No newline at end of file",
            "+z",
            "\\ No newline at end of file",
        ]
        .map(String::from)
        .to_vec(),
    };
    assert_eq!(diff.hunks, [hunk]);
}

#[test]
fn names_git_would_quote_are_quoted_in_patches() {
    let patch = super::diff::patch(
        "dir/a \"b\"\tc.txt",
        &Hunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            header: String::new(),
            lines: vec!["-a".into(), "+b".into()],
        },
    );
    let first = patch.lines().next().unwrap_or_default();
    assert_eq!(
        first,
        "diff --git \"a/dir/a \\\"b\\\"\\tc.txt\" \"b/dir/a \\\"b\\\"\\tc.txt\""
    );
}

#[test]
fn a_refused_switch_names_a_few_files_and_says_what_to_do() {
    let refused = |files: &[&str]| {
        let list: String = files.iter().map(|f| format!("\t{f}\n")).collect();
        super::refused_switch(&format!(
            "error: Your local changes to the following files would be overwritten by checkout:\n\
             {list}Please commit your changes or stash them before you switch branches.\nAborting"
        ))
    };
    assert_eq!(
        refused(&["a", "b", "c"]),
        "you have changes to a, b and c that this branch would overwrite: \
         commit them or discard them first"
    );
    let many = refused(&["1", "2", "3", "4", "5", "6", "7"]);
    assert!(
        many.starts_with("you have changes to 1, 2, 3, 4, 5 and 2 more files that"),
        "{many}"
    );
    assert_eq!(super::refused_switch("fatal: odd"), "fatal: odd");
}
