//! `git.diff` of one file, parsed into hunks, and `git.stage_hunk`, which
//! turns a hunk back into a patch for the index.

use std::path::Path;

use ovra_core::path::RelPath;
use ovra_core::protocol::git::{FileDiff, Hunk, MAX_DIFF_LINES};
use ovra_core::services::Result;

use super::{conflict, printed};
use crate::files::{self, OnDisk};
use crate::git::{self, Git, internal};

/// The person's diff drivers, text conversions and colours never apply.
const DIFF: [&str; 4] = ["--no-ext-diff", "--no-textconv", "--no-color", "-U3"];

pub(super) fn diff(root: &Path, path: &RelPath, staged: bool) -> Result<FileDiff> {
    git::dirs(root)?;
    let mut run = Git::new(root, "diff")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(DIFF);
    let mut from = None;
    if staged {
        from = renamed_from(root, path)?;
        run = run.args(["--cached", "-M"]);
    } else {
        run = run.arg("--no-renames");
    }
    let mut out = run
        .arg("--")
        .args(from.as_deref())
        .arg(path.as_str())
        .run()?;
    if out.is_empty() && !staged && untracked(root, path)? {
        out = added(root, path)?;
    }
    Ok(parse(path.as_str(), from, &out))
}

/// The old path, relative to the project, of a staged rename to `path`.
fn renamed_from(root: &Path, path: &RelPath) -> Result<Option<String>> {
    let out = Git::new(root, "diff")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["--cached", "-M", "--name-status", "-z", "--relative"])
        .run()?;
    let fields: Vec<String> = git::fields(&out).collect();
    let mut i = 0;
    while i < fields.len() {
        // `R<score>\0<old>\0<new>`, `C<score>\0..` or `<status>\0<path>`.
        let pair = matches!(fields[i].as_bytes().first(), Some(b'R' | b'C'));
        if pair && fields.get(i + 2).is_some_and(|new| new == path.as_str()) {
            return Ok(fields
                .get(i + 1)
                .cloned()
                .filter(|_| fields[i].starts_with('R')));
        }
        i += if pair { 3 } else { 2 };
    }
    Ok(None)
}

/// A file on disk, not under a symlinked folder, that the index lacks.
fn untracked(root: &Path, path: &RelPath) -> Result<bool> {
    if files::on_disk(root, path)? != OnDisk::File {
        return Ok(false);
    }
    let listed = Git::new(root, "ls-files")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["-z", "--cached", "--"])
        .arg(path.as_str())
        .run()?;
    Ok(listed.is_empty())
}

/// An untracked file as all added.
fn added(root: &Path, path: &RelPath) -> Result<Vec<u8>> {
    let out = Git::new(root, "diff")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("--no-index")
        .args(DIFF)
        .args(["--", "/dev/null", path.as_str()])
        .output()?;
    // 1: the files differ, as they do unless the file is empty.
    match out.status.code() {
        Some(0 | 1) => Ok(out.stdout),
        _ => Err(internal(format!("git diff: {}", printed(&out)))),
    }
}

/// `@@ -a[,b] +c[,d] @@ header`
fn hunk_header(line: &str) -> Option<Hunk> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, header) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let range = |r: &str| -> Option<(u32, u32)> {
        match r.split_once(',') {
            Some((start, lines)) => Some((start.parse().ok()?, lines.parse().ok()?)),
            None => Some((r.parse().ok()?, 1)),
        }
    };
    let ((old_start, old_lines), (new_start, new_lines)) = (range(old)?, range(new)?);
    Some(Hunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        header: header.strip_prefix(' ').unwrap_or(header).to_owned(),
        lines: Vec::new(),
    })
}

/// The hunks of a one-file diff. Each hunk takes as many lines as its
/// header counts, so a changed line that looks like a header stays a line.
pub(super) fn parse(path: &str, from: Option<String>, out: &[u8]) -> FileDiff {
    let text = String::from_utf8_lossy(out);
    let mut diff = FileDiff {
        path: path.to_owned(),
        from,
        ..FileDiff::default()
    };
    let mut lines = text.split('\n').peekable();
    let mut total = 0;
    while let Some(line) = lines.next() {
        if line.starts_with("Binary files ") {
            diff.binary = true;
            continue;
        }
        let Some(mut hunk) = hunk_header(line) else {
            continue;
        };
        let (mut old, mut new) = (hunk.old_lines, hunk.new_lines);
        while old > 0 || new > 0 || lines.peek().is_some_and(|l| l.starts_with('\\')) {
            let Some(line) = lines.next() else {
                break;
            };
            if total == MAX_DIFF_LINES {
                diff.truncated = true;
                break;
            }
            match line.as_bytes().first() {
                Some(b'+') => new = new.saturating_sub(1),
                Some(b'-') => old = old.saturating_sub(1),
                Some(b'\\') => {}
                // `diff.suppressBlankEmpty` prints an empty context line.
                _ => {
                    old = old.saturating_sub(1);
                    new = new.saturating_sub(1);
                }
            }
            hunk.lines.push(if line.is_empty() {
                " ".to_owned()
            } else {
                line.to_owned()
            });
            total += 1;
        }
        diff.hunks.push(hunk);
        if diff.truncated {
            break;
        }
    }
    diff
}

/// A name in a patch header, quoted as git quotes it when needed.
fn patch_name(side: char, path: &str) -> String {
    let name = format!("{side}/{path}");
    if !name
        .chars()
        .any(|c| c == '"' || c == '\\' || c.is_control())
    {
        return name;
    }
    let mut quoted = String::from("\"");
    for c in name.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\t' => quoted.push_str("\\t"),
            c if c.is_control() => {
                let mut buf = [0; 4];
                for b in c.encode_utf8(&mut buf).bytes() {
                    quoted.push_str(&format!("\\{b:03o}"));
                }
            }
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// A patch of one file with only `hunk`; `path` is from the repository top.
pub(super) fn patch(path: &str, hunk: &Hunk) -> String {
    let (a, b) = (patch_name('a', path), patch_name('b', path));
    let mut patch = format!(
        "diff --git {a} {b}\n--- {a}\n+++ {b}\n@@ -{},{} +{},{} @@\n",
        hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
    );
    for line in &hunk.lines {
        patch.push_str(line);
        patch.push('\n');
    }
    patch
}

pub(super) fn stage_hunk(root: &Path, path: &RelPath, hunk: &Hunk, unstage: bool) -> Result<()> {
    // The core checked it too; a patch must never reach other files.
    hunk.check()?;
    let dirs = git::dirs(root)?;
    let mut run = Git::new(&dirs.top, "apply").args(["--cached", "--whitespace=nowarn"]);
    if unstage {
        run = run.arg("--reverse");
    }
    let out = run
        .input(patch(&format!("{}{path}", dirs.prefix), hunk))
        .output()?;
    if !out.status.success() {
        return Err(conflict(format!(
            "the change no longer applies to {path}; look at its diff again: {}",
            printed(&out)
        )));
    }
    Ok(())
}
