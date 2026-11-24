use std::fs;
use std::path::Path;

use ovra_core::Core;
use ovra_core::protocol::search::{
    FileMatches, LineMatch, MAX_SEARCH_LINES, SearchEvent, SearchId, SearchQuery,
};
use ovra_core::protocol::{ErrorCode, Event};
use ovra_core::services::{FsService, SearchCancel};

use super::line_match;
use crate::FsIndex;

fn put(root: &Path, path: &str, text: impl AsRef<[u8]>) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("has parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

fn query(pattern: &str) -> SearchQuery {
    SearchQuery {
        pattern: pattern.into(),
        regex: false,
        case_sensitive: false,
        whole_word: false,
        include: vec![],
        exclude: vec![],
    }
}

/// What one search reported: files sorted by path, and the `search.done`.
#[derive(Debug)]
struct Outcome {
    files: Vec<FileMatches>,
    done: SearchEvent,
}

fn search_with(root: &Path, q: SearchQuery, cancel: SearchCancel) -> Outcome {
    let core = Core::new("test");
    let mut rx = core.subscribe();
    FsIndex::new()
        .search(SearchId(7), root.to_path_buf(), q, core.sink(), cancel)
        .expect("search starts");
    let mut files = Vec::new();
    loop {
        match rx.blocking_recv().expect("events until done") {
            Event::Search(SearchEvent::Results { id, files: more }) => {
                assert_eq!(id, SearchId(7));
                files.extend(more);
            }
            Event::Search(done @ SearchEvent::Done { .. }) => {
                files.sort_by(|a, b| a.path.cmp(&b.path));
                return Outcome { files, done };
            }
            other => panic!("{other:?}"),
        }
    }
}

fn search(root: &Path, q: SearchQuery) -> Outcome {
    search_with(root, q, SearchCancel::default())
}

/// `path:line` of every match, for short assertions.
fn hits(outcome: &Outcome) -> Vec<String> {
    outcome
        .files
        .iter()
        .flat_map(|f| {
            f.lines
                .iter()
                .map(move |l| format!("{}:{}", f.path, l.line))
        })
        .collect()
}

#[test]
fn finds_text_in_files_that_are_not_ignored() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    put(root, ".gitignore", "target/\n");
    put(
        root,
        "src/main.rs",
        "fn main() {\r\n    println!(\"Hello\");\r\n}\r\n",
    );
    put(root, "notes.md", "say hello, h\u{e9}llo, \u{1f600} hello\n");
    put(root, "target/out.txt", "hello\n");
    put(root, "logo.bin", b"hello\0binary".as_slice());
    let outcome = search(root, query("hello"));
    insta::assert_json_snapshot!(outcome.files);
    let SearchEvent::Done {
        files,
        lines,
        truncated,
        cancelled,
        ..
    } = outcome.done
    else {
        panic!("{:?}", outcome.done);
    };
    assert_eq!((files, lines, truncated, cancelled), (2, 2, false, false));
}

#[cfg(unix)]
#[test]
fn symlinks_are_not_followed() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let outside = tempfile::tempdir().expect("temp dir");
    put(outside.path(), "secret.txt", "hello\n");
    let root = tmp.path();
    put(root, "a.txt", "hello\n");
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), root.join("link.txt"))
        .expect("link");
    assert_eq!(hits(&search(root, query("hello"))), ["a.txt:1"]);
}

#[test]
fn regex_case_and_whole_word() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    put(root, "a.txt", "Foo\nfoo\nfoobar\nfoo_bar\nbar foo.\n");
    let mut q = query("foo");
    q.case_sensitive = true;
    q.whole_word = true;
    assert_eq!(hits(&search(root, q)), ["a.txt:2", "a.txt:5"]);
    let mut q = query("^fo+b");
    q.regex = true;
    assert_eq!(hits(&search(root, q)), ["a.txt:3"]);
    // Without `regex`, regex syntax is plain text.
    assert!(hits(&search(root, query("^fo+b"))).is_empty());
}

#[test]
fn include_and_exclude_globs() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    for path in ["a.rs", "src/b.rs", "src/c.ts", "src/gen/d.rs", "docs/e.md"] {
        put(root, path, "x\n");
    }
    let found = |include: &[&str], exclude: &[&str]| {
        let mut q = query("x");
        q.include = include.iter().map(|s| (*s).to_owned()).collect();
        q.exclude = exclude.iter().map(|s| (*s).to_owned()).collect();
        hits(&search(root, q))
    };
    assert_eq!(
        found(&["*.rs"], &[]),
        ["a.rs:1", "src/b.rs:1", "src/gen/d.rs:1"]
    );
    assert_eq!(found(&["src"], &["gen", "*.ts"]), ["src/b.rs:1"]);
    assert_eq!(found(&["/src/*.rs"], &[]), ["src/b.rs:1"]);
    assert_eq!(found(&["docs/", " "], &[]), ["docs/e.md:1"]);
}

#[test]
fn a_query_that_cannot_run_is_a_bad_request() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let fs = FsIndex::new();
    let core = Core::new("test");
    let mut rx = core.subscribe();
    let mut bad_regex = query("(");
    bad_regex.regex = true;
    let mut bad_glob = query("x");
    bad_glob.include = vec!["src/{a".into()];
    for q in [bad_regex, bad_glob] {
        let started = fs.search(
            SearchId(1),
            tmp.path().to_path_buf(),
            q,
            core.sink(),
            SearchCancel::default(),
        );
        assert_eq!(started.map_err(|e| e.code), Err(ErrorCode::BadRequest));
    }
    assert!(rx.try_recv().is_err());
}

#[test]
fn stops_at_the_line_limit() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    put(
        root,
        "many.txt",
        "x\n".repeat(MAX_SEARCH_LINES as usize + 5),
    );
    let outcome = search(root, query("x"));
    let SearchEvent::Done {
        files,
        lines,
        truncated,
        ..
    } = outcome.done
    else {
        panic!("{:?}", outcome.done);
    };
    assert_eq!((files, lines, truncated), (1, MAX_SEARCH_LINES, true));
    assert_eq!(outcome.files[0].lines.len(), MAX_SEARCH_LINES as usize);
}

#[test]
fn a_cancelled_search_reports_nothing_more() {
    let tmp = tempfile::tempdir().expect("temp dir");
    put(tmp.path(), "a.txt", "x\n");
    let cancel = SearchCancel::default();
    cancel.cancel();
    let outcome = search_with(tmp.path(), query("x"), cancel);
    assert!(outcome.files.is_empty());
    assert!(matches!(
        outcome.done,
        SearchEvent::Done {
            cancelled: true,
            lines: 0,
            ..
        }
    ));
}

#[test]
fn long_lines_are_cut_around_the_first_match() {
    let text = format!("{}needle{}", "\u{e9}".repeat(300), "b".repeat(300));
    let start = 600; // bytes before the match
    let cut = line_match(3, &text, &[(start, start + 6), (start + 500, start + 506)]);
    assert_eq!(
        cut.column, 260,
        "80 bytes (40 two-byte chars) before the match"
    );
    assert!(cut.text.starts_with("\u{e9}\u{e9}") && cut.text.len() <= 400);
    assert_eq!(
        cut.ranges,
        [[40, 46]],
        "the second match is outside the window"
    );
    let short = line_match(1, "a\u{1f600}b", &[(5, 6)]);
    assert_eq!(
        short,
        LineMatch {
            line: 1,
            text: "a\u{1f600}b".into(),
            column: 0,
            ranges: vec![[3, 4]]
        }
    );
}
