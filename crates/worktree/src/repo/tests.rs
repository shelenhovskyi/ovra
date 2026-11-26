//! `GitRepo` against real git in temporary repositories, with few git
//! commands per test.

use std::fs;
use std::path::PathBuf;

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

#[tokio::test]
async fn init_sets_up_a_repository_only_where_there_is_none() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let g = GitRepo::new();
    let root = dir.path().join("proj");
    put(&root, "a.txt", "a\n");
    // A project root may come with a trailing slash.
    ok(g.init(PathBuf::from(format!("{}/", root.display()))).await);
    let status = ok(g.status(root.clone()).await).unwrap_or_else(|| panic!("no repo"));
    assert_eq!((status.head, status.files.len()), (None, 1));

    assert_eq!(code(g.init(root.clone()).await), ErrorCode::Conflict);
    fs::create_dir(root.join("sub")).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(code(g.init(root.join("sub")).await), ErrorCode::Conflict);
    assert!(!root.join("sub/.git").exists());

    // A `.git` git cannot read is left alone.
    let broken = dir.path().join("broken");
    put(&broken, ".git", "not a gitfile\n");
    assert_eq!(code(g.init(broken.clone()).await), ErrorCode::Conflict);
    assert_eq!(read(&broken, ".git").as_deref(), Some("not a gitfile\n"));
    assert_eq!(
        code(g.init(dir.path().join("gone")).await),
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

    // An empty message keeps the whole message, body included.
    put(root, "a.txt", "a3\n");
    ok(g.stage(root.clone(), rels(&["a.txt"])).await);
    ok(g.commit(root.clone(), "Keep\n\nThe body.".into(), true)
        .await);
    ok(g.stage(root.clone(), rels(&["a.txt"])).await);
    ok(g.commit(root.clone(), String::new(), true).await);
    assert_eq!(
        git(root, &["log", "-1", "--format=%B"]),
        "Keep\n\nThe body."
    );
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

/// An executable hook `name` in the test repositories' `core.hooksPath`.
#[cfg(unix)]
fn hook(root: &std::path::Path, name: &str, script: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    let path = format!("no-hooks/{name}");
    put(root, &path, &format!("#!/bin/sh\n{script}"));
    fs::set_permissions(root.join(&path), fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| panic!("{e}"));
}

#[cfg(unix)]
#[tokio::test]
async fn hooks_see_the_persons_git_not_ovras_options() {
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let root = &repo.root;
    let seen = repo.tmp.path().join("seen");
    let record = format!(
        "echo \"$GIT_LITERAL_PATHSPECS|$GIT_CONFIG_PARAMETERS\" >> '{}'\n",
        seen.display()
    );
    hook(root, "pre-commit", &record);
    hook(root, "post-checkout", &record);
    put(root, "a.txt", "a2\n");
    git(root, &["add", "a.txt"]);
    let g = GitRepo::new();
    ok(g.commit(root.clone(), "Change".into(), false).await);
    ok(g.create_branch(root.clone(), "dev".into(), true).await);
    ok(g.switch(root.clone(), "main".into()).await);

    let seen = fs::read_to_string(seen).unwrap_or_else(|e| panic!("{e}"));
    let lines: Vec<&str> = seen.lines().collect();
    assert_eq!(lines.len(), 3, "{seen}");
    for line in lines {
        let (literal, config) = line.split_once('|').unwrap_or_default();
        assert_eq!(literal, "", "{line}");
        assert!(!config.contains("quotepath"), "{line}");
        // What keeps repository content from running stays.
        assert!(config.contains("'core.fsmonitor'='false'"), "{line}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_hanging_hook_is_ended_after_the_limit_with_its_output() {
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let root = &repo.root;
    hook(
        root,
        "pre-commit",
        "echo 'Enter passphrase:' >&2\nsleep 30\n",
    );
    put(root, "a.txt", "a2\n");
    git(root, &["add", "a.txt"]);

    // A busy machine may stop git before the hook has printed anything:
    // a longer limit then, until its output is in the message.
    let mut message = String::new();
    for limit in [50, 200, 800] {
        let err = GitRepo::with_limit(std::time::Duration::from_millis(limit))
            .commit(root.clone(), "Change".into(), false)
            .await
            .err()
            .unwrap_or_else(|| panic!("committed through a hanging hook"));
        assert_eq!(err.code, ErrorCode::Conflict);
        assert!(
            err.message
                .starts_with("git commit was still running after"),
            "{}",
            err.message
        );
        message = err.message;
        if message.contains("Enter passphrase:") {
            break;
        }
    }
    assert!(message.contains("Enter passphrase:"), "{message}");
    // git was asked to end, so it removed its lock.
    assert!(!root.join(".git/index.lock").exists());
    assert_eq!(git(root, &["log", "-1", "--format=%s"]), "start");
}

#[cfg(unix)]
#[tokio::test]
async fn cancel_stops_a_running_commit_and_its_hook() {
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let root = &repo.root;
    let started = repo.tmp.path().join("started");
    hook(
        root,
        "pre-commit",
        &format!("touch '{}'\nsleep 30\n", started.display()),
    );
    put(root, "a.txt", "a2\n");
    git(root, &["add", "a.txt"]);

    let g = std::sync::Arc::new(GitRepo::new());
    let commit = tokio::spawn({
        let (g, root) = (g.clone(), root.clone());
        async move { g.commit(root, "Change".into(), false).await }
    });
    let waiting = std::time::Instant::now();
    while !started.exists() {
        assert!(waiting.elapsed().as_secs() < 2, "the hook never started");
        tokio::task::yield_now().await;
    }
    ok(g.cancel(root.clone()).await);
    let err = commit
        .await
        .unwrap_or_else(|e| panic!("{e}"))
        .err()
        .unwrap_or_else(|| panic!("the commit was not stopped"));
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(err.message, "git commit was stopped");
    assert_eq!(git(root, &["log", "-1", "--format=%s"]), "start");
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
async fn blame_says_why_it_failed_and_shows_new_files_as_uncommitted() {
    let repo = Repo::new(&[("a.txt", "1\n")]);
    let root = &repo.root;
    put(root, "new.txt", "1\n2\n");
    let g = GitRepo::new();
    let new = ok(g.blame(root.clone(), rel("new.txt")).await);
    let ranges: Vec<_> = new
        .ranges
        .iter()
        .map(|r| (r.start, r.lines, r.commit.clone()))
        .collect();
    assert_eq!(ranges, [(1, 2, None)]);
    assert_eq!(
        code(g.blame(root.clone(), rel("gone.txt")).await),
        ErrorCode::NotFound
    );

    // A committed file whose blame fails is not shown as all uncommitted.
    git(root, &["config", "blame.ignoreRevsFile", "missing-revs"]);
    let err = g
        .blame(root.clone(), rel("a.txt"))
        .await
        .err()
        .unwrap_or_else(|| panic!("blame hid the failure"));
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(err.message.contains("missing-revs"), "{}", err.message);
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

#[tokio::test]
async fn pulled_lists_the_commits_a_merge_brought_with_bodies_and_files() {
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let root = &repo.root;
    let g = GitRepo::new();
    // No ORIG_HEAD yet: nothing to summarize.
    let none = ok(g.pulled(root.clone()).await);
    assert_eq!((none.from, none.commits.len()), (None, 0));

    let start = git(root, &["rev-parse", "HEAD"]);
    git(root, &["switch", "--quiet", "-c", "other"]);
    put(root, "a.txt", "a\nb\n");
    put(root, "bin.dat", "\0\x01");
    git(root, &["add", "--all"]);
    git(
        root,
        &["commit", "--quiet", "-m", "Add b\n\nBecause.\nTwo lines."],
    );
    put(root, "c.txt", "c\n");
    git(root, &["add", "--all"]);
    git(root, &["commit", "--quiet", "-m", "Add c"]);
    git(root, &["switch", "--quiet", "main"]);
    git(root, &["merge", "--quiet", "--ff-only", "other"]);

    let pulled = ok(g.pulled(root.clone()).await);
    assert_eq!(pulled.from.as_deref(), Some(start.as_str()));
    assert_eq!(pulled.truncated, 0);
    let shown: Vec<_> = pulled
        .commits
        .iter()
        .map(|c| {
            let files: Vec<_> = c
                .files
                .iter()
                .map(|f| (f.path.as_str(), f.added, f.removed))
                .collect();
            (c.commit.summary.as_str(), c.body.as_str(), files)
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("Add c", "", vec![("c.txt", Some(1), Some(0))]),
            (
                "Add b",
                "Because.\nTwo lines.",
                vec![("a.txt", Some(1), Some(0)), ("bin.dat", None, None)]
            ),
        ]
    );
}

#[tokio::test]
async fn pulled_in_a_project_folder_lists_only_its_commits_and_paths() {
    let repo = Repo::new(&[("sub/a.txt", "a\n"), ("other.txt", "o\n")]);
    let root = &repo.root;
    let start = git(root, &["rev-parse", "HEAD"]);
    put(root, "other.txt", "o2\n");
    git(root, &["commit", "--quiet", "-am", "Outside only"]);
    put(root, "sub/a.txt", "a2\n");
    git(root, &["commit", "--quiet", "-am", "Inside"]);
    git(root, &["reset", "--quiet", "--hard", &start]);
    git(root, &["merge", "--quiet", "--ff-only", "HEAD@{1}"]);

    let pulled = ok(GitRepo::new().pulled(root.join("sub")).await);
    let shown: Vec<_> = pulled
        .commits
        .iter()
        .map(|c| (c.commit.summary.as_str(), c.files[0].path.as_str()))
        .collect();
    assert_eq!(shown, [("Inside", "a.txt")]);
    assert_eq!(pulled.truncated, 0);
}

#[tokio::test]
async fn the_upstream_is_read_from_config_recorded_and_only_fast_forwarded() {
    use ovra_core::services::{Ancestry, RemoteBranch};
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let root = &repo.root;
    let g = GitRepo::new();
    let id = |rev: &str| git(root, &["rev-parse", rev]).trim().to_owned();
    // A local upstream is no remote.
    git(root, &["branch", "base"]);
    git(root, &["branch", "--set-upstream-to=base"]);
    assert_eq!(ok(g.upstream(root.clone()).await), None);
    // A remote whose name has a slash, tracking a branch of another name.
    git(
        root,
        &["remote", "add", "team/x", "https://example.invalid/r.git"],
    );
    git(
        root,
        &[
            "config",
            "remote.team/x.fetch",
            "+refs/heads/*:refs/remotes/t/*",
        ],
    );
    git(root, &["update-ref", "refs/remotes/t/trunk", "HEAD"]);
    git(root, &["config", "branch.main.remote", "team/x"]);
    git(root, &["config", "branch.main.merge", "refs/heads/trunk"]);
    let found = RemoteBranch {
        remote: "team/x".into(),
        branch: "trunk".into(),
    };
    assert_eq!(ok(g.upstream(root.clone()).await), Some(found.clone()));
    let f = || found.clone();

    let first = id("HEAD");
    // What the remote gained: two commits on top of HEAD, made elsewhere.
    git(root, &["switch", "--quiet", "-c", "elsewhere"]);
    put(root, "a.txt", "b\n");
    git(root, &["commit", "--quiet", "-am", "b"]);
    git(root, &["commit", "--quiet", "--allow-empty", "-m", "c"]);
    let there = id("HEAD");
    git(root, &["switch", "--quiet", "main"]);

    let ancestry = ok(g.record_upstream(root.clone(), f(), there.clone()).await);
    assert_eq!((ancestry, id("t/trunk")), (Ancestry::Behind, there.clone()));
    // Changes the fast-forward would overwrite: refused, nothing moves.
    put(root, "a.txt", "mine\n");
    let refused = g.fast_forward(root.clone(), f(), there.clone()).await;
    assert_eq!(code(refused), ErrorCode::Conflict);
    assert_eq!(
        (id("HEAD"), read(root, "a.txt").as_deref()),
        (first.clone(), Some("mine\n"))
    );
    put(root, "a.txt", "a\n");
    ok(g.fast_forward(root.clone(), f(), there.clone()).await);
    assert_eq!(
        (id("HEAD"), read(root, "a.txt").as_deref()),
        (there.clone(), Some("b\n"))
    );
    // After a push: the remote is where HEAD is.
    assert_eq!(
        ok(g.record_upstream(root.clone(), f(), first.clone()).await),
        Ancestry::Has
    );

    // Both sides moved.
    git(root, &["switch", "--quiet", "-c", "other", &first]);
    git(root, &["commit", "--quiet", "--allow-empty", "-m", "other"]);
    let other = id("HEAD");
    git(root, &["switch", "--quiet", "main"]);
    assert_eq!(
        ok(g.record_upstream(root.clone(), f(), other.clone()).await),
        Ancestry::Diverged
    );
    assert_eq!(
        code(g.fast_forward(root.clone(), f(), other).await),
        ErrorCode::Conflict
    );
    assert_eq!(
        code(g.record_upstream(root.clone(), f(), "HEAD".into()).await),
        ErrorCode::BadRequest
    );
    // Switched meanwhile to a branch that follows something else: refused.
    git(root, &["switch", "--quiet", "other"]);
    let moved = g.record_upstream(root.clone(), f(), there.clone()).await;
    assert_eq!(code(moved), ErrorCode::Conflict);
    assert_eq!(
        code(g.fast_forward(root.clone(), f(), there).await),
        ErrorCode::Conflict
    );
}
