//! Against real git in temporary repositories. Paths have spaces and
//! non-ASCII letters, and worktrees live outside the repository, as they do
//! under Ovra's data directory. Each test runs few git commands: on Windows
//! every one costs tens of milliseconds.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use ovra_core::path::RelPath;
use ovra_core::protocol::ErrorCode;
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
    let out = Command::new("git")
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
    let status = Command::new("git").args(args).current_dir(dir).output();
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
            .any(|n| n.to_string_lossy().starts_with("ovra-index")),
        "{left:?}"
    );
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
async fn a_patch_shows_one_file_and_finds_its_rename() {
    let long: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    let repo = Repo::new(&[("a.rs", "fn a() {}\nfn b() {}\n"), ("old name.txt", &long)]);
    let root = &repo.root;
    put(root, "a.rs", "fn a() {}\nfn c() {}\n");
    fs::rename(root.join("old name.txt"), root.join("new \u{e9}.txt"))
        .unwrap_or_else(|e| panic!("{e}"));
    let w = GitWorktrees::new();

    let patch = w.patch(root.clone(), "HEAD".into(), rel("a.rs")).await;
    let patch = patch.unwrap_or_else(|e| panic!("{e:?}"));
    assert!(patch.contains("-fn b() {}\n+fn c() {}\n"), "{patch}");
    assert!(!patch.contains("name.txt"), "{patch}");
    let patch = w
        .patch(root.clone(), "HEAD".into(), rel("new \u{e9}.txt"))
        .await;
    let patch = patch.unwrap_or_else(|e| panic!("{e:?}"));
    assert!(
        patch.contains("rename from old name.txt\nrename to new \u{e9}.txt\n"),
        "{patch}"
    );
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

    let paths = rels(&["a.txt", "b.txt", "sub/new.txt", "old.txt"]);
    let conflicts = w
        .apply(wt.clone(), base.clone(), repo.root.clone(), paths)
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
