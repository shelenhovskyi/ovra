use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use ovra_core::Core;
use ovra_core::protocol::Event;
use ovra_core::protocol::fs::{FileBody, FsEvent};
use tempfile::TempDir;
use tokio::sync::broadcast;

use super::*;

fn rel(path: &str) -> RelPath {
    RelPath::parse(path).expect("valid path")
}

fn put(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("has parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

/// Runs git with a config that does not depend on the machine.
fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?}");
}

fn dir() -> TempDir {
    tempfile::tempdir().expect("temp dir")
}

async fn list(fs: &FsIndex, root: &Path, path: &str) -> Result<DirListing> {
    fs.list_dir(root.to_path_buf(), rel(path)).await
}

fn code<T: std::fmt::Debug>(result: Result<T>) -> ErrorCode {
    result.expect_err("should fail").code
}

#[tokio::test]
async fn lists_with_gitignore_and_git_status() {
    let tmp = dir();
    let root = tmp.path();
    git(root, &["init", "-q"]);
    put(root, ".gitignore", "target/\n*.log\n");
    put(root, "src/main.rs", "fn main() {}\n");
    put(root, "src/lib.rs", "\n");
    put(root, "src/util.rs", "\n");
    put(root, "src/module-10.rs", "\n");
    put(root, "src/module-2.rs", "\n");
    put(root, "old/gone.txt", "bye\n");
    put(root, "README.md", "# hi\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "init"]);
    put(root, "src/main.rs", "fn main() { changed() }\n");
    put(root, "src/staged.rs", "\n");
    git(root, &["add", "src/staged.rs"]);
    git(root, &["mv", "src/util.rs", "src/helpers.rs"]);
    fs::remove_file(root.join("src/lib.rs")).expect("rm");
    fs::remove_file(root.join("old/gone.txt")).expect("rm");
    fs::remove_dir(root.join("old")).expect("rmdir");
    put(root, "target/debug/app", "bin");
    put(root, "debug.log", "x");
    put(root, "notes/todo.md", "- a\n");
    #[cfg(unix)]
    std::os::unix::fs::symlink("README.md", root.join("link")).expect("symlink");

    let fs = FsIndex::new();
    insta::assert_json_snapshot!("root", list(&fs, root, "").await.expect("root"));
    insta::assert_json_snapshot!("src", list(&fs, root, "src").await.expect("src"));
    // Everything inside an untracked directory is untracked.
    insta::assert_json_snapshot!("notes", list(&fs, root, "notes").await.expect("notes"));
    // A directory that only holds deleted files still lists them.
    insta::assert_json_snapshot!("old", list(&fs, root, "old").await.expect("old"));
}

#[tokio::test]
async fn gitignore_applies_outside_git_and_project_may_be_a_repo_subfolder() {
    let tmp = dir();
    put(tmp.path(), "plain/.gitignore", "*.tmp\n");
    put(tmp.path(), "plain/a.tmp", "");
    put(tmp.path(), "plain/b.txt", "");
    let fs = FsIndex::new();
    let plain = list(&fs, &tmp.path().join("plain"), "")
        .await
        .expect("list");
    let flags: Vec<_> = plain
        .entries
        .iter()
        .map(|e| (e.name.as_str(), e.ignored, e.git))
        .collect();
    assert_eq!(
        flags,
        [
            (".gitignore", false, None),
            ("a.tmp", true, None),
            ("b.txt", false, None)
        ]
    );

    git(tmp.path(), &["init", "-q"]);
    put(tmp.path(), "repo/app/new.txt", "");
    let sub = list(&fs, &tmp.path().join("repo"), "").await.expect("list");
    assert_eq!(sub.entries[0].name, "app");
    assert_eq!(
        sub.entries[0].git,
        Some(ovra_core::protocol::fs::GitStatus::Untracked)
    );
}

#[tokio::test]
async fn missing_and_wrong_kind() {
    let tmp = dir();
    put(tmp.path(), "file.txt", "x");
    let fs = FsIndex::new();
    assert_eq!(
        code(list(&fs, tmp.path(), "nope").await),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(list(&fs, tmp.path(), "file.txt").await),
        ErrorCode::BadRequest
    );
    let read = fs.read_file(tmp.path().to_path_buf(), rel("nope")).await;
    assert_eq!(code(read), ErrorCode::NotFound);
    let write = fs.write_file(
        tmp.path().to_path_buf(),
        rel("file.txt/x"),
        "y".into(),
        None,
    );
    assert_eq!(code(write.await), ErrorCode::NotFound);
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_do_not_leave_the_root() {
    use std::os::unix::fs::symlink;
    let outside = dir();
    put(outside.path(), "secret.txt", "s");
    let tmp = dir();
    let root = tmp.path();
    put(root, "inside.txt", "i");
    symlink(outside.path(), root.join("out")).expect("symlink");
    symlink(outside.path().join("secret.txt"), root.join("secret")).expect("symlink");
    symlink(outside.path().join("new.txt"), root.join("dangling")).expect("symlink");
    symlink("inside.txt", root.join("alias")).expect("symlink");

    let fs = FsIndex::new();
    let r = |p: &str| (root.to_path_buf(), rel(p));
    assert_eq!(code(list(&fs, root, "out").await), ErrorCode::Forbidden);
    let (a, b) = r("secret");
    assert_eq!(code(fs.read_file(a, b).await), ErrorCode::Forbidden);
    for path in ["out/new.txt", "out/deep/new.txt", "secret", "dangling"] {
        let (a, b) = r(path);
        let written = fs.write_file(a, b, "x".into(), None).await;
        assert_eq!(code(written), ErrorCode::Forbidden, "{path}");
    }
    assert!(!outside.path().join("new.txt").exists());
    assert!(!outside.path().join("deep").exists());

    let (a, b) = r("alias");
    let alias = fs.read_file(a, b).await.expect("inside link");
    assert_eq!(alias.body, FileBody::Text { text: "i".into() });
}

#[tokio::test]
async fn reads_text_binary_and_large_files() {
    let tmp = dir();
    let root = tmp.path();
    put(root, "a.txt", "hello");
    fs::write(root.join("dot.png"), [0x89, b'P', b'N', b'G', 0, 1]).expect("write");
    fs::File::create(root.join("big.bin"))
        .and_then(|f| f.set_len(files::MAX_FILE_BYTES + 1))
        .expect("big file");
    let fs = FsIndex::new();
    let read = |p: &str| fs.read_file(root.to_path_buf(), rel(p));

    let text = read("a.txt").await.expect("text");
    assert_eq!(
        (text.size, text.body),
        (
            5,
            FileBody::Text {
                text: "hello".into()
            }
        )
    );
    let png = read("dot.png").await.expect("png").body;
    let expected = FileBody::Binary {
        mime: "image/png".into(),
        base64: "iVBORwAB".into(),
    };
    assert_eq!(png, expected);
    assert_eq!(read("big.bin").await.expect("big").body, FileBody::TooLarge);
    assert_eq!(code(read("").await), ErrorCode::BadRequest);
}

#[tokio::test]
async fn writes_with_mtime_conflict_check() {
    let tmp = dir();
    let root = tmp.path().to_path_buf();
    let fs = FsIndex::new();
    let write =
        |p: &str, text: &str, mtime| fs.write_file(root.clone(), rel(p), text.into(), mtime);

    let created = write("new/deep/file.txt", "one", None)
        .await
        .expect("create");
    let read = fs
        .read_file(root.clone(), rel("new/deep/file.txt"))
        .await
        .expect("read");
    assert_eq!(read.mtime_ms, created);
    assert_eq!(read.body, FileBody::Text { text: "one".into() });

    let stale = created.saturating_sub(10_000);
    assert_eq!(
        code(write("new/deep/file.txt", "two", Some(stale)).await),
        ErrorCode::Conflict
    );
    write("new/deep/file.txt", "two", Some(created))
        .await
        .expect("matching mtime");
    assert_eq!(
        fs::read_to_string(root.join("new/deep/file.txt")).expect("read"),
        "two"
    );
    assert_eq!(code(write("new", "x", None).await), ErrorCode::BadRequest);

    let names: Vec<_> = fs::read_dir(root.join("new/deep"))
        .expect("ls")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(names, ["file.txt"], "no temp files left behind");
}

#[cfg(unix)]
#[tokio::test]
async fn write_keeps_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = dir();
    put(tmp.path(), "run.sh", "#!/bin/sh\n");
    let script = tmp.path().join("run.sh");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");
    FsIndex::new()
        .write_file(
            tmp.path().to_path_buf(),
            rel("run.sh"),
            "#!/bin/sh\necho\n".into(),
            None,
        )
        .await
        .expect("write");
    let mode = fs::metadata(&script).expect("meta").permissions().mode();
    assert_eq!(mode & 0o777, 0o755);
}

/// Waits for the next `fs.*` event, failing after a generous timeout.
async fn next_fs(rx: &mut broadcast::Receiver<Event>) -> FsEvent {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("an event in time")
            .expect("channel open");
        if let Event::Fs(e) = event {
            return e;
        }
    }
}

/// Collects changed paths until `want` was seen.
async fn changed_until(rx: &mut broadcast::Receiver<Event>, want: &str) -> Vec<String> {
    let mut seen = Vec::new();
    while !seen.iter().any(|p| p == want) {
        if let FsEvent::Changed { paths, .. } = next_fs(rx).await {
            seen.extend(paths);
        }
    }
    seen
}

/// Waits until the watcher of `project` finished its initial walk.
async fn ready(rx: &mut broadcast::Receiver<Event>, project: &str) {
    loop {
        if let FsEvent::FilesChanged {
            project: p,
            version: 1,
            ..
        } = next_fs(rx).await
            && p.0 == project
        {
            return;
        }
    }
}

#[tokio::test]
async fn watcher_reports_changes_in_new_dirs_and_skips_ignored() {
    let tmp = dir();
    let root = tmp.path();
    put(root, ".gitignore", "build/\n");
    put(root, "build/out.o", "");
    put(root, "src/a.rs", "");
    let core = Core::new("test");
    let mut rx = core.subscribe();
    let fs = FsIndex::new();
    fs.watch(ProjectId("p".into()), root.to_path_buf(), core.sink())
        .expect("watch");
    ready(&mut rx, "p").await;

    put(root, "build/later.o", "");
    put(root, "src/a.rs", "changed");
    let seen = changed_until(&mut rx, "src/a.rs").await;
    assert!(!seen.iter().any(|p| p.starts_with("build/")), "{seen:?}");

    fs::create_dir(root.join("fresh")).expect("mkdir");
    let seen = changed_until(&mut rx, "fresh").await;
    assert!(seen.contains(&"fresh".to_owned()));
    put(root, "fresh/inner.txt", "x");
    changed_until(&mut rx, "fresh/inner.txt").await;

    // Writes through the service are atomic and report only the real path.
    fs.write_file(root.to_path_buf(), rel("src/b.rs"), "b".into(), None)
        .await
        .expect("write");
    let seen = changed_until(&mut rx, "src/b.rs").await;
    assert!(
        !seen.iter().any(|p| p.ends_with(files::TMP_SUFFIX)),
        "{seen:?}"
    );
}

#[tokio::test]
async fn watcher_reports_git_changes_and_stops_on_unwatch() {
    let tmp = dir();
    let root = tmp.path();
    git(root, &["init", "-q"]);
    put(root, "a.txt", "one");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "init"]);
    let core = Core::new("test");
    let mut rx = core.subscribe();
    let fs = FsIndex::fast();
    let p1 = ProjectId("p1".into());
    fs.watch(p1.clone(), root.to_path_buf(), core.sink())
        .expect("watch");
    ready(&mut rx, "p1").await;
    // Prime the watcher's cache so the change below is a change.
    list(&fs, root, "").await.expect("list");

    put(root, "a.txt", "two");
    while next_fs(&mut rx).await
        != (FsEvent::GitChanged {
            project: p1.clone(),
        })
    {}
    let listed = list(&fs, root, "").await.expect("list");
    let a = listed
        .entries
        .iter()
        .find(|e| e.name == "a.txt")
        .expect("a.txt");
    assert_eq!(a.git, Some(ovra_core::protocol::fs::GitStatus::Modified));

    // After unwatch, only the other project's change arrives.
    fs.unwatch(&p1);
    let other = dir();
    fs.watch(
        ProjectId("p2".into()),
        other.path().to_path_buf(),
        core.sink(),
    )
    .expect("watch");
    ready(&mut rx, "p2").await;
    put(root, "a.txt", "three");
    put(other.path(), "b.txt", "b");
    loop {
        match next_fs(&mut rx).await {
            FsEvent::Changed { project, paths } if project.0 == "p2" => {
                assert_eq!(paths, ["b.txt"]);
                break;
            }
            FsEvent::Changed { project, .. }
            | FsEvent::GitChanged { project }
            | FsEvent::FilesChanged { project, .. } => {
                assert_ne!(project, p1, "event after unwatch");
            }
        }
    }
}

#[tokio::test]
async fn git_refresher_folds_a_burst_of_changes_into_one_status_run() {
    let tmp = dir();
    let root = tmp.path();
    git(root, &["init", "-q"]);
    put(root, "a.txt", "one");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "init"]);
    let cache = Arc::new(watch::GitCache::new(git::Repo::discover(
        &resolve::root(root).expect("root"),
    )));
    cache.current();
    let core = Core::new("test");
    let mut rx = core.subscribe();
    let project = ProjectId("p".into());
    let timing = watch::Timing {
        git_quiet: Duration::from_millis(50),
        ..Default::default()
    };
    let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let refresh = watch::spawn_git_refresher(
        project.clone(),
        core.sink(),
        Arc::clone(&cache),
        stopped,
        timing,
    )
    .expect("spawn");

    // An agent writing many files: every batch of file events asks for a
    // refresh, faster than the quiet period.
    put(root, "a.txt", "two");
    for _ in 0..50 {
        refresh.send(()).expect("send");
    }
    while next_fs(&mut rx).await
        != (FsEvent::GitChanged {
            project: project.clone(),
        })
    {}
    // One run to prime the cache, one for the whole burst.
    assert_eq!(cache.runs.load(std::sync::atomic::Ordering::Relaxed), 2);
}

#[tokio::test]
async fn lists_all_files_that_are_not_ignored() {
    let tmp = dir();
    let root = tmp.path();
    git(root, &["init", "-q"]);
    put(root, ".gitignore", "target/\n");
    put(root, "target/out", "");
    put(root, "src/b.rs", "");
    put(root, "src/a.rs", "");
    put(root, "README.md", "");
    let fs = FsIndex::new();
    let page = |offset, limit| fs.list_files(root.to_path_buf(), offset, limit);
    let all = page(0, 100).await.expect("list");
    assert_eq!(
        all.paths,
        [".gitignore", "README.md", "src/a.rs", "src/b.rs"]
    );
    let second = page(1, 2).await.expect("page");
    assert_eq!(
        (second.paths, second.total),
        (vec!["README.md".to_owned(), "src/a.rs".to_owned()], 4)
    );
}

#[tokio::test]
async fn watcher_keeps_the_file_list_fresh() {
    let tmp = dir();
    let root = tmp.path();
    put(root, ".gitignore", "*.log\n");
    put(root, "src/a.rs", "");
    put(root, "src/deep/b.rs", "");
    put(root, "src-old.rs", "");
    let core = Core::new("test");
    let mut rx = core.subscribe();
    let fs = FsIndex::fast();
    let project = ProjectId("p".into());
    fs.watch(project.clone(), root.to_path_buf(), core.sink())
        .expect("watch");
    ready(&mut rx, "p").await;
    let listed = || fs.list_files(root.to_path_buf(), 0, 100);
    let before = listed().await.expect("list");
    assert_eq!(
        before.paths,
        [".gitignore", "src-old.rs", "src/a.rs", "src/deep/b.rs"]
    );

    let mut next_change = async || loop {
        if let FsEvent::FilesChanged {
            added,
            removed,
            version,
            ..
        } = next_fs(&mut rx).await
        {
            return (added, removed, version);
        }
    };
    put(root, "new/c.rs", "");
    put(root, "debug.log", "");
    let (added, removed, version) = next_change().await;
    assert_eq!((added, removed), (vec!["new/c.rs".to_owned()], vec![]));
    assert!(version > before.version);

    fs::remove_dir_all(root.join("src")).expect("rm");
    let (added, removed, _) = next_change().await;
    assert_eq!(
        (added, removed),
        (
            vec![],
            vec!["src/a.rs".to_owned(), "src/deep/b.rs".to_owned()]
        )
    );
    let after = listed().await.expect("list");
    assert_eq!(after.paths, [".gitignore", "new/c.rs", "src-old.rs"]);
}

#[tokio::test]
async fn file_list_pages_through_a_watched_project() {
    let tmp = dir();
    let root = tmp.path();
    for d in 0..10 {
        fs::create_dir(root.join(format!("d{d}"))).expect("mkdir");
        for f in 0..50 {
            fs::write(root.join(format!("d{d}/f{f}")), "").expect("write");
        }
    }
    let fs = FsIndex::new();
    let core = Core::new("t");
    let mut rx = core.subscribe();
    fs.watch(ProjectId("p".into()), root.to_path_buf(), core.sink())
        .expect("watch");
    ready(&mut rx, "p").await;
    let mut all = Vec::new();
    let mut version = None;
    while all.len() < 500 {
        let offset = u32::try_from(all.len()).expect("fits");
        let page = fs.list_files(root.to_path_buf(), offset, 200);
        let page = page.await.expect("page");
        assert_eq!(page.total, 500);
        assert_eq!(*version.get_or_insert(page.version), page.version);
        all.extend(page.paths);
    }
    assert!(all.is_sorted());
    assert_eq!(all.len(), 500);
}

#[tokio::test]
async fn saving_a_file_deleted_on_disk_is_a_conflict() {
    let tmp = dir();
    let root = tmp.path().to_path_buf();
    let fs = FsIndex::new();
    let mtime = fs.write_file(root.clone(), rel("a.txt"), "one".into(), None);
    let mtime = mtime.await.expect("create");
    fs::rename(root.join("a.txt"), root.join("b.txt")).expect("rename");

    let saved = fs.write_file(root.clone(), rel("a.txt"), "two".into(), Some(mtime));
    let err = saved.await.expect_err("conflict");
    assert_eq!(
        (err.code, err.message.as_str()),
        (ErrorCode::Conflict, "a.txt was deleted on disk")
    );
    assert!(!root.join("a.txt").exists(), "not recreated");
}

#[tokio::test]
async fn a_listed_directory_reports_changes_before_indexing_ends() {
    let tmp = dir();
    let root = tmp.path();
    for d in 0..20 {
        put(root, &format!("bulk{d}/f.txt"), "");
    }
    put(root, "src/a.rs", "");
    let core = Core::new("test");
    let mut rx = core.subscribe();
    let fs = FsIndex::new();
    fs.watch(ProjectId("p".into()), root.to_path_buf(), core.sink())
        .expect("watch");
    // No waiting for the walk: listing `src` watches it right away.
    list(&fs, root, "src").await.expect("list");
    put(root, "src/a.rs", "changed");
    changed_until(&mut rx, "src/a.rs").await;
}
