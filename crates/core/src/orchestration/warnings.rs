//! `review.warnings`: what an agent's diff does to the project's tests and
//! checks (`docs/specs/review-check.md`, "Weakened checks"). Agents that
//! cannot make a test pass sometimes edit the test, skip it or silence the
//! check instead, then say they are done. Ovra reads the diff, not the
//! agent's words, and points the person at those lines. It never blocks
//! anything: a test changed on purpose is normal work.

use std::collections::BTreeMap;

use super::review::{self, Entry};
use crate::Core;
use crate::path::RelPath;
use crate::protocol::ProtocolError;
use crate::protocol::review::{
    ChangeKind, ChangedBy, FileChange, ReviewWarning, ReviewWarningKind,
};

/// At most this many line warnings per file and kind: enough to see the
/// pattern, without a wall of text for a big rewrite.
const PER_FILE: usize = 3;

/// The longest line text shown, in characters.
const MAX_TEXT: usize = 160;

/// `review.warnings`: the warnings for `entry`'s item as it is now, or as
/// it was decided (from the diffs kept with the decision).
pub(super) async fn of(core: &Core, entry: &Entry) -> Result<Vec<ReviewWarning>, ProtocolError> {
    if !review::live(entry.state) {
        let (files, diffs) = review::decided(entry);
        return Ok(scan(
            files
                .iter()
                .map(|f| (f, diffs.get(&f.path).map(String::as_str))),
        ));
    }
    let (files, _, tree) = review::listed(core, entry, None).await?;
    let paths: Vec<RelPath> = files
        .iter()
        .filter(|f| f.by != ChangedBy::Outside && f.kind != ChangeKind::Deleted)
        .filter_map(|f| RelPath::parse(&f.path).ok())
        .collect();
    let patches = match tree {
        Some(tree) if !paths.is_empty() => {
            let ws = &entry.workspace;
            core.worktrees()
                .patches(ws.dir.clone(), ws.base.clone(), tree, paths)
                .await?
        }
        _ => BTreeMap::new(),
    };
    Ok(scan(
        files
            .iter()
            .map(|f| (f, patches.get(&f.path).map(String::as_str))),
    ))
}

/// The warnings for `files`, each with its unified diff (`None` when it
/// could not be read). Only the agent's changes count.
pub(super) fn scan<'a>(
    files: impl IntoIterator<Item = (&'a FileChange, Option<&'a str>)>,
) -> Vec<ReviewWarning> {
    let mut out = Vec::new();
    for (file, patch) in files {
        if file.by == ChangedBy::Outside {
            continue;
        }
        let path = file.path.as_str();
        let test = is_test(path);
        if file.kind == ChangeKind::Deleted && test {
            out.push(whole(ReviewWarningKind::TestDeleted, path));
            continue;
        }
        if is_check_config(path) {
            out.push(whole(ReviewWarningKind::CheckConfig, path));
        }
        let Some(patch) = patch else { continue };
        let lines = diff_lines(patch);
        let mut found: Vec<ReviewWarning> = Vec::new();
        let count = |kind, found: &mut Vec<ReviewWarning>, w: ReviewWarning| {
            if found.iter().filter(|f| f.kind == kind).count() < PER_FILE {
                found.push(w);
            }
        };
        let added_asserts = lines
            .iter()
            .filter(|l| l.added && is_assertion(&l.text))
            .count();
        let mut removed_asserts = lines
            .iter()
            .filter(|l| !l.added && is_assertion(&l.text))
            .count()
            .saturating_sub(added_asserts);
        let mut edited = None;
        for line in &lines {
            if line.added {
                if is_skip(&line.text) {
                    let kind = ReviewWarningKind::TestSkipped;
                    count(kind, &mut found, at(kind, path, line));
                } else if is_silenced(path, &line.text) {
                    let kind = ReviewWarningKind::CheckSilenced;
                    count(kind, &mut found, at(kind, path, line));
                }
            } else {
                if removed_asserts > 0 && is_assertion(&line.text) {
                    removed_asserts -= 1;
                    let kind = ReviewWarningKind::AssertionRemoved;
                    count(kind, &mut found, at(kind, path, line));
                }
                if test && file.kind != ChangeKind::Added && edited.is_none() {
                    edited = Some(line);
                }
            }
        }
        // One line for a test file whose own lines changed, unless its
        // removed assertions already say it.
        if let Some(line) = edited
            && !found
                .iter()
                .any(|f| f.kind == ReviewWarningKind::AssertionRemoved)
        {
            found.push(at(ReviewWarningKind::TestEdited, path, line));
        }
        found.sort_by_key(|w| (w.kind, w.line));
        out.extend(found);
    }
    out
}

fn whole(kind: ReviewWarningKind, path: &str) -> ReviewWarning {
    ReviewWarning {
        kind,
        path: path.to_owned(),
        line: None,
        old: false,
        text: None,
    }
}

fn at(kind: ReviewWarningKind, path: &str, line: &Line) -> ReviewWarning {
    ReviewWarning {
        kind,
        path: path.to_owned(),
        line: Some(line.number),
        old: !line.added,
        text: Some(shown(&line.text)),
    }
}

/// The line trimmed, without control or direction characters, cut.
fn shown(text: &str) -> String {
    let clean: String = text
        .trim()
        .chars()
        .filter(|c| !crate::untrusted::hides_text(*c))
        .collect();
    match clean.char_indices().nth(MAX_TEXT) {
        Some((cut, _)) => format!("{}…", &clean[..cut]),
        None => clean,
    }
}

/// An added or removed line of a diff, numbered in its own version.
#[derive(Debug)]
struct Line {
    added: bool,
    number: u32,
    text: String,
}

fn diff_lines(patch: &str) -> Vec<Line> {
    let mut out = Vec::new();
    let (mut old, mut new) = (0u32, 0u32);
    let mut in_hunk = false;
    for raw in patch.lines() {
        if let Some(head) = raw.strip_prefix("@@ ") {
            let mut parts = head.split_whitespace();
            let start = |p: Option<&str>, sign: char| {
                p.and_then(|p| p.strip_prefix(sign))
                    .and_then(|p| p.split(',').next())
                    .and_then(|n| n.parse::<u32>().ok())
                    .unwrap_or(1)
            };
            old = start(parts.next(), '-');
            new = start(parts.next(), '+');
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        if let Some(text) = raw.strip_prefix('+') {
            out.push(Line {
                added: true,
                number: new,
                text: text.to_owned(),
            });
            new += 1;
        } else if let Some(text) = raw.strip_prefix('-') {
            out.push(Line {
                added: false,
                number: old,
                text: text.to_owned(),
            });
            old += 1;
        } else if raw.starts_with(' ') || raw.is_empty() {
            old += 1;
            new += 1;
        } else if !raw.starts_with('\\') {
            // `diff --git` of the next file in the same patch.
            in_hunk = false;
        }
    }
    out
}

/// A test file by the usual names: a `tests` folder, `*.test.*`,
/// `*_test.*`, `test_*.py` and the like.
pub(super) fn is_test(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let mut parts = lower.split('/').collect::<Vec<_>>();
    let name = parts.pop().unwrap_or_default();
    if parts.iter().any(|p| {
        matches!(
            *p,
            "test" | "tests" | "__tests__" | "spec" | "specs" | "e2e" | "testdata"
        )
    }) {
        return true;
    }
    let stem = name.split('.').next().unwrap_or_default();
    name.contains(".test.")
        || name.contains(".spec.")
        || stem.ends_with("_test")
        || stem.ends_with("_spec")
        || stem.ends_with("_tests")
        || (stem.starts_with("test_") && name.ends_with(".py"))
        || path.rsplit('/').next().is_some_and(|n| {
            let stem = n.split('.').next().unwrap_or_default();
            stem.len() > 4 && (stem.ends_with("Test") || stem.ends_with("Tests"))
        })
}

/// A file that sets up what the project checks: CI, lint, format, type and
/// test runner config.
pub(super) fn is_check_config(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or_default();
    if lower.starts_with(".github/workflows/")
        || lower.starts_with(".circleci/")
        || lower.starts_with(".config/nextest")
    {
        return true;
    }
    let stem = name.split('.').next().unwrap_or_default();
    matches!(
        name,
        "justfile"
            | ".justfile"
            | "makefile"
            | ".gitlab-ci.yml"
            | "azure-pipelines.yml"
            | "clippy.toml"
            | ".clippy.toml"
            | "rustfmt.toml"
            | ".rustfmt.toml"
            | "deny.toml"
            | "biome.json"
            | "biome.jsonc"
            | ".pre-commit-config.yaml"
            | "pytest.ini"
            | "tox.ini"
            | "setup.cfg"
            | ".coveragerc"
            | "codecov.yml"
            | ".golangci.yml"
            | ".golangci.yaml"
            | ".rubocop.yml"
            | ".swiftlint.yml"
            | ".flake8"
            | "mypy.ini"
            | "ruff.toml"
            | ".ruff.toml"
            | ".stylelintrc"
            | ".editorconfig"
    ) || name.starts_with(".eslintrc")
        || name.starts_with(".prettierrc")
        || name.starts_with(".stylelintrc")
        || name.starts_with("tsconfig") && name.ends_with(".json")
        || matches!(
            stem,
            "eslint" | "vitest" | "jest" | "playwright" | "karma" | "cypress"
        ) && name.contains(".config.")
}

/// A test switched off, or the others switched off with `only`.
fn is_skip(text: &str) -> bool {
    let t = text.trim_start();
    const CALLS: [&str; 14] = [
        ".skip(",
        ".only(",
        ".todo(",
        ".fixme(",
        ".skipIf(",
        "xit(",
        "xdescribe(",
        "xtest(",
        "fit(",
        "fdescribe(",
        "pytest.skip(",
        "t.Skip(",
        "t.SkipNow(",
        "self.skipTest(",
    ];
    const MARKS: [&str; 9] = [
        "#[ignore",
        "@pytest.mark.skip",
        "@pytest.mark.xfail",
        "@unittest.skip",
        "@unittest.expectedFailure",
        "@Disabled",
        "@Ignore",
        "[Ignore",
        "[Fact(Skip",
    ];
    MARKS.iter().any(|m| t.starts_with(m))
        || CALLS.iter().any(|c| {
            t.match_indices(c).any(|(at, _)| {
                // `xit(`, `fit(` only as whole words: not `exit(` or `profit(`.
                !c.starts_with(|ch: char| ch.is_ascii_alphabetic())
                    || t[..at]
                        .chars()
                        .next_back()
                        .is_none_or(|b| !b.is_alphanumeric() && b != '_')
            })
        })
}

/// A lint, type or compiler check silenced on a line or a file.
fn is_silenced(path: &str, text: &str) -> bool {
    const MARKS: [&str; 22] = [
        "eslint-disable",
        "@ts-ignore",
        "@ts-nocheck",
        "@ts-expect-error",
        "svelte-ignore",
        "biome-ignore",
        "deno-lint-ignore",
        "oxlint-disable",
        "#[allow(",
        "#![allow(",
        "#[expect(",
        "#![expect(",
        "# noqa",
        "#noqa",
        "type: ignore",
        "pylint: disable",
        "pyright: ignore",
        "nolint",
        "NOLINT",
        "@SuppressWarnings",
        "rubocop:disable",
        "#pragma warning disable",
    ];
    if MARKS.iter().any(|m| text.contains(m)) {
        return true;
    }
    // A lint turned off in a Cargo.toml `[lints]` table.
    let name = path.rsplit('/').next().unwrap_or_default();
    name == "Cargo.toml" && text.contains('=') && text.contains("\"allow\"")
}

/// A line that asserts something in a test.
fn is_assertion(text: &str) -> bool {
    let t = text.trim_start();
    if t.starts_with("//") || t.starts_with('#') && !t.starts_with("#[") {
        return false;
    }
    t.contains("assert")
        || t.contains("expect(")
        || t.contains(".should")
        || t.contains(".to_be(")
        || t.contains("XCTAssert")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str, kind: ChangeKind) -> FileChange {
        FileChange {
            path: path.into(),
            kind,
            from: None,
            by: ChangedBy::Agent,
            additions: 0,
            deletions: 0,
            blocked: false,
            protected: false,
        }
    }

    fn scan_one(path: &str, kind: ChangeKind, patch: &str) -> Vec<ReviewWarning> {
        let file = change(path, kind);
        scan([(&file, Some(patch))])
    }

    #[test]
    fn test_files_are_known_by_their_usual_names() {
        for path in [
            "src/a.test.ts",
            "ui/src/Inbox.spec.js",
            "tests/e2e/agents.ts",
            "crates/core/src/orchestration/tests/check.rs",
            "pkg/server_test.go",
            "app/test_models.py",
            "src/test/java/FooTest.java",
            "spec/user_spec.rb",
            "src/__tests__/x.js",
        ] {
            assert!(is_test(path), "{path}");
        }
        for path in [
            "src/latest.rs",
            "src/contest.ts",
            "testing.md",
            "src/Test.java",
        ] {
            assert!(!is_test(path), "{path}");
        }
    }

    #[test]
    fn check_config_is_ci_lint_and_test_runner_setup() {
        for path in [
            ".github/workflows/ci.yml",
            "justfile",
            "ui/eslint.config.js",
            "ui/vitest.config.ts",
            "tsconfig.json",
            "ui/tsconfig.node.json",
            ".config/nextest.toml",
            "deny.toml",
            ".eslintrc.cjs",
            "pytest.ini",
        ] {
            assert!(is_check_config(path), "{path}");
        }
        for path in ["src/config.ts", "package.json", "README.md", "src/jest.ts"] {
            assert!(!is_check_config(path), "{path}");
        }
    }

    #[test]
    fn skips_and_silenced_checks_are_whole_marks_not_words() {
        for line in [
            "  it.skip(\"works\", () => {",
            "test.only(\"x\", fn)",
            "xit('later', () => {})",
            "    #[ignore]",
            "@pytest.mark.skip(reason='flaky')",
            "\tt.Skip(\"later\")",
            "describe.skip('all')",
        ] {
            assert!(is_skip(line), "{line}");
        }
        for line in [
            "std::process::exit(1)",
            "let profit(x) = 1",
            "skip(3)",
            "fn only()",
        ] {
            assert!(!is_skip(line), "{line}");
        }
        assert!(is_silenced(
            "a.ts",
            "// eslint-disable-next-line no-explicit-any"
        ));
        assert!(is_silenced("a.rs", "#[allow(dead_code)]"));
        assert!(is_silenced("a.py", "x = f()  # noqa: E501"));
        assert!(is_silenced("Cargo.toml", "unreachable_pub = \"allow\""));
        assert!(!is_silenced("Cargo.toml", "edition = \"2024\""));
        assert!(!is_silenced("a.rs", "let allowed = true;"));
    }

    const RUST_TEST: &str = "\
diff --git a/crates/x/tests/it.rs b/crates/x/tests/it.rs
--- a/crates/x/tests/it.rs
+++ b/crates/x/tests/it.rs
@@ -10,6 +10,7 @@ fn setup() {}

 #[test]
+#[ignore]
 fn adds() {
-    assert_eq!(add(2, 2), 4);
-    assert!(add(0, 0) == 0);
+    assert!(add(2, 2) > 0);
 }
";

    #[test]
    fn a_weakened_test_shows_its_skip_and_the_assertion_it_lost() {
        let found = scan_one("crates/x/tests/it.rs", ChangeKind::Modified, RUST_TEST);
        insta::assert_json_snapshot!(found);
    }

    #[test]
    fn new_tests_and_new_assertions_are_not_warnings() {
        let patch = "\
@@ -1,3 +1,5 @@
 fn a() {
-    assert_eq!(a(), 1);
+    assert_eq!(a(), 1, \"a is one\");
+    assert_eq!(b(), 2);
 }
";
        assert_eq!(scan_one("src/lib.rs", ChangeKind::Modified, patch), vec![]);
        let added = "@@ -0,0 +1,2 @@\n+it('works', () => {\n+});\n";
        assert_eq!(scan_one("src/a.test.ts", ChangeKind::Added, added), vec![]);
    }

    #[test]
    fn a_deleted_test_and_changed_config_are_whole_file_warnings() {
        let gone = scan_one("src/a.test.ts", ChangeKind::Deleted, "@@ -1 +0,0 @@\n-x\n");
        assert_eq!(
            gone,
            vec![whole(ReviewWarningKind::TestDeleted, "src/a.test.ts")]
        );
        let patch = "@@ -3,1 +3,1 @@\n-  \"strict\": true,\n+  \"strict\": false,\n";
        let config = scan_one("tsconfig.json", ChangeKind::Modified, patch);
        assert_eq!(
            config,
            vec![whole(ReviewWarningKind::CheckConfig, "tsconfig.json")]
        );
    }

    #[test]
    fn an_edited_test_without_lost_assertions_says_so_once() {
        let patch = "@@ -4,2 +4,2 @@\n-  const want = 3;\n-  const got = 4;\n+  const want = 4;\n+  const got = 4;\n";
        let found = scan_one("src/a.test.ts", ChangeKind::Modified, patch);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ReviewWarningKind::TestEdited);
        assert_eq!((found[0].line, found[0].old), (Some(4), true));
    }

    #[test]
    fn lines_are_cut_and_cleaned_and_each_kind_is_capped_per_file() {
        let long = format!("+// eslint-disable-line \u{202e}{}\n", "x".repeat(400));
        let patch = format!("@@ -1,0 +1,5 @@\n{}", long.repeat(5));
        let found = scan_one("src/a.ts", ChangeKind::Modified, &patch);
        assert_eq!(found.len(), PER_FILE);
        let text = found[0].text.clone().unwrap_or_default();
        assert!(!text.contains('\u{202e}'), "{text}");
        assert_eq!(text.chars().count(), MAX_TEXT + 1);
    }

    #[test]
    fn the_persons_own_changes_are_not_the_agents() {
        let mut file = change("src/a.test.ts", ChangeKind::Deleted);
        file.by = ChangedBy::Outside;
        assert_eq!(scan([(&file, None)]), vec![]);
    }
}
