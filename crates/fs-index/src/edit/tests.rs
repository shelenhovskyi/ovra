use std::sync::Mutex;

use ovra_core::services::FsService;
use tempfile::TempDir;

use super::*;
use crate::FsIndex;

fn rel(path: &str) -> RelPath {
    RelPath::parse(path).expect("valid path")
}

/// A project `p` and a sibling folder `out` with `secret.txt`.
fn project() -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (root, out) = (tmp.path().join("p"), tmp.path().join("out"));
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::create_dir(&out).expect("mkdir");
    fs::write(root.join("src/a.txt"), "a").expect("write");
    fs::write(out.join("secret.txt"), "s").expect("write");
    (tmp, root, out)
}

fn code<T: fmt::Debug>(result: Result<T>) -> ErrorCode {
    result.expect_err("should fail").code
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("read")
}

/// A fake trash that records the paths it gets.
fn recording() -> (Trash, Arc<Mutex<Vec<PathBuf>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let trash = Trash::fake(move |path| {
        log.lock().expect("lock").push(path.to_path_buf());
        Ok(())
    });
    (trash, seen)
}

#[test]
fn creates_files_and_folders_with_their_parents() {
    let (_tmp, root, _) = project();
    create(&root, &rel("new/deep/n.txt"), false).expect("file");
    assert_eq!(read(&root.join("new/deep/n.txt")), "");
    create(&root, &rel("docs/guide"), true).expect("dir");
    assert!(root.join("docs/guide").is_dir());

    assert_eq!(
        code(create(&root, &rel("src/a.txt"), false)),
        ErrorCode::Conflict
    );
    assert_eq!(
        code(create(&root, &rel("docs/guide"), true)),
        ErrorCode::Conflict
    );
    assert_eq!(read(&root.join("src/a.txt")), "a");
    assert_eq!(
        code(create(&root, &rel("src/a.txt/x"), false)),
        ErrorCode::NotFound
    );
}

#[test]
fn renames_and_moves_into_new_folders() {
    let (_tmp, root, _) = project();
    rename(&root, &rel("src/a.txt"), &rel("lib/b.txt")).expect("move");
    assert_eq!(read(&root.join("lib/b.txt")), "a");
    assert!(!root.join("src/a.txt").exists());

    fs::write(root.join("src/c.txt"), "c").expect("write");
    let taken = rename(&root, &rel("src/c.txt"), &rel("lib/b.txt"));
    assert_eq!(code(taken), ErrorCode::Conflict);
    assert_eq!(read(&root.join("lib/b.txt")), "a");
    let missing = rename(&root, &rel("src/none"), &rel("x"));
    assert_eq!(code(missing), ErrorCode::NotFound);

    rename(&root, &rel("lib"), &rel("src/lib")).expect("move folder");
    assert_eq!(read(&root.join("src/lib/b.txt")), "a");
}

#[test]
fn copies_files_and_trees() {
    let (_tmp, root, _) = project();
    fs::create_dir_all(root.join("src/sub/empty")).expect("mkdir");
    fs::write(root.join("src/sub/b.txt"), "b").expect("write");

    copy(&root, &rel("src/a.txt"), &rel("copy/a.txt")).expect("file");
    assert_eq!(read(&root.join("copy/a.txt")), "a");
    copy(&root, &rel("src"), &rel("src2")).expect("tree");
    assert_eq!(read(&root.join("src2/a.txt")), "a");
    assert_eq!(read(&root.join("src2/sub/b.txt")), "b");
    assert!(root.join("src2/sub/empty").is_dir());

    let taken = copy(&root, &rel("src/a.txt"), &rel("copy/a.txt"));
    assert_eq!(code(taken), ErrorCode::Conflict);
    let missing = copy(&root, &rel("nope"), &rel("x"));
    assert_eq!(code(missing), ErrorCode::NotFound);
}

#[test]
fn trash_checks_every_path_before_trashing_any() {
    let (_tmp, root, _) = project();
    fs::write(root.join("b.txt"), "b").expect("write");
    let (fake, seen) = recording();

    let missing = trash(&root, &[rel("b.txt"), rel("gone")], &fake);
    assert_eq!(code(missing), ErrorCode::NotFound);
    assert!(seen.lock().expect("lock").is_empty());

    trash(&root, &[rel("src/a.txt"), rel("b.txt")], &fake).expect("trash");
    let real = fs::canonicalize(&root).expect("canonical");
    let expected = vec![real.join("src").join("a.txt"), real.join("b.txt")];
    assert_eq!(*seen.lock().expect("lock"), expected);
}

#[test]
fn a_failing_trash_is_an_internal_error_and_stops() {
    let (_tmp, root, _) = project();
    let calls = Arc::new(Mutex::new(0));
    let count = Arc::clone(&calls);
    let fake = Trash::fake(move |_| {
        *count.lock().expect("lock") += 1;
        Err("no trash here".into())
    });
    let failed = trash(&root, &[rel("src/a.txt"), rel("src")], &fake);
    assert_eq!(code(failed), ErrorCode::Internal);
    assert_eq!(*calls.lock().expect("lock"), 1);
}

#[tokio::test]
async fn the_service_runs_each_operation() {
    let (_tmp, root, _) = project();
    let (fake, seen) = recording();
    let fs = FsIndex::with_trash(fake);
    fs.create(root.clone(), rel("n.txt"), false)
        .await
        .expect("create");
    fs.rename(root.clone(), rel("n.txt"), rel("m.txt"))
        .await
        .expect("rename");
    fs.copy(root.clone(), rel("m.txt"), rel("k.txt"))
        .await
        .expect("copy");
    fs.trash(root.clone(), vec![rel("k.txt")])
        .await
        .expect("trash");
    assert!(root.join("m.txt").is_file() && root.join("k.txt").is_file());
    assert_eq!(seen.lock().expect("lock").len(), 1);
}

#[cfg(unix)]
mod links {
    use std::os::unix::fs::symlink;

    use super::*;

    #[test]
    fn a_parent_link_out_of_the_root_is_refused() {
        let (_tmp, root, out) = project();
        symlink(&out, root.join("out")).expect("symlink");
        symlink(out.join("missing"), root.join("dangling")).expect("symlink");
        let (fake, seen) = recording();

        for path in ["out/x", "out/new/deep/x", "dangling/x"] {
            assert_eq!(
                code(create(&root, &rel(path), false)),
                ErrorCode::Forbidden,
                "{path}"
            );
            let moved = rename(&root, &rel("src/a.txt"), &rel(path));
            assert_eq!(code(moved), ErrorCode::Forbidden, "{path}");
            let copied = copy(&root, &rel("src/a.txt"), &rel(path));
            assert_eq!(code(copied), ErrorCode::Forbidden, "{path}");
        }
        let taken = rename(&root, &rel("out/secret.txt"), &rel("stolen.txt"));
        assert_eq!(code(taken), ErrorCode::Forbidden);
        let copied = copy(&root, &rel("out/secret.txt"), &rel("stolen.txt"));
        assert_eq!(code(copied), ErrorCode::Forbidden);
        let trashed = trash(&root, &[rel("src/a.txt"), rel("out/secret.txt")], &fake);
        assert_eq!(code(trashed), ErrorCode::Forbidden);
        assert!(seen.lock().expect("lock").is_empty());

        let mut left: Vec<_> = fs::read_dir(&out)
            .expect("list")
            .map(|e| e.expect("entry").file_name())
            .collect();
        left.sort();
        assert_eq!(left, ["secret.txt"]);
        assert_eq!(read(&root.join("src/a.txt")), "a");
    }

    #[test]
    fn a_link_into_git_is_refused() {
        let (_tmp, root, _) = project();
        fs::create_dir_all(root.join(".git/hooks")).expect("mkdir");
        symlink(root.join(".git"), root.join("g")).expect("symlink");
        let planted = create(&root, &rel("g/hooks/pre-commit"), false);
        assert_eq!(code(planted), ErrorCode::Forbidden);
        let moved = rename(&root, &rel("src/a.txt"), &rel("g/hooks/pre-commit"));
        assert_eq!(code(moved), ErrorCode::Forbidden);
        assert!(!root.join(".git/hooks/pre-commit").exists());
    }

    #[test]
    fn links_are_moved_copied_and_trashed_not_followed() {
        let (_tmp, root, out) = project();
        symlink(out.join("secret.txt"), root.join("l")).expect("symlink");
        symlink(&out, root.join("dir-link")).expect("symlink");

        rename(&root, &rel("l"), &rel("m")).expect("move link");
        assert!(
            fs::symlink_metadata(root.join("m"))
                .expect("m")
                .is_symlink()
        );
        assert_eq!(read(&out.join("secret.txt")), "s");

        copy(&root, &rel("m"), &rel("n")).expect("copy link");
        assert_eq!(
            fs::read_link(root.join("n")).ok(),
            Some(out.join("secret.txt"))
        );
        fs::rename(root.join("dir-link"), root.join("src/dir-link")).expect("move");
        copy(&root, &rel("src"), &rel("src2")).expect("copy tree");
        assert_eq!(
            fs::read_link(root.join("src2/dir-link")).ok(),
            Some(out.clone())
        );

        let taken = create(&root, &rel("m"), false);
        assert_eq!(code(taken), ErrorCode::Conflict);

        let (fake, seen) = recording();
        trash(&root, &[rel("m")], &fake).expect("trash link");
        let real = fs::canonicalize(&root).expect("canonical");
        assert_eq!(*seen.lock().expect("lock"), [real.join("m")]);
    }

    #[test]
    fn a_folder_cannot_go_inside_itself_through_a_link() {
        let (_tmp, root, _) = project();
        symlink(root.join("src"), root.join("alias")).expect("symlink");
        let copied = copy(&root, &rel("src"), &rel("alias/again"));
        assert_eq!(code(copied), ErrorCode::BadRequest);
        let moved = rename(&root, &rel("src"), &rel("alias/again"));
        assert_eq!(code(moved), ErrorCode::BadRequest);
        assert!(!root.join("src/again").exists());
        // The link itself may move into the folder it points to.
        rename(&root, &rel("alias"), &rel("alias/again")).expect("move link");
        assert!(
            fs::symlink_metadata(root.join("src/again"))
                .expect("link")
                .is_symlink()
        );
    }
}
