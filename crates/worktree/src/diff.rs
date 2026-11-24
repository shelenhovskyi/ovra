//! `changes` and `patch`: a commit against the working tree, untracked files
//! included, through a tree written from a temporary index.

use std::collections::BTreeSet;
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

/// One diff for any number of files, in a fixed number of git calls: a tree
/// that is `base` with just `paths` (and the old side of their renames)
/// taken from the working tree, diffed against `base`. No path list ever
/// goes on a command line, which Windows limits to 32k characters.
pub(crate) fn patch(dir: &Path, base: &str, paths: &[RelPath]) -> Result<String> {
    if paths.is_empty() {
        return Ok(String::new());
    }
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
    let prefix = Git::new(dir, "rev-parse").arg("--show-prefix").text()?;
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
    let out = Git::new(dir, "diff-tree")
        .args(["-p", "-M", "--relative"])
        .args([&base, &picked])
        .run()?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Parses `diff-tree -z --raw [--numstat]`: all raw records, then the line
/// counts in the same order. Paths may hold any byte but NUL, so fields are
/// taken by position, never by their look.
fn parse(out: &[u8]) -> Result<Vec<Delta>> {
    let malformed = || internal("git diff-tree printed something unexpected");
    let mut fields = git::fields(out).peekable();
    let mut deltas = Vec::new();
    while let Some(raw) = fields.next_if(|f| f.starts_with(':')) {
        let status = raw.rsplit(' ').next().ok_or_else(malformed)?;
        let mut path = fields.next().ok_or_else(malformed)?;
        let (kind, from) = match status.chars().next() {
            Some('A') => (ChangeKind::Added, None),
            Some('D') => (ChangeKind::Deleted, None),
            Some('R' | 'C') => {
                let to = fields.next().ok_or_else(malformed)?;
                (ChangeKind::Renamed, Some(std::mem::replace(&mut path, to)))
            }
            _ => (ChangeKind::Modified, None),
        };
        deltas.push(Delta {
            path,
            kind,
            from,
            additions: 0,
            deletions: 0,
        });
    }
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
