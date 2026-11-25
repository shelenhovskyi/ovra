//! Against real git in temporary repositories. Paths have spaces and
//! non-ASCII letters, and worktrees live outside the repository, as they do
//! under Ovra's data directory. Each test runs few git commands: on Windows
//! every one costs tens of milliseconds.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use ovra_core::path::RelPath;
use ovra_core::protocol::ErrorCode;
use ovra_core::protocol::review::ChangeKind;
use ovra_core::services::Worktrees;
use tempfile::TempDir;

use super::GitWorktrees;

const BRANCH: &str = "ovra/t1/worker";

fn rel(path: &str) -> RelPath {
    RelPath::parse(path).unwrap_or_else(|e| panic!("{e}"))
}

fn rels(paths: &[&str]) -> Vec<RelPath> {
    paths.iter().map(|p| rel(p)).collect()
}

fn put(dir: &Path, path: &str, text: &str) {
    let path = dir.join(path);
    fs::create_dir_all(path.parent().unwrap_or(dir)).unwrap_or_else(|e| panic!("{e}"));
    fs::write(path, text).unwrap_or_else(|e| panic!("{e}"));
}

fn read(dir: &Path, path: &str) -> Option<String> {
    fs::read_to_string(dir.join(path)).ok()
}

/// Runs git for checks and setup; the repository's own config makes it
/// independent of the machine's.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new(crate::git::program())
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("git: {e}"));
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    text
}

fn git_fails(dir: &Path, args: &[&str]) -> bool {
    let status = Command::new(crate::git::program())
        .args(args)
        .current_dir(dir)
        .output();
    status.map(|o| !o.status.success()).unwrap_or(false)
}

struct Repo {
    tmp: TempDir,
    root: PathBuf,
}

impl Repo {
    /// A repository with `files` committed, or no commit if empty.
    fn new(files: &[(&str, &str)]) -> Self {
        let tmp = tempfile::Builder::new()
            .prefix("ovra wt \u{e9} ")
            .tempdir()
            .unwrap_or_else(|e| panic!("{e}"));
        let root = tmp.path().join("main checkout");
        fs::create_dir_all(&root).unwrap_or_else(|e| panic!("{e}"));
        git(&root, &["init", "--quiet", "--initial-branch=main"]);
        let config = root.join(".git").join("config");
        let mut text = fs::read_to_string(&config).unwrap_or_default();
        text.push_str(
            "[user]\n\tname = Test\n\temail = test@example.com\n\
             [commit]\n\tgpgsign = false\n\
             [core]\n\tautocrlf = false\n\thooksPath = no-hooks\n",
        );
        fs::write(&config, text).unwrap_or_else(|e| panic!("{e}"));
        for (path, text) in files {
            put(&root, path, text);
        }
        if !files.is_empty() {
            git(&root, &["add", "--all"]);
            git(&root, &["commit", "--quiet", "-m", "start"]);
        }
        Self { tmp, root }
    }

    /// Where a worker's worktree goes: outside the repository, in folders
    /// that do not exist yet.
    fn worktree_dir(&self) -> PathBuf {
        self.tmp
            .path()
            .join("data dir")
            .join("worktrees")
            .join("t1 w\u{e9}")
    }

    /// A snapshot of the main checkout and a worker worktree from it.
    async fn worker(&self, w: &GitWorktrees) -> (String, PathBuf) {
        let base = w.snapshot(self.root.clone()).await;
        let base = base.unwrap_or_else(|e| panic!("{e:?}"));
        let dir = self.worktree_dir();
        let info = w
            .create(self.root.clone(), dir.clone(), BRANCH.into(), base.clone())
            .await
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            (info.path, info.branch, &info.base),
            (dir.clone(), BRANCH.into(), &base)
        );
        (base, dir)
    }
}

#[tokio::test]
async fn a_snapshot_records_the_working_tree_and_touches_nothing() {
    let repo = Repo::new(&[
        ("a.txt", "one\n"),
        ("b.txt", "b\n"),
        (".gitignore", "*.log\n"),
    ]);
    let root = &repo.root;
    put(root, "b.txt", "staged\n");
    git(root, &["add", "b.txt"]);
    put(root, "a.txt", "two\n");
    put(root, "c/new.txt", "new\n");
    put(root, "x.log", "ignored\n");
    let index = fs::read(root.join(".git/index")).unwrap_or_default();
    let head = git(root, &["rev-parse", "HEAD"]);

    let snap = GitWorktrees::new().snapshot(root.clone()).await;
    let snap = snap.unwrap_or_else(|e| panic!("{e:?}"));

    assert_eq!(fs::read(root.join(".git/index")).unwrap_or_default(), index);
    let refs = git(
        root,
        &[
            "rev-parse",
            "HEAD",
            &format!("{snap}^"),
            "refs/ovra/snapshot",
        ],
    );
    assert_eq!(refs, format!("{head}\n{head}\n{snap}"));
    assert!(git_fails(
        root,
        &["rev-parse", "--verify", "--quiet", "refs/stash"]
    ));
    let files = git(root, &["ls-tree", "-r", "--name-only", &snap]);
    assert_eq!(files, ".gitignore\na.txt\nb.txt\nc/new.txt");
    assert_eq!(git(root, &["show", &format!("{snap}:a.txt")]), "two");
    let left: Vec<_> = fs::read_dir(root.join(".git"))
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(
        !left
            .iter()
            .any(|n| n.to_string_lossy().starts_with("ovra-index-")),
        "{left:?}"
    );
}

#[tokio::test]
async fn later_snapshots_see_every_change_since_the_last() {
    let repo = Repo::new(&[("a.txt", "one\n"), (".gitignore", "*.log\n")]);
    let root = &repo.root;
    let w = GitWorktrees::new();
    put(root, "new.txt", "new\n");
    put(root, "build.out", "generated\n");
    let first = w.snapshot(root.clone()).await;
    let first = first.unwrap_or_else(|e| panic!("{e:?}"));
    assert!(root.join(".git/ovra-index").is_file());

    // Within the same second, same sizes: git must look at the contents.
    put(root, "a.txt", "two\n");
    put(root, "new.txt", "NEW\n");
    fs::remove_file(root.join("build.out")).unwrap_or_else(|e| panic!("{e}"));
    put(root, "build.out", "kept out\n");
    put(root, ".gitignore", "*.log\n*.out\n");
    let second = w.snapshot(root.clone()).await;
    let second = second.unwrap_or_else(|e| panic!("{e:?}"));

    let show = |rev: &str, path: &str| git(root, &["show", &format!("{rev}:{path}")]);
    assert_eq!(
        (show(&first, "a.txt"), show(&first, "build.out")),
        ("one".into(), "generated".into())
    );
    assert_eq!(
        (show(&second, "a.txt"), show(&second, "new.txt")),
        ("two".into(), "NEW".into())
    );
    // A file ignored since the last snapshot is no longer in it.
    let files = git(root, &["ls-tree", "--name-only", &second]);
    assert_eq!(files, ".gitignore\na.txt\nnew.txt");

    // Between the two snapshots, without reading the working tree.
    let diff = w.diff(root.clone(), first, second).await;
    let diff = diff.unwrap_or_else(|e| panic!("{e:?}"));
    let summary: Vec<_> = diff
        .iter()
        .map(|d| (d.path.as_str(), d.kind, d.additions, d.deletions))
        .collect();
    assert_eq!(
        summary,
        [
            (".gitignore", ChangeKind::Modified, 1, 0),
            ("a.txt", ChangeKind::Modified, 1, 1),
            ("build.out", ChangeKind::Deleted, 0, 1),
            ("new.txt", ChangeKind::Modified, 1, 1),
        ]
    );
}

/// Git never looks inside a submodule an agent may have changed, nor
/// recurses into one: that would run the submodule's config.
#[test]
fn git_stays_out_of_submodules() {
    let repo = Repo::new(&[]);
    let root = &repo.root;
    let sub = root.join("sub");
    put(&sub, "x.txt", "x\n");
    git(&sub, &["init", "--quiet"]);
    git(&sub, &["add", "--all"]);
    git(
        &sub,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "sub",
        ],
    );
    git(root, &["add", "sub"]);
    git(root, &["commit", "--quiet", "-m", "start"]);
    put(&sub, "x.txt", "changed\n");
    assert_eq!(git(root, &["status", "--porcelain"]), "M sub");

    let status = crate::git::Git::new(root, "status")
        .arg("--porcelain")
        .text();
    assert_eq!(status.ok().as_deref(), Some(""));
    let args = format!("{:?}", crate::git::Git::new(root, "reset"));
    assert!(args.contains(r#""-c" "submodule.recurse=false""#), "{args}");
    assert!(
        args.contains(r#""-c" "diff.ignoreSubmodules=all""#),
        "{args}"
    );
}

#[test]
fn head_is_read_like_git_reads_it() {
    let repo = Repo::new(&[]);
    let root = &repo.root;
    assert_eq!(crate::git::head(root).ok(), Some(None));
    put(root, "a.txt", "a\n");
    git(root, &["add", "--all"]);
    git(root, &["commit", "--quiet", "-m", "one"]);
    let head = || crate::git::head(root).ok().flatten();
    let parsed = || git(root, &["rev-parse", "HEAD"]);
    assert_eq!(head(), Some(parsed()));
    git(root, &["pack-refs", "--all"]);
    assert_eq!(head(), Some(parsed()));
    git(root, &["checkout", "--quiet", "--detach"]);
    assert_eq!(head(), Some(parsed()));
}

#[tokio::test]
async fn a_snapshot_works_before_the_first_commit() {
    let repo = Repo::new(&[]);
    put(&repo.root, "first.txt", "hi\n");
    let snap = GitWorktrees::new().snapshot(repo.root.clone()).await;
    let snap = snap.unwrap_or_else(|e| panic!("{e:?}"));
    let parents = git(&repo.root, &["rev-list", "--parents", "-n1", &snap]);
    assert_eq!(parents, snap);
    assert_eq!(
        git(&repo.root, &["ls-tree", "--name-only", &snap]),
        "first.txt"
    );
}

#[tokio::test]
async fn a_worker_sees_its_changes_against_the_base() {
    let long: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    let repo = Repo::new(&[
        ("a.rs", "fn a() {}\nfn b() {}\n"),
        ("b.rs", "gone\n"),
        ("dir/old name.txt", &long),
        ("bin.dat", "\0\x01\x02"),
        (".gitignore", "*.log\n"),
    ]);
    let w = GitWorktrees::new();
    // Work in progress in the main checkout is part of the base.
    put(&repo.root, "wip.txt", "wip\n");
    let (base, wt) = repo.worker(&w).await;
    assert_eq!(read(&wt, "wip.txt").as_deref(), Some("wip\n"));
    assert_eq!(git(&repo.root, &["rev-parse", BRANCH]), base);

    put(&wt, "a.rs", "fn a() {}\nfn c() {}\nfn d() {}\n");
    fs::remove_file(wt.join("b.rs")).unwrap_or_else(|e| panic!("{e}"));
    fs::create_dir_all(wt.join("dir")).unwrap_or_else(|e| panic!("{e}"));
    fs::rename(wt.join("dir/old name.txt"), wt.join("dir/new \u{e9}.txt"))
        .unwrap_or_else(|e| panic!("{e}"));
    put(&wt, "bin.dat", "\0\x03");
    put(&wt, "c.rs", "new\n");
    put(&wt, "debug.log", "ignored\n");

    let changes = w.changes(wt.clone(), base.clone()).await;
    let summary: Vec<String> = changes
        .unwrap_or_else(|e| panic!("{e:?}"))
        .iter()
        .map(|d| {
            let from = d
                .from
                .as_deref()
                .map(|f| format!(" from {f}"))
                .unwrap_or_default();
            format!(
                "{:?} {}{from} +{} -{}",
                d.kind, d.path, d.additions, d.deletions
            )
        })
        .collect();
    insta::assert_snapshot!(summary.join("\n"));
}

#[tokio::test]
async fn a_patch_shows_exactly_the_files_asked_for_with_their_renames() {
    let long: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    let repo = Repo::new(&[
        ("a.rs", "fn a() {}\nfn b() {}\n"),
        ("old name.txt", &long),
        ("gone.txt", "gone\n"),
        ("kind", "a file\n"),
        ("same.txt", "same\n"),
    ]);
    let root = &repo.root;
    put(root, "a.rs", "fn a() {}\nfn c() {}\n");
    #[cfg(unix)]
    {
        fs::remove_file(root.join("kind")).unwrap_or_else(|e| panic!("{e}"));
        std::os::unix::fs::symlink("a.rs", root.join("kind")).unwrap_or_else(|e| panic!("{e}"));
    }
    fs::rename(root.join("old name.txt"), root.join("new \u{e9}.txt"))
        .unwrap_or_else(|e| panic!("{e}"));
    fs::remove_file(root.join("gone.txt")).unwrap_or_else(|e| panic!("{e}"));
    put(root, "untouched.txt", "not asked for\n");
    let w = GitWorktrees::new();

    let patch = w.patch(root.clone(), "HEAD".into(), rels(&["a.rs"])).await;
    let patch = patch.unwrap_or_else(|e| panic!("{e:?}"));
    assert!(patch.contains("-fn b() {}\n+fn c() {}\n"), "{patch}");
    assert_eq!(patch.matches("diff --git").count(), 1, "{patch}");

    let paths = rels(&["new \u{e9}.txt", "gone.txt"]);
    let patch = w.patch(root.clone(), "HEAD".into(), paths).await;
    let patch = patch.unwrap_or_else(|e| panic!("{e:?}"));
    assert!(
        patch.contains("rename from old name.txt\nrename to new \u{e9}.txt\n"),
        "{patch}"
    );
    assert!(patch.contains("deleted file mode"), "{patch}");
    assert_eq!(patch.matches("diff --git").count(), 2, "{patch}");

    // Each file's part of one diff of the working tree: the parts make up
    // the diff of them all.
    let all = ["a.rs", "new \u{e9}.txt", "gone.txt", "kind", "same.txt"];
    let now = w
        .look(root.clone())
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    let patches = w
        .patches(root.clone(), "HEAD".into(), now, rels(&all))
        .await;
    let patches = patches.unwrap_or_else(|e| panic!("{e:?}"));
    let whole = w.patch(root.clone(), "HEAD".into(), rels(&all)).await;
    let whole = whole.unwrap_or_else(|e| panic!("{e:?}"));
    let keys: Vec<&str> = patches.keys().map(String::as_str).collect();
    let mut sorted = all.to_vec();
    sorted.sort_unstable();
    assert_eq!(keys, sorted);
    for (path, patch) in &patches {
        assert!(whole.contains(patch.as_str()), "{path}: {patch}");
    }
    let parts: usize = patches.values().map(String::len).sum();
    assert_eq!(parts, whole.len());
    let rename = patches
        .get("new \u{e9}.txt")
        .map(String::as_str)
        .unwrap_or_default();
    assert!(rename.contains("rename from old name.txt\n"), "{rename}");
    #[cfg(unix)]
    {
        let kind = patches.get("kind").map(String::as_str).unwrap_or_default();
        assert_eq!(kind.matches("diff --git").count(), 2, "{kind}");
    }
    assert_eq!(patches.get("same.txt").map(String::as_str), Some(""));
}

#[tokio::test]
async fn approved_work_applies_unless_the_person_changed_the_file() {
    let repo = Repo::new(&[("a.txt", "a\n"), ("b.txt", "b\n"), ("old.txt", "old\n")]);
    let w = GitWorktrees::new();
    let (base, wt) = repo.worker(&w).await;
    put(&wt, "a.txt", "agent\n");
    put(&wt, "b.txt", "agent\n");
    put(&wt, "sub/new.txt", "new\n");
    fs::remove_file(wt.join("old.txt")).unwrap_or_else(|e| panic!("{e}"));
    // Meanwhile the person edits b.txt in the main checkout.
    put(&repo.root, "b.txt", "mine\n");

    // What the person reviewed; the worker writes on after it.
    let reviewed = w.look(wt.clone()).await.unwrap_or_else(|e| panic!("{e:?}"));
    put(&wt, "a.txt", "written after the review\n");
    put(&wt, "old.txt", "back after the review\n");

    let paths = rels(&["a.txt", "b.txt", "sub/new.txt", "old.txt"]);
    let conflicts = w
        .apply(reviewed, base.clone(), repo.root.clone(), paths)
        .await;
    assert_eq!(conflicts.ok(), Some(vec!["b.txt".to_owned()]));
    let root = &repo.root;
    assert_eq!(read(root, "a.txt").as_deref(), Some("agent\n"));
    assert_eq!(read(root, "b.txt").as_deref(), Some("mine\n"));
    assert_eq!(read(root, "sub/new.txt").as_deref(), Some("new\n"));
    assert_eq!(read(root, "old.txt"), None);
    // Nothing was staged.
    assert_eq!(git(root, &["diff", "--cached", "--name-only"]), "");

    let restored = w.restore(wt.clone(), base, rels(&["a.txt", "sub/new.txt", "old.txt"]));
    assert!(restored.await.is_ok());
    assert_eq!(read(&wt, "a.txt").as_deref(), Some("a\n"));
    assert_eq!(read(&wt, "old.txt").as_deref(), Some("old\n"));
    assert_eq!(read(&wt, "sub/new.txt"), None);
    // Deleting a file leaves its folder.
    assert!(wt.join("sub").is_dir());

    assert!(w.remove(root.clone(), wt.clone()).await.is_ok());
    assert!(!wt.exists());
    git(root, &["rev-parse", "--verify", "--quiet", BRANCH]);
    // A worktree that is already gone is pruned.
    assert!(w.remove(root.clone(), wt).await.is_ok());
}

#[tokio::test]
async fn worktrees_are_locked_until_removed_even_when_their_folder_is_gone() {
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let w = GitWorktrees::new();
    let (_, wt) = repo.worker(&w).await;
    let root = &repo.root;
    let listed = git(root, &["worktree", "list", "--porcelain"]);
    assert!(listed.contains("locked Ovra agent session"), "{listed}");
    assert!(git_fails(
        root,
        &["worktree", "remove", "--force", &wt.to_string_lossy()]
    ));

    // The folder disappears (an unmounted disk, say): git keeps the worktree.
    fs::remove_dir_all(&wt).unwrap_or_else(|e| panic!("{e}"));
    git(root, &["worktree", "prune"]);
    assert_eq!(git(root, &["worktree", "list"]).lines().count(), 2);

    assert!(w.remove(root.clone(), wt).await.is_ok());
    assert_eq!(git(root, &["worktree", "list"]).lines().count(), 1);
}

#[tokio::test]
async fn restore_touches_only_the_files_named() {
    let repo = Repo::new(&[("d/f.txt", "f\n"), ("keep.txt", "keep\n")]);
    let root = &repo.root;
    let w = GitWorktrees::new();
    let base = git(root, &["rev-parse", "HEAD"]);
    put(root, "d/f.txt", "edited\n");
    put(root, "x/inside.txt", "mine\n");
    put(root, "keep.txt", "edited\n");
    fs::remove_file(root.join("keep.txt")).unwrap_or_else(|e| panic!("{e}"));
    let index = fs::read(root.join(".git/index")).unwrap_or_default();

    // `d` is a folder in the base and `x` one on disk: both are skipped.
    let paths = rels(&["d", "x", "keep.txt"]);
    assert!(w.restore(root.clone(), base.clone(), paths).await.is_ok());
    assert_eq!(read(root, "d/f.txt").as_deref(), Some("edited\n"));
    assert_eq!(read(root, "x/inside.txt").as_deref(), Some("mine\n"));
    assert_eq!(read(root, "keep.txt").as_deref(), Some("keep\n"));
    assert_eq!(fs::read(root.join(".git/index")).unwrap_or_default(), index);

    #[cfg(unix)]
    {
        // Never delete through a symlinked folder.
        let outside = repo.tmp.path().join("outside");
        put(&outside, "f.txt", "outside\n");
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            w.restore(root.clone(), base, rels(&["link/f.txt"]))
                .await
                .is_ok()
        );
        assert_eq!(read(&outside, "f.txt").as_deref(), Some("outside\n"));
    }
}

#[tokio::test]
async fn commit_takes_exactly_the_files_named() {
    let repo = Repo::new(&[("a.txt", "a\n"), ("b.txt", "b\n"), ("gone.txt", "gone\n")]);
    let root = &repo.root;
    let w = GitWorktrees::new();
    let head = git(root, &["rev-parse", "HEAD"]);
    put(root, "a.txt", "agent\n");
    put(root, "b.txt", "person\n");
    git(root, &["add", "b.txt"]);
    put(root, "new.txt", "new\n");
    fs::remove_file(root.join("gone.txt")).unwrap_or_else(|e| panic!("{e}"));

    let paths = rels(&["a.txt", "new.txt", "gone.txt"]);
    let id = w
        .commit(root.clone(), paths.clone(), "Agent work\n\nDetails.".into())
        .await;
    let id = id.unwrap_or_else(|e| panic!("{e:?}"));

    let shown = git(
        root,
        &["show", "--name-status", "--format=%H %P%n%B", "HEAD"],
    );
    assert_eq!(
        shown,
        format!("{id} {head}\nAgent work\n\nDetails.\n\nM\ta.txt\nD\tgone.txt\nA\tnew.txt")
    );
    // What the person staged is still staged, and nothing else differs.
    assert_eq!(git(root, &["status", "--porcelain"]), "M  b.txt");

    let again = w.commit(root.clone(), paths, "Again".into()).await;
    assert_eq!(again.err().map(|e| e.code), Some(ErrorCode::Conflict));
}

#[tokio::test]
async fn create_refuses_taken_names_and_unknown_bases() {
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let w = GitWorktrees::new();
    let dir = repo.worktree_dir();
    let code = |r: Result<_, ovra_core::protocol::ProtocolError>| r.err().map(|e| e.code);

    let taken = w.create(repo.root.clone(), dir.clone(), "main".into(), "HEAD".into());
    assert_eq!(code(taken.await), Some(ErrorCode::Conflict));
    let unknown = w.create(repo.root.clone(), dir.clone(), BRANCH.into(), "nope".into());
    assert_eq!(code(unknown.await), Some(ErrorCode::NotFound));
    let exists = w.create(
        repo.root.clone(),
        repo.root.clone(),
        BRANCH.into(),
        "HEAD".into(),
    );
    assert_eq!(code(exists.await), Some(ErrorCode::Conflict));
    assert!(!dir.exists());
}

/// A snapshot-like commit from 2020, logged in the snapshot ref.
fn old_snapshot(root: &Path, message: &str) -> String {
    let tree = git(root, &["write-tree"]);
    let out = Command::new(crate::git::program())
        .args(["commit-tree", &tree, "-m", message])
        .env("GIT_COMMITTER_DATE", "2020-01-01T00:00:00Z")
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| panic!("git: {e}"));
    let id = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    git(
        root,
        &[
            "update-ref",
            "--create-reflog",
            "-m",
            "old",
            "refs/ovra/snapshot",
            &id,
        ],
    );
    id
}

#[tokio::test]
async fn pruning_keeps_the_snapshots_asked_for_and_the_young_ones() {
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let root = &repo.root;
    let w = GitWorktrees::new();
    // Nothing to prune before the first snapshot.
    assert!(w.prune_snapshots(root.clone(), Vec::new()).await.is_ok());

    let kept = old_snapshot(root, "kept");
    let dropped = old_snapshot(root, "dropped");
    let young = w.snapshot(root.clone()).await;
    let young = young.unwrap_or_else(|e| panic!("{e:?}"));
    let log = |root: &Path| {
        git(
            root,
            &["log", "--walk-reflogs", "--format=%H", "refs/ovra/snapshot"],
        )
    };
    assert_eq!(log(root), format!("{young}\n{dropped}\n{kept}"));

    let pruned = w.prune_snapshots(root.clone(), vec![kept.clone()]).await;
    assert!(pruned.is_ok(), "{pruned:?}");
    assert_eq!(log(root), format!("{young}\n{kept}"));
    assert_eq!(git(root, &["rev-parse", "refs/ovra/snapshot"]), young);

    assert!(w.prune_snapshots(root.clone(), Vec::new()).await.is_ok());
    assert_eq!(log(root), young);
}

/// A symlink in the main checkout is a conflict, never written through to
/// whatever it points to.
#[cfg(unix)]
#[tokio::test]
async fn apply_leaves_a_symlink_in_the_checkout_to_the_person() {
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let root = &repo.root;
    let outside = repo.tmp.path().join("outside.txt");
    fs::write(&outside, "outside\n").unwrap_or_else(|e| panic!("{e}"));
    std::os::unix::fs::symlink(&outside, root.join("link.txt")).unwrap_or_else(|e| panic!("{e}"));
    git(root, &["add", "link.txt"]);
    git(root, &["commit", "--quiet", "-m", "link"]);
    let w = GitWorktrees::new();
    let (base, worker) = repo.worker(&w).await;
    fs::remove_file(worker.join("link.txt")).unwrap_or_else(|e| panic!("{e}"));
    put(&worker, "link.txt", "agent\n");
    let tree = w.look(worker).await.unwrap_or_else(|e| panic!("{e:?}"));

    let conflicts = w
        .apply(tree, base, root.clone(), rels(&["link.txt"]))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(conflicts, ["link.txt"]);
    assert_eq!(
        read(repo.tmp.path(), "outside.txt").as_deref(),
        Some("outside\n")
    );
    let meta = fs::symlink_metadata(root.join("link.txt")).unwrap_or_else(|e| panic!("{e}"));
    assert!(meta.is_symlink());
}

/// A worker's symlink lands in the checkout only when it stays inside.
#[cfg(unix)]
#[tokio::test]
async fn apply_recreates_only_symlinks_that_stay_inside() {
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let root = &repo.root;
    let w = GitWorktrees::new();
    let (base, worker) = repo.worker(&w).await;
    fs::create_dir_all(worker.join("d")).unwrap_or_else(|e| panic!("{e}"));
    let link = |target: &str, at: &str| {
        std::os::unix::fs::symlink(target, worker.join(at)).unwrap_or_else(|e| panic!("{e}"));
    };
    link("../a.txt", "d/up");
    link("/etc/passwd", "abs");
    link("../../x", "climb");
    link(".", "s");
    link("s/../secret", "bypass");

    let tree = w.look(worker).await.unwrap_or_else(|e| panic!("{e:?}"));
    let paths = rels(&["d/up", "abs", "climb", "s", "bypass"]);
    let conflicts = w.apply(tree, base, root.clone(), paths).await;
    let conflicts = conflicts.unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(conflicts, ["abs", "bypass", "climb"]);
    let target = |at: &str| fs::read_link(root.join(at)).ok();
    assert_eq!(target("d/up"), Some(PathBuf::from("../a.txt")));
    assert_eq!(target("s"), Some(PathBuf::from(".")));
    assert!(!root.join("abs").exists() && target("bypass").is_none());
}

#[tokio::test]
async fn taking_the_workers_version_can_be_undone() {
    let repo = Repo::new(&[("a.txt", "a\n"), ("gone.txt", "gone\n")]);
    let root = &repo.root;
    let w = GitWorktrees::new();
    let (_, wt) = repo.worker(&w).await;
    put(&wt, "a.txt", "agent\n");
    fs::remove_file(wt.join("gone.txt")).unwrap_or_else(|e| panic!("{e}"));
    put(root, "a.txt", "mine\n");
    let approved = w
        .snapshot(wt.clone())
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    // Written after the approve: not what is taken.
    put(&wt, "a.txt", "newer\n");

    let paths = rels(&["a.txt", "gone.txt"]);
    let taken = w.take(approved, root.clone(), paths.clone()).await;
    let taken = taken.unwrap_or_else(|e| panic!("{e:?}"));
    assert!(taken.skipped.is_empty(), "{:?}", taken.skipped);
    // `after` holds the worker's version: nothing changed since the take.
    let since = w.changes(root.clone(), taken.after.clone()).await;
    assert_eq!(since.map(|c| c.len()).ok(), Some(0));
    assert_eq!(read(root, "a.txt").as_deref(), Some("agent\n"));
    assert_eq!(read(root, "gone.txt"), None);

    assert!(w.restore(root.clone(), taken.undo, paths).await.is_ok());
    assert_eq!(read(root, "a.txt").as_deref(), Some("mine\n"));
    assert_eq!(read(root, "gone.txt").as_deref(), Some("gone\n"));
}

#[tokio::test]
async fn ended_workers_lose_nothing_when_cleaned_up() {
    let repo = Repo::new(&[("a.txt", "a\n"), (".gitignore", "*.env\ntarget/\n")]);
    let root = &repo.root;
    let w = GitWorktrees::new();
    let (base, wt) = repo.worker(&w).await;
    put(&wt, "target/debug/out.bin", "build output\n");
    put(&wt, "secrets.env", "TOKEN=1\n");

    let refused = w.remove(root.clone(), wt.clone()).await;
    let err = refused.err();
    assert_eq!(err.as_ref().map(|e| e.code), Some(ErrorCode::Conflict));
    assert!(format!("{err:?}").contains("secrets.env"), "{err:?}");
    assert!(wt.join("secrets.env").exists());
    fs::remove_file(wt.join("secrets.env")).unwrap_or_else(|e| panic!("{e}"));
    let removed = w.remove(root.clone(), wt.clone()).await;
    assert!(removed.is_ok(), "{removed:?}");

    // A branch with only its base goes; one the worker committed on stays.
    let other = "ovra/t1/committed";
    git(root, &["branch", other, &base]);
    git(root, &["commit", "--allow-empty", "--quiet", "-m", "x"]);
    git(
        root,
        &["update-ref", &format!("refs/heads/{other}"), "HEAD"],
    );
    git(root, &["reset", "--quiet", "--hard", "HEAD~1"]);
    let kept = w
        .delete_branch(root.clone(), other.into(), base.clone())
        .await;
    assert_eq!(kept.err().map(|e| e.code), Some(ErrorCode::Conflict));
    assert!(
        w.delete_branch(root.clone(), BRANCH.into(), base.clone())
            .await
            .is_ok()
    );
    assert!(git_fails(
        root,
        &["rev-parse", "--verify", "--quiet", BRANCH]
    ));
    // Already gone is fine.
    assert!(
        w.delete_branch(root.clone(), BRANCH.into(), base)
            .await
            .is_ok()
    );
}

/// Only the reviewed files are written, the way the person's checkout
/// writes files: attributes elsewhere in the worker's tree do not apply.
#[tokio::test]
async fn apply_writes_files_with_the_checkouts_rules_only() {
    let repo = Repo::new(&[("a.txt", "a\n"), ("run.sh", "echo\n")]);
    let root = &repo.root;
    let w = GitWorktrees::new();
    let (base, wt) = repo.worker(&w).await;
    put(&wt, ".gitattributes", "*.txt eol=crlf\n");
    put(&wt, "a.txt", "one\ntwo\n");
    put(&wt, "run.sh", "echo hi\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let exec = fs::Permissions::from_mode(0o755);
        fs::set_permissions(wt.join("run.sh"), exec).unwrap_or_else(|e| panic!("{e}"));
    }
    let tree = w.look(wt).await.unwrap_or_else(|e| panic!("{e:?}"));

    let paths = rels(&["a.txt", "run.sh"]);
    let conflicts = w.apply(tree, base, root.clone(), paths).await;
    assert_eq!(conflicts.ok(), Some(Vec::new()));
    let bytes = fs::read(root.join("a.txt")).unwrap_or_default();
    assert_eq!(
        bytes, b"one\ntwo\n",
        "no CRLF from the unreviewed attributes"
    );
    assert!(!root.join(".gitattributes").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(root.join("run.sh")).map(|m| m.permissions().mode());
        assert_eq!(mode.ok().map(|m| m & 0o111), Some(0o111));
    }
}

/// A submodule in the worker's tree is not a file to copy; a checkout below
/// its repository's top is refused rather than written in the wrong place.
#[tokio::test]
async fn apply_refuses_submodules_and_checkouts_below_the_top() {
    let repo = Repo::new(&[("a.txt", "a\n"), ("p/b.txt", "b\n")]);
    let root = &repo.root;
    let w = GitWorktrees::new();
    let base = git(root, &["rev-parse", "HEAD"]);
    let listing = format!("160000 commit {base}\tsub\n");
    let tree = Command::new(crate::git::program())
        .args(["mktree"])
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(listing.as_bytes())?;
            }
            child.wait_with_output()
        })
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_else(|e| panic!("{e}"));

    let conflicts = w
        .apply(tree.clone(), base.clone(), root.clone(), rels(&["sub"]))
        .await;
    assert_eq!(conflicts.ok(), Some(vec!["sub".to_owned()]));
    assert!(!root.join("sub").exists());

    let below = w.apply(tree, base, root.join("p"), rels(&["b.txt"])).await;
    assert_eq!(below.err().map(|e| e.code), Some(ErrorCode::Conflict));
    assert_eq!(read(root, "p/b.txt").as_deref(), Some("b\n"));
}

/// A review's fingerprint rests on `Delta.version`: a change of mode alone,
/// or a symlink pointing elsewhere, must change it.
#[cfg(unix)]
#[tokio::test]
async fn a_version_changes_with_the_mode_and_a_links_target() {
    use std::os::unix::fs::PermissionsExt;
    let repo = Repo::new(&[("run.sh", "echo\n"), ("x", "x\n"), ("y", "y\n")]);
    let root = &repo.root;
    std::os::unix::fs::symlink("x", root.join("link")).unwrap_or_else(|e| panic!("{e}"));
    let w = GitWorktrees::new();
    let versions = |deltas: Vec<ovra_core::services::Delta>| {
        let map: std::collections::BTreeMap<String, String> =
            deltas.into_iter().map(|d| (d.path, d.version)).collect();
        map
    };
    let before = w.changes(root.clone(), "HEAD".into()).await;
    let before = versions(before.unwrap_or_else(|e| panic!("{e:?}")));

    fs::set_permissions(root.join("run.sh"), fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| panic!("{e}"));
    fs::remove_file(root.join("link")).unwrap_or_else(|e| panic!("{e}"));
    std::os::unix::fs::symlink("y", root.join("link")).unwrap_or_else(|e| panic!("{e}"));
    let after = w.changes(root.clone(), "HEAD".into()).await;
    let after = versions(after.unwrap_or_else(|e| panic!("{e:?}")));

    assert!(!before.contains_key("run.sh"), "{before:?}");
    assert!(after.contains_key("run.sh"), "mode only: {after:?}");
    assert_ne!(before.get("link"), after.get("link"));
    assert!(after.get("link").is_some_and(|v| v.starts_with("120000 ")));
}

/// A symlink blob with a NUL: git would write only `..`. Real links cannot
/// hold one, but a worker can forge its tree's blobs.
#[cfg(unix)]
#[tokio::test]
async fn apply_refuses_a_link_target_git_would_cut_short() {
    use std::io::Write;
    let repo = Repo::new(&[("a.txt", "a\n")]);
    let root = &repo.root;
    let w = GitWorktrees::new();
    let base = git(root, &["rev-parse", "HEAD"]);
    let mut hash = Command::new(crate::git::program())
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("{e}"));
    if let Some(mut stdin) = hash.stdin.take() {
        stdin.write_all(b"..\0x").unwrap_or_else(|e| panic!("{e}"));
    }
    let out = hash.wait_with_output().unwrap_or_else(|e| panic!("{e}"));
    let blob = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    let index = repo.tmp.path().join("forged-index");
    let forge = |args: &[&str]| {
        let out = Command::new(crate::git::program())
            .args(args)
            .current_dir(root)
            .env("GIT_INDEX_FILE", &index)
            .output()
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(out.status.success(), "{args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    forge(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("120000,{blob},l"),
    ]);
    let tree = forge(&["write-tree"]);

    let conflicts = w.apply(tree, base, root.clone(), rels(&["l"])).await;
    assert_eq!(conflicts.unwrap_or_else(|e| panic!("{e:?}")), ["l"]);
    assert!(fs::symlink_metadata(root.join("l")).is_err());
}

/// Paths git or the hash check would misread are left to the person, and
/// a deletion never reaches into `.git`.
#[tokio::test]
async fn apply_leaves_git_folders_and_odd_names_alone() {
    let repo = Repo::new(&[("readme.md", "old\n")]);
    let root = &repo.root;
    let w = GitWorktrees::new();
    let (base, worker) = repo.worker(&w).await;
    // Renamed in case only: on a case-insensitive disk the deletion of the
    // old name would remove the file just written.
    fs::remove_file(worker.join("readme.md")).unwrap_or_else(|e| panic!("{e}"));
    put(&worker, "README.md", "new\n");
    let tree = w.look(worker).await.unwrap_or_else(|e| panic!("{e:?}"));
    let mut paths = vec![".git/config", "README.md", "readme.md"];
    // Names `RelPath` already refuses on Windows.
    if cfg!(unix) {
        paths.extend(["\"quoted", "sub/Git~1/x"]);
    }
    let conflicts = w.apply(tree, base, root.clone(), rels(&paths)).await;
    let conflicts = conflicts.unwrap_or_else(|e| panic!("{e:?}"));
    let mut want = vec![".git/config", "readme.md"];
    if cfg!(unix) {
        want.insert(0, "\"quoted");
        want.push("sub/Git~1/x");
    }
    // On a case-insensitive disk README.md is the person's readme.md, so it
    // conflicts too; elsewhere it is written.
    let written = !conflicts.iter().any(|c| c == "README.md");
    let rest: Vec<&str> = conflicts
        .iter()
        .map(String::as_str)
        .filter(|c| *c != "README.md")
        .collect();
    assert_eq!(rest, want);
    assert!(root.join(".git").join("config").is_file());
    if written {
        assert_eq!(read(root, "README.md").as_deref(), Some("new\n"));
    }
}
