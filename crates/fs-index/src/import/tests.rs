use std::sync::{Arc, Mutex};

use tempfile::TempDir;

use super::*;

fn rel(path: &str) -> RelPath {
    RelPath::parse(path).expect("valid path")
}

/// A project `p` with `src/a.png`, and a folder `out` outside it with
/// `a.png` and `d/x.txt`.
fn setup() -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (root, out) = (tmp.path().join("p"), tmp.path().join("out"));
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::create_dir_all(out.join("d")).expect("mkdir");
    fs::write(root.join("src/a.png"), "old").expect("write");
    fs::write(out.join("a.png"), "new").expect("write");
    fs::write(out.join("d/x.txt"), "x").expect("write");
    (tmp, root, out)
}

/// A trash that removes what it gets and records it.
fn trash() -> (Trash, Arc<Mutex<Vec<PathBuf>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let trash = Trash::fake(move |path| {
        log.lock().expect("lock").push(path.to_path_buf());
        fs::remove_file(path).map_err(|e| e.to_string())
    });
    (trash, seen)
}

fn run(root: &Path, sources: &[PathBuf], into: &str, clash: Clash) -> Result<Imported> {
    import(root, sources, &rel(into), clash, &trash().0, LIMITS)
}

fn copied(paths: &[&str], links: &[&str]) -> Imported {
    Imported::Copied {
        paths: paths.iter().map(|p| rel(p)).collect(),
        skipped_links: links.iter().map(|l| (*l).to_owned()).collect(),
    }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("read")
}

#[test]
fn copies_files_and_folders_into_the_target() {
    let (_tmp, root, out) = setup();
    let done = run(&root, &[out.join("a.png"), out.join("d")], "", Clash::Ask);
    assert_eq!(done.expect("import"), copied(&["a.png", "d"], &[]));
    assert_eq!(read(&root.join("a.png")), "new");
    assert_eq!(read(&root.join("d/x.txt")), "x");
    assert_eq!(read(&out.join("d/x.txt")), "x", "the source stays");
}

#[test]
fn a_clash_copies_nothing_without_an_answer() {
    let (_tmp, root, out) = setup();
    let sources = [out.join("d"), out.join("a.png")];
    let done = run(&root, &sources, "src", Clash::Ask).expect("import");
    assert_eq!(done, Imported::Clashes(vec!["a.png".into()]));
    assert!(!root.join("src/d").exists());
    assert_eq!(read(&root.join("src/a.png")), "old");
}

#[test]
fn keep_both_copies_under_a_free_name() {
    let (_tmp, root, out) = setup();
    fs::write(root.join("src/a copy.png"), "older").expect("write");
    fs::create_dir(root.join("src/d")).expect("mkdir");
    let sources = [out.join("a.png"), out.join("d")];
    let done = run(&root, &sources, "src", Clash::KeepBoth).expect("import");
    assert_eq!(done, copied(&["src/a copy 2.png", "src/d copy"], &[]));
    assert_eq!(read(&root.join("src/a.png")), "old");
    assert_eq!(read(&root.join("src/a copy 2.png")), "new");
    assert_eq!(read(&root.join("src/d copy/x.txt")), "x");
}

#[test]
fn replace_moves_the_old_entry_to_the_trash_first() {
    let (_tmp, root, out) = setup();
    let (bin, seen) = trash();
    let done = import(
        &root,
        &[out.join("a.png")],
        &rel("src"),
        Clash::Replace,
        &bin,
        LIMITS,
    );
    assert_eq!(done.expect("import"), copied(&["src/a.png"], &[]));
    assert_eq!(read(&root.join("src/a.png")), "new");
    let trashed = seen.lock().expect("lock").clone();
    assert_eq!(trashed.len(), 1);
    assert!(trashed[0].ends_with("src/a.png"));

    // Not with itself: a file dropped into its own folder.
    let own = root.join("src/a.png");
    let done = run(&root, &[own], "src", Clash::Replace);
    assert_eq!(done.expect_err("refused").code, ErrorCode::BadRequest);
}

#[cfg(unix)]
#[test]
fn links_are_skipped_and_named() {
    let (_tmp, root, out) = setup();
    std::os::unix::fs::symlink("/etc/passwd", out.join("d/passwd")).expect("link");
    std::os::unix::fs::symlink(out.join("d"), out.join("link")).expect("link");
    let done = run(&root, &[out.join("d"), out.join("link")], "", Clash::Ask);
    assert_eq!(done.expect("import"), copied(&["d"], &["d/passwd", "link"]));
    assert!(fs::symlink_metadata(root.join("d/passwd")).is_err());
    assert!(fs::symlink_metadata(root.join("link")).is_err());
}

#[cfg(unix)]
#[test]
fn a_socket_refuses_the_whole_drop() {
    let (_tmp, root, out) = setup();
    let _socket = std::os::unix::net::UnixListener::bind(out.join("d/s")).expect("bind");
    let done = run(&root, &[out.join("a.png"), out.join("d")], "", Clash::Ask);
    assert_eq!(done.expect_err("refused").code, ErrorCode::BadRequest);
    assert!(!root.join("a.png").exists(), "nothing was copied");
}

#[test]
fn more_than_the_limits_is_refused_before_copying() {
    let (_tmp, root, out) = setup();
    let sources = [out.join("a.png"), out.join("d")];
    for limits in [
        Limits {
            entries: 2,
            bytes: 100,
        },
        Limits {
            entries: 100,
            bytes: 3,
        },
    ] {
        let done = import(&root, &sources, &rel(""), Clash::Ask, &trash().0, limits);
        let err = done.expect_err("refused");
        assert_eq!(err.code, ErrorCode::BadRequest, "{err:?}");
        assert!(!root.join("a.png").exists());
    }
}

#[test]
fn refuses_targets_and_sources_it_must_not_touch() {
    let (_tmp, root, out) = setup();
    fs::create_dir(root.join(".git")).expect("mkdir");
    fs::create_dir(out.join(".git")).expect("mkdir");
    let file = [out.join("a.png")];
    let refused = |done: Result<Imported>| done.expect_err("refused").code;

    assert_eq!(
        refused(run(&root, &file, "src/a.png", Clash::Ask)),
        ErrorCode::BadRequest
    );
    assert_eq!(
        refused(run(&root, &file, "missing", Clash::Ask)),
        ErrorCode::NotFound
    );
    assert_eq!(
        refused(run(&root, &[out.join(".git")], "", Clash::Ask)),
        ErrorCode::BadRequest
    );
    assert_eq!(
        refused(run(&root, &[PathBuf::from("a.png")], "", Clash::Ask)),
        ErrorCode::BadRequest
    );
    // Two items with one name, and a folder into itself.
    let twice = [out.join("a.png"), root.join("src/a.png")];
    assert_eq!(
        refused(run(&root, &twice, "", Clash::Ask)),
        ErrorCode::BadRequest
    );
    assert_eq!(
        refused(run(&root, &[root.join("src")], "src", Clash::KeepBoth)),
        ErrorCode::BadRequest
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join(".git"), root.join("g")).expect("link");
        std::os::unix::fs::symlink(&out, root.join("o")).expect("link");
        assert_eq!(
            refused(run(&root, &file, "g", Clash::Ask)),
            ErrorCode::Forbidden
        );
        assert_eq!(
            refused(run(&root, &file, "o", Clash::Ask)),
            ErrorCode::Forbidden
        );
    }
    assert!(!out.join(".git/a.png").exists() && !root.join(".git/a.png").exists());
}
