//! `changes` and `patch`: a commit against the working tree, untracked files
//! included, through a tree written from a temporary index.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ovra_core::path::RelPath;
use ovra_core::protocol::review::ChangeKind;
use ovra_core::services::{Delta, Result};

use crate::git::{self, Git, TempIndex, internal};

pub(crate) fn changes(dir: &Path, base: &str) -> Result<Vec<Delta>> {
    let base = git::commit_id(dir, base)?;
    let tree = git::working_tree(dir)?;
    let out = Git::new(dir, "diff-tree")
        .args(["-r", "-z", "-M", "--raw", "--numstat", "--relative"])
        .args([&base, &tree])
        .run()?;
    parse(&out)
}

/// What differs between two commits or trees; the working tree is not read.
pub(crate) fn diff(dir: &Path, from: &str, to: &str) -> Result<Vec<Delta>> {
    let out = Git::new(dir, "diff-tree")
        .args(["-r", "-z", "-M", "--raw", "--numstat", "--relative"])
        .args(["--end-of-options", from, to])
        .run()?;
    parse(&out)
}

/// One diff for any number of files, in a fixed number of git calls: a tree
/// that is `base` with just `paths` (and the old side of their renames)
/// taken from the working tree, diffed against `base`. No path list ever
/// goes on a command line, which Windows limits to 32k characters.
pub(crate) fn patch(dir: &Path, base: &str, paths: &[RelPath]) -> Result<String> {
    if paths.is_empty() {
        return Ok(String::new());
    }
    let (base, picked) = picked(dir, base, paths)?;
    let out = Git::new(dir, "diff-tree")
        .args(["-p", "-M", "--relative"])
        .args([&base, &picked])
        .run()?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// The diff of each of `paths` between two commits or trees, from one
/// diff of everything: its raw records tell which file each part is of.
pub(crate) fn patches(
    dir: &Path,
    from: &str,
    to: &str,
    paths: &[RelPath],
) -> Result<BTreeMap<String, String>> {
    let mut patches: BTreeMap<String, String> = paths
        .iter()
        .map(|p| (p.as_str().to_owned(), String::new()))
        .collect();
    if paths.is_empty() {
        return Ok(patches);
    }
    let out = Git::new(dir, "diff-tree")
        .args(["-r", "-z", "-M", "--raw", "-p", "--relative"])
        .args(["--end-of-options", from, to])
        .run()?;
    // Raw records, an empty field (no path is empty), then the patch text.
    let Some(end) = out.windows(2).position(|w| w == b"\0\0") else {
        return Ok(patches);
    };
    let deltas = records(&mut git::fields(&out[..=end]).peekable())?;
    let text = String::from_utf8_lossy(&out[end + 2..]);
    let mut parts = split_files(&text).into_iter();
    for (letter, delta) in deltas {
        // A file that became a symlink, or back, is a deletion and a
        // creation in the patch.
        let count = if letter == 'T' { 2 } else { 1 };
        let part: String = parts.by_ref().take(count).collect();
        if let Some(patch) = patches.get_mut(&delta.path) {
            patch.push_str(&part);
        }
    }
    Ok(patches)
}

/// Patch text split before each `diff --git` header; hunk lines always
/// start with a space, `+`, `-`, `@` or `\\`, so no header hides in them.
fn split_files(text: &str) -> Vec<&str> {
    let mut starts: Vec<usize> = text
        .match_indices("diff --git ")
        .map(|(at, _)| at)
        .filter(|&at| at == 0 || text.as_bytes()[at - 1] == b'\n')
        .collect();
    starts.push(text.len());
    starts.windows(2).map(|w| &text[w[0]..w[1]]).collect()
}

/// `base` as a commit id, and a tree that is `base` with just `paths` (and
/// the old side of their renames) as they are in the working tree.
fn picked(dir: &Path, base: &str, paths: &[RelPath]) -> Result<(String, String)> {
    let base = git::commit_id(dir, base)?;
    let tree = git::working_tree(dir)?;
    let raw = Git::new(dir, "diff-tree")
        .args(["-r", "-z", "-M", "--raw", "--relative"])
        .args([&base, &tree])
        .run()?;
    let mut wanted: BTreeSet<String> = paths.iter().map(|p| p.as_str().to_owned()).collect();
    for delta in parse(&raw)? {
        if let Some(from) = delta.from.filter(|_| wanted.contains(&delta.path)) {
            wanted.insert(from);
        }
    }
    let prefix = git::dirs(dir)?.prefix;
    let wanted: BTreeSet<String> = wanted.into_iter().map(|p| format!("{prefix}{p}")).collect();

    // `<mode> <type> <id>\t<path>` of the wanted files in the working tree;
    // the others are gone there and get a zero mode, which removes them.
    let listing = Git::new(dir, "ls-tree")
        .args(["-r", "-z", "--full-name", &tree])
        .run()?;
    let mut info = Vec::new();
    let mut found = BTreeSet::new();
    for record in git::fields(&listing) {
        if let Some((_, path)) = record.split_once('\t').filter(|(_, p)| wanted.contains(*p)) {
            found.insert(path.to_owned());
            info.extend_from_slice(record.as_bytes());
            info.push(0);
        }
    }
    let zero = "0".repeat(tree.len());
    for gone in wanted.difference(&found) {
        info.extend_from_slice(format!("0 {zero}\t{gone}").as_bytes());
        info.push(0);
    }
    let index = TempIndex::empty(dir)?;
    Git::new(dir, "read-tree").arg(&base).index(&index).run()?;
    Git::new(dir, "update-index")
        .args(["-z", "--index-info"])
        .index(&index)
        .input(info)
        .run()?;
    let picked = Git::new(dir, "write-tree").index(&index).text()?;
    Ok((base, picked))
}

/// Parses `diff-tree -z --raw [--numstat]`: all raw records, then the line
/// counts in the same order. Paths may hold any byte but NUL, so fields are
/// taken by position, never by their look.
fn parse(out: &[u8]) -> Result<Vec<Delta>> {
    let mut fields = git::fields(out).peekable();
    let mut deltas: Vec<Delta> = records(&mut fields)?.into_iter().map(|(_, d)| d).collect();
    for delta in &mut deltas {
        let Some(counts) = fields.next() else { break };
        let mut parts = counts.splitn(3, '\t');
        // Binary files count as `-`.
        let mut count = || parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        delta.additions = count();
        delta.deletions = count();
        if parts.next() == Some("") {
            // A rename: the old and the new path follow as fields.
            fields.next();
            fields.next();
        }
    }
    Ok(deltas)
}

/// The raw records at the start of `fields`, each with its status letter.
fn records(
    fields: &mut std::iter::Peekable<impl Iterator<Item = String>>,
) -> Result<Vec<(char, Delta)>> {
    let malformed = || internal("git diff-tree printed something unexpected");
    let mut deltas = Vec::new();
    while let Some(raw) = fields.next_if(|f| f.starts_with(':')) {
        let status = raw.rsplit(' ').next().ok_or_else(malformed)?;
        let letter = status.chars().next().ok_or_else(malformed)?;
        let mut path = fields.next().ok_or_else(malformed)?;
        let (kind, from) = match letter {
            'A' => (ChangeKind::Added, None),
            'D' => (ChangeKind::Deleted, None),
            'R' | 'C' => {
                let to = fields.next().ok_or_else(malformed)?;
                (ChangeKind::Renamed, Some(std::mem::replace(&mut path, to)))
            }
            _ => (ChangeKind::Modified, None),
        };
        let delta = Delta {
            path,
            kind,
            from,
            additions: 0,
            deletions: 0,
        };
        deltas.push((letter, delta));
    }
    Ok(deltas)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_raw_and_numstat_records() {
        let out = [
            ":100644 100644 aaa bbb M",
            "a.rs",
            ":000000 100644 000 ccc A",
            ":odd",
            ":100644 100644 ddd ddd R100",
            "old\tname",
            "new",
            ":100644 000000 eee 000 D",
            "bin",
            "3\t1\ta.rs",
            "1\t0\t:odd",
            "0\t0\t",
            "old\tname",
            "new",
            "-\t-\tbin",
            "",
        ]
        .join("\0");
        let deltas = parse(out.as_bytes()).unwrap_or_default();
        let summary: Vec<_> = deltas
            .iter()
            .map(|d| {
                let from = d.from.as_deref();
                (d.path.as_str(), d.kind, from, d.additions, d.deletions)
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("a.rs", ChangeKind::Modified, None, 3, 1),
                (":odd", ChangeKind::Added, None, 1, 0),
                ("new", ChangeKind::Renamed, Some("old\tname"), 0, 0),
                ("bin", ChangeKind::Deleted, None, 0, 0),
            ]
        );
    }
}
