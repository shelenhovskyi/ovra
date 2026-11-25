//! `git.blame`: `git blame --porcelain` of the working-tree file.

use std::fs;
use std::path::Path;

use ovra_core::path::RelPath;
use ovra_core::protocol::git::{Blame, BlameRange, CommitInfo, MAX_GIT_FILE};
use ovra_core::services::Result;

use super::{bad, printed};
use crate::files::{self, OnDisk};
use crate::git::{self, Git, internal};

pub(super) fn blame(root: &Path, path: &RelPath) -> Result<Blame> {
    git::dirs(root)?;
    let meta = fs::symlink_metadata(path.under(root));
    if meta.as_ref().is_ok_and(|m| m.len() > MAX_GIT_FILE) {
        return Err(bad(format!(
            "{path} is larger than {} MiB; blame is not shown for it",
            MAX_GIT_FILE / 1024 / 1024
        )));
    }
    let out = Git::new(root, "blame")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["--porcelain", "--no-ext-diff", "--no-textconv", "--"])
        .arg(path.as_str())
        .output()?;
    if out.status.success() {
        return Ok(parse(&String::from_utf8_lossy(&out.stdout)));
    }
    // Not in HEAD (new, or no commit yet): every line is uncommitted. A
    // symlink, or a file under a symlinked folder, is not read.
    if meta.is_ok_and(|m| m.is_file()) && files::on_disk(root, path)? == OnDisk::File {
        let text = fs::read(path.under(root)).map_err(|e| internal(format!("{path}: {e}")))?;
        let lines = text.split(|b| *b == b'\n').count() - usize::from(text.ends_with(b"\n"));
        let ranges = (!text.is_empty()).then(|| BlameRange {
            start: 1,
            lines: u32::try_from(lines).unwrap_or(u32::MAX),
            commit: None,
        });
        return Ok(Blame {
            ranges: ranges.into_iter().collect(),
            commits: Default::default(),
        });
    }
    Err(internal(format!("git blame: {}", printed(&out))))
}

fn is_id(word: &str) -> bool {
    matches!(word.len(), 40 | 64) && word.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Porcelain blame: a `<id> <old line> <new line>[ <count>]` header per
/// line, the commit's details the first time it appears, then the line
/// itself after a tab.
fn parse(out: &str) -> Blame {
    let mut blame = Blame::default();
    // The commit and final line of the line being read; the details of a
    // commit not yet in `commits`.
    let mut current: Option<(String, u32)> = None;
    let mut details: Option<CommitInfo> = None;
    for line in out.lines() {
        if line.starts_with('\t') {
            let Some((id, at)) = current.take() else {
                continue;
            };
            if let Some(info) = details.take() {
                blame.commits.insert(id.clone(), info);
            }
            let commit = (!id.bytes().all(|b| b == b'0')).then_some(id);
            match blame.ranges.last_mut() {
                Some(last) if last.commit == commit && last.start + last.lines == at => {
                    last.lines += 1;
                }
                _ => blame.ranges.push(BlameRange {
                    start: at,
                    lines: 1,
                    commit,
                }),
            }
            continue;
        }
        let mut words = line.split(' ');
        let first = words.next().unwrap_or_default();
        if is_id(first) {
            let at = words.nth(1).and_then(|n| n.parse().ok()).unwrap_or(0);
            if !blame.commits.contains_key(first) && !first.bytes().all(|b| b == b'0') {
                details.get_or_insert_with(|| CommitInfo {
                    id: first.to_owned(),
                    author: String::new(),
                    time: 0,
                    summary: String::new(),
                });
            }
            current = Some((first.to_owned(), at));
            continue;
        }
        let Some(info) = details.as_mut() else {
            continue;
        };
        let value = line.split_once(' ').map(|(_, v)| v).unwrap_or_default();
        match first {
            "author" => info.author = value.to_owned(),
            "author-time" => info.time = value.parse().unwrap_or(0),
            "summary" => info.summary = value.to_owned(),
            _ => {}
        }
    }
    blame
}
