//! `changes` and `patch`: a commit against the working tree, untracked files
//! included, through a tree written from a temporary index.

use std::path::Path;

use ovra_core::path::RelPath;
use ovra_core::protocol::review::ChangeKind;
use ovra_core::services::{Delta, Result};

use crate::git::{self, Git, internal};

pub(crate) fn changes(dir: &Path, base: &str) -> Result<Vec<Delta>> {
    let base = git::commit_id(dir, base)?;
    let tree = git::working_tree(dir)?;
    let out = Git::new(dir, "diff-tree")
        .args(["-r", "-z", "-M", "--raw", "--numstat", "--relative"])
        .args([&base, &tree])
        .run()?;
    parse(&out)
}

pub(crate) fn patch(dir: &Path, base: &str, path: &RelPath) -> Result<String> {
    let base = git::commit_id(dir, base)?;
    let tree = git::working_tree(dir)?;
    // Renames are only found among the paths given, so name the old one too.
    let renamed = Git::new(dir, "diff-tree")
        .args(["-r", "-z", "-M", "--raw", "--relative"])
        .args([&base, &tree])
        .run()?;
    let from = parse(&renamed)?
        .into_iter()
        .find(|d| d.path == path.as_str())
        .and_then(|d| d.from);
    let out = Git::new(dir, "diff-tree")
        .args(["-p", "-M", "--relative"])
        .args([&base, &tree])
        .arg("--")
        .args(from.as_deref())
        .arg(path.as_str())
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
