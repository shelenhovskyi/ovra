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

type Seen = Arc<Mutex<Vec<PathBuf>>>;

/// A trash that removes what it gets and records it.
fn trash() -> (Trash, Seen) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let trash = Trash::fake(move |path| {
        log.lock().expect("lock").push(path.to_path_buf());
        fs::remove_file(path).map_err(|e| e.to_string())
    });
    (trash, seen)
}

fn trashed(seen: &Seen) -> Vec<PathBuf> {
    seen.lock().expect("lock").clone()
}

/// Imports with `shown` as the names the person was asked about.
fn import_shown(
    root: &Path,
    sources: &[PathBuf],
    into: &str,
    clash: Clash,
    shown: &[&str],
) -> (Result<Imported>, Seen) {
    let (bin, seen) = trash();
    let into = rel(into);
    let to = Target {
        root,
        into: &into,
        clash,
    };
    let shown: Vec<String> = shown.iter().map(|s| (*s).to_owned()).collect();
    (import(&to, sources, &shown, &bin, LIMITS), seen)
}

fn run_import(root: &Path, sources: &[PathBuf], into: &str, clash: Clash) -> Result<Imported> {
    import_shown(root, sources, into, clash, &[]).0
}

fn with_limits(root: &Path, sources: &[PathBuf], limits: Limits) -> Result<Imported> {
    let into = RelPath::root();
    let to = Target {
        root,
        into: &into,
        clash: Clash::Ask,
    };
    import(&to, sources, &[], &trash().0, limits)
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

fn code(done: Result<Imported>) -> ErrorCode {
    done.expect_err("refused").code
}

#[test]
fn copies_files_and_folders_into_the_target() {
    let (_tmp, root, out) = setup();
    let done = run_import(&root, &[out.join("a.png"), out.join("d")], "", Clash::Ask);
    assert_eq!(done.expect("import"), copied(&["a.png", "d"], &[]));
    assert_eq!(read(&root.join("a.png")), "new");
    assert_eq!(read(&root.join("d/x.txt")), "x");
    assert_eq!(read(&out.join("d/x.txt")), "x", "the source stays");
}

#[test]
fn a_clash_copies_nothing_without_an_answer() {
    let (_tmp, root, out) = setup();
    let sources = [out.join("d"), out.join("a.png")];
    let done = run_import(&root, &sources, "src", Clash::Ask).expect("import");
    assert_eq!(done, Imported::Clashes(vec!["a.png".into()]));
    assert!(!root.join("src/d").exists());
    assert_eq!(read(&root.join("src/a.png")), "old");
}

#[test]
fn keep_both_copies_under_a_free_name() {
    let (_tmp, root, out) = setup();
    fs::write(root.join("src/a copy.png"), "older").expect("write");
    fs::create_dir(root.join("src/d")).expect("mkdir");
    // A dropped item named like the copy name is not overwritten either.
    fs::write(out.join("a copy 2.png"), "other").expect("write");
    let sources = [out.join("a.png"), out.join("d"), out.join("a copy 2.png")];
    let done = run_import(&root, &sources, "src", Clash::KeepBoth).expect("import");
    let names = ["src/a copy 3.png", "src/d copy", "src/a copy 2.png"];
    assert_eq!(done, copied(&names, &[]));
    assert_eq!(read(&root.join("src/a.png")), "old");
    assert_eq!(read(&root.join("src/a copy 3.png")), "new");
    assert_eq!(read(&root.join("src/a copy 2.png")), "other");
    assert_eq!(read(&root.join("src/d copy/x.txt")), "x");
}

#[test]
fn replace_trashes_only_the_names_the_person_was_shown() {
    let (_tmp, root, out) = setup();
    let sources = [out.join("a.png")];
    let (done, seen) = import_shown(&root, &sources, "src", Clash::Replace, &[]);
    assert_eq!(code(done), ErrorCode::Conflict);
    assert!(trashed(&seen).is_empty());

    let (done, seen) = import_shown(&root, &sources, "src", Clash::Replace, &["A.PNG"]);
    assert_eq!(done.expect("import"), copied(&["src/a.png"], &[]));
    assert_eq!(read(&root.join("src/a.png")), "new");
    let trashed = trashed(&seen);
    assert_eq!(trashed.len(), 1);
    assert!(trashed[0].ends_with("src/a.png"));
}

#[test]
fn replace_refuses_before_any_change() {
    let (_tmp, root, out) = setup();
    fs::create_dir_all(root.join("src/x")).expect("mkdir");
    fs::write(root.join("src/x/a.png"), "inner").expect("write");
    // `x` would trash the folder the second item is dropped from.
    fs::create_dir(out.join("x")).expect("mkdir");
    let sources = [out.join("x"), root.join("src/x/a.png")];
    let (done, seen) = import_shown(&root, &sources, "src", Clash::Replace, &["x", "a.png"]);
    assert_eq!(code(done), ErrorCode::BadRequest);
    assert!(trashed(&seen).is_empty());
    assert_eq!(read(&root.join("src/x/a.png")), "inner");

    // Nor itself: a file dropped into its own folder.
    let own = [root.join("src/a.png")];
    let (done, seen) = import_shown(&root, &own, "src", Clash::Replace, &["a.png"]);
    assert_eq!(code(done), ErrorCode::BadRequest);
    assert!(trashed(&seen).is_empty());
}

#[test]
fn a_name_that_appears_after_the_plan_is_never_replaced() {
    let (_tmp, root, out) = setup();
    let sources = [out.join("a.png"), out.join("d")];
    for clash in [Clash::Ask, Clash::Replace, Clash::KeepBoth] {
        let (bin, seen) = trash();
        let into = RelPath::root();
        let to = Target {
            root: &root,
            into: &into,
            clash,
        };
        let Planned::Ready(plan) = plan(&to, &sources, &["a.png".into()], LIMITS).expect("plan")
        else {
            panic!("no clash yet");
        };
        fs::write(root.join("a.png"), "theirs").expect("write");
        assert_eq!(code(run(&to, &plan, &bin, LIMITS)), ErrorCode::Conflict);
        assert!(trashed(&seen).is_empty(), "{clash:?}");
        assert_eq!(read(&root.join("a.png")), "theirs");
        fs::remove_file(root.join("a.png")).expect("rm");
    }
}

#[test]
fn names_that_one_folder_may_hold_once_are_refused_together() {
    let (_tmp, root, out) = setup();
    for (one, two) in [("Ä.txt", "ä.txt"), ("\u{e9}.txt", "e\u{301}.txt")] {
        fs::create_dir_all(out.join("1")).expect("mkdir");
        fs::create_dir_all(out.join("2")).expect("mkdir");
        fs::write(out.join("1").join(one), "1").expect("write");
        fs::write(out.join("2").join(two), "2").expect("write");
        let sources = [out.join("1").join(one), out.join("2").join(two)];
        let done = run_import(&root, &sources, "", Clash::Ask);
        assert_eq!(code(done), ErrorCode::BadRequest, "{one} {two}");
        assert!(!root.join(one).exists());
    }
}

#[cfg(unix)]
#[test]
fn links_are_skipped_and_named() {
    let (_tmp, root, out) = setup();
    std::os::unix::fs::symlink("/etc/passwd", out.join("d/passwd")).expect("link");
    std::os::unix::fs::symlink(out.join("d"), out.join("link")).expect("link");
    let done = run_import(&root, &[out.join("d"), out.join("link")], "", Clash::Ask);
    assert_eq!(done.expect("import"), copied(&["d"], &["d/passwd", "link"]));
    assert!(fs::symlink_metadata(root.join("d/passwd")).is_err());
    assert!(fs::symlink_metadata(root.join("link")).is_err());
}

#[cfg(unix)]
#[test]
fn a_socket_refuses_the_whole_drop() {
    let (_tmp, root, out) = setup();
    let _socket = std::os::unix::net::UnixListener::bind(out.join("d/s")).expect("bind");
    let done = run_import(&root, &[out.join("a.png"), out.join("d")], "", Clash::Ask);
    assert_eq!(code(done), ErrorCode::BadRequest);
    assert!(!root.join("a.png").exists(), "nothing was copied");
}

#[cfg(unix)]
#[test]
fn copies_keep_at_most_the_owners_rwx_and_others_rx() {
    use std::os::unix::fs::PermissionsExt;
    let (_tmp, root, out) = setup();
    let mode = |path: &Path| fs::metadata(path).expect("stat").permissions().mode() & 0o7777;
    fs::set_permissions(out.join("a.png"), fs::Permissions::from_mode(0o6777)).expect("chmod");
    fs::set_permissions(out.join("d"), fs::Permissions::from_mode(0o1777)).expect("chmod");
    let done = run_import(&root, &[out.join("a.png"), out.join("d")], "", Clash::Ask);
    done.expect("import");
    assert_eq!(mode(&root.join("a.png")), 0o755);
    assert_eq!(mode(&root.join("d")), 0o755);
}

#[test]
fn more_than_the_limits_is_refused_before_copying() {
    let (_tmp, root, out) = setup();
    fs::create_dir_all(out.join("d/e/f")).expect("mkdir");
    let sources = [out.join("a.png"), out.join("d")];
    let few = Limits {
        entries: 2,
        ..LIMITS
    };
    let small = Limits { bytes: 3, ..LIMITS };
    let shallow = Limits { depth: 2, ..LIMITS };
    for limits in [few, small, shallow] {
        let err = with_limits(&root, &sources, limits).expect_err("refused");
        assert_eq!(err.code, ErrorCode::BadRequest, "{err:?}");
        assert!(!root.join("a.png").exists());
    }
    let deep_enough = Limits { depth: 3, ..LIMITS };
    with_limits(&root, &sources, deep_enough).expect("import");
}

#[test]
fn a_file_that_grew_past_the_limit_is_not_left_half_copied() {
    let (_tmp, root, out) = setup();
    fs::write(out.join("big"), "0123456789").expect("write");
    let sources = [out.join("a.png"), out.join("big")];
    let into = RelPath::root();
    let to = Target {
        root: &root,
        into: &into,
        clash: Clash::Ask,
    };
    let Planned::Ready(plan) = plan(&to, &sources, &[], LIMITS).expect("plan") else {
        panic!("no clash");
    };
    // As if `big` had been 1 byte at the scan: the copy stops at the limit.
    let limits = Limits { bytes: 5, ..LIMITS };
    let err = run(&to, &plan, &trash().0, limits).expect_err("refused");
    assert_eq!(err.code, ErrorCode::BadRequest);
    assert_eq!(read(&root.join("a.png")), "new", "copied before stays");
    assert!(!root.join("big").exists(), "the partial copy is removed");
}

#[test]
fn refuses_targets_and_sources_it_must_not_touch() {
    let (_tmp, root, out) = setup();
    fs::create_dir(root.join(".git")).expect("mkdir");
    fs::create_dir(out.join(".git")).expect("mkdir");
    let file = [out.join("a.png")];
    let refused =
        |sources: &[PathBuf], into: &str| code(run_import(&root, sources, into, Clash::Ask));

    assert_eq!(refused(&file, "src/a.png"), ErrorCode::BadRequest);
    assert_eq!(refused(&file, "missing"), ErrorCode::NotFound);
    assert_eq!(refused(&[out.join(".git")], ""), ErrorCode::BadRequest);
    assert_eq!(
        refused(&[PathBuf::from("a.png")], ""),
        ErrorCode::BadRequest
    );
    // Two items with one name, and a folder into itself.
    assert_eq!(
        refused(&[out.join("a.png"), root.join("src/a.png")], ""),
        ErrorCode::BadRequest
    );
    let into_itself = run_import(&root, &[root.join("src")], "src", Clash::KeepBoth);
    assert_eq!(code(into_itself), ErrorCode::BadRequest);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join(".git"), root.join("g")).expect("link");
        std::os::unix::fs::symlink(&out, root.join("o")).expect("link");
        assert_eq!(refused(&file, "g"), ErrorCode::Forbidden);
        assert_eq!(refused(&file, "o"), ErrorCode::Forbidden);
    }
    assert!(!out.join(".git/a.png").exists() && !root.join(".git/a.png").exists());
}

#[test]
fn errors_name_the_dropped_item_not_its_full_path() {
    let (_tmp, root, out) = setup();
    let gone = run_import(&root, &[out.join("gone.txt")], "", Clash::Ask);
    let message = gone.expect_err("refused").message;
    assert!(message.starts_with("gone.txt"), "{message}");
    assert!(!message.contains(out.to_str().expect("utf-8")), "{message}");
}

#[test]
fn a_drop_waits_for_the_answer_to_its_clash_and_is_then_used_up() {
    let (_tmp, root, out) = setup();
    let (bin, seen) = trash();
    let drops = Drops::new();
    let now = Instant::now();
    let id = drops.put(vec![out.join("a.png")], now);
    let into = rel("src");
    let to = |clash| Target {
        root: &root,
        into: &into,
        clash,
    };
    let ask = import_drop(&drops, &id, now, &to(Clash::Ask), &bin, LIMITS);
    assert_eq!(ask.expect("ask"), Imported::Clashes(vec!["a.png".into()]));
    // The names shown with the question are the ones Replace may replace.
    let replace = import_drop(&drops, &id, now, &to(Clash::Replace), &bin, LIMITS);
    assert_eq!(replace.expect("replace"), copied(&["src/a.png"], &[]));
    assert_eq!(trashed(&seen).len(), 1);
    let again = import_drop(&drops, &id, now, &to(Clash::KeepBoth), &bin, LIMITS);
    assert_eq!(code(again), ErrorCode::NotFound);

    // A refused import uses the drop up too.
    let id = drops.put(vec![out.join("a.png")], now);
    let refused = import_drop(&drops, &id, now, &to(Clash::Replace), &bin, LIMITS);
    assert_eq!(code(refused), ErrorCode::Conflict);
    let again = import_drop(&drops, &id, now, &to(Clash::KeepBoth), &bin, LIMITS);
    assert_eq!(code(again), ErrorCode::NotFound);
}

#[cfg(unix)]
#[test]
fn a_link_swapped_in_after_the_plan_is_skipped_and_named_once() {
    let (_tmp, root, out) = setup();
    std::os::unix::fs::symlink("/etc/passwd", out.join("d/passwd")).expect("link");
    let sources = [out.join("d")];
    let into = RelPath::root();
    let to = Target {
        root: &root,
        into: &into,
        clash: Clash::Ask,
    };
    let Planned::Ready(plan) = plan(&to, &sources, &[], LIMITS).expect("plan") else {
        panic!("no clash");
    };
    fs::remove_file(out.join("d/x.txt")).expect("rm");
    std::os::unix::fs::symlink("/etc/passwd", out.join("d/x.txt")).expect("link");
    let done = run(&to, &plan, &trash().0, LIMITS).expect("import");
    assert_eq!(done, copied(&["d"], &["d/passwd", "d/x.txt"]));
    assert!(fs::symlink_metadata(root.join("d/x.txt")).is_err());
}

type Hook = Box<dyn FnMut(&str)>;

std::thread_local! {
    static ON_OPEN: std::cell::RefCell<Option<Hook>> = std::cell::RefCell::new(None);
}

/// Called by the walk once it opened the dropped folder `shown` (and made
/// its copy), before it lists it: where a test swaps things.
pub(super) fn opened(shown: &str) {
    ON_OPEN.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(shown);
        }
    });
}

/// Plans an import of `sources` into `into`, then runs it with `hook`
/// called as each folder is opened.
fn run_with_hook(
    root: &Path,
    sources: &[PathBuf],
    into: &str,
    hook: impl FnMut(&str) + 'static,
) -> Result<Imported> {
    let into = rel(into);
    let to = Target {
        root,
        into: &into,
        clash: Clash::Ask,
    };
    let Planned::Ready(plan) = plan(&to, sources, &[], LIMITS)? else {
        panic!("no clash");
    };
    ON_OPEN.with(|h| *h.borrow_mut() = Some(Box::new(hook)));
    let done = run(&to, &plan, &trash().0, LIMITS);
    ON_OPEN.with(|h| *h.borrow_mut() = None);
    done
}

#[cfg(unix)]
#[test]
fn a_dropped_folder_swapped_for_a_link_mid_copy_is_not_followed() {
    let (tmp, root, out) = setup();
    let secret = tmp.path().join("ssh");
    fs::create_dir(&secret).expect("mkdir");
    fs::write(secret.join("id_ed25519"), "key").expect("write");
    fs::create_dir(out.join("d/sub")).expect("mkdir");
    fs::write(out.join("d/sub/inner.txt"), "inner").expect("write");
    let sub = out.join("d/sub");
    let done = run_with_hook(&root, &[out.join("d")], "", move |shown| {
        if shown == "d/sub" {
            fs::rename(&sub, sub.with_file_name("sub-old")).expect("rename");
            std::os::unix::fs::symlink(&secret, &sub).expect("link");
        }
    });
    assert_eq!(done.expect("import"), copied(&["d"], &[]));
    assert_eq!(read(&root.join("d/sub/inner.txt")), "inner");
    assert!(!root.join("d/sub/id_ed25519").exists());
}

#[cfg(unix)]
#[test]
fn a_new_folder_swapped_for_a_link_is_not_chmodded_through_it() {
    use std::os::unix::fs::PermissionsExt;
    let (tmp, root, out) = setup();
    let secret = tmp.path().join("id_ed25519");
    fs::write(&secret, "key").expect("write");
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).expect("chmod");
    fs::create_dir(out.join("e")).expect("mkdir");
    let made = root.join("e");
    let link_to = secret.clone();
    let done = run_with_hook(&root, &[out.join("e")], "", move |shown| {
        if shown == "e" {
            fs::rename(&made, made.with_file_name("e-old")).expect("rename");
            std::os::unix::fs::symlink(&link_to, &made).expect("link");
        }
    });
    done.expect("import");
    let mode = fs::metadata(&secret).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert!(root.join("e-old").is_dir());
}

#[cfg(unix)]
#[test]
fn a_target_through_a_link_is_refused_and_a_swap_after_the_plan_is_not_followed() {
    let (_tmp, root, out) = setup();
    let attachments = root.join(".ovra-attachments");
    std::os::unix::fs::symlink(root.join("src"), &attachments).expect("link");
    std::os::unix::fs::symlink("src", root.join("lnk")).expect("link");
    for into in [".ovra-attachments", "lnk"] {
        let done = run_import(&root, &[out.join("d")], into, Clash::Ask);
        assert_eq!(code(done), ErrorCode::Forbidden, "{into}");
    }
    assert!(!root.join("src/d").exists());

    fs::remove_file(&attachments).expect("rm");
    fs::create_dir(&attachments).expect("mkdir");
    let into = rel(".ovra-attachments");
    let to = Target {
        root: &root,
        into: &into,
        clash: Clash::Ask,
    };
    let sources = [out.join("d")];
    let Planned::Ready(plan) = plan(&to, &sources, &[], LIMITS).expect("plan") else {
        panic!("no clash");
    };
    fs::rename(&attachments, root.join("att-old")).expect("rename");
    std::os::unix::fs::symlink("src", &attachments).expect("link");
    run(&to, &plan, &trash().0, LIMITS).expect("import");
    assert!(!root.join("src/d").exists());
    assert_eq!(read(&root.join("att-old/d/x.txt")), "x");
}
