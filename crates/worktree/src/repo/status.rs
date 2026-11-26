//! `git.status`: `git status --porcelain=v2` and the git folder's markers of
//! an operation in progress.

use std::path::Path;

use ovra_core::protected::PLACEHOLDERS;
use ovra_core::protocol::ErrorCode;
use ovra_core::protocol::git::{GitChange, GitOperation, MAX_STATUS_FILES, RepoStatus, StatusFile};
use ovra_core::services::Result;

use crate::git::{self, Dirs, Git};

/// Reads only: `GIT_OPTIONAL_LOCKS=0` keeps it off the index lock that
/// the person's and agents' git need.
pub(super) fn status(root: &Path) -> Result<Option<RepoStatus>> {
    let dirs = match git::dirs(root) {
        Ok(dirs) => dirs,
        Err(e) if e.code == ErrorCode::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut run = Git::new(root, "status")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["--porcelain=v2", "-z", "--branch", "--ahead-behind"])
        .arg("--untracked-files=all");
    // Only the project's folder, when it is not the whole repository.
    if !dirs.prefix.is_empty() {
        run = run.args(["--", "."]);
    }
    let mut status = parse(&run.run()?, &dirs.prefix, |f| !placeholder(root, f));
    status.operation = operation(&dirs);
    if status.branch.is_some() && status.upstream.is_none() {
        status.publish_to = super::publish_remote(root)?;
    }
    Ok(Some(status))
}

/// Whether `file` is only the sandbox's untracked, empty placeholder at the
/// top of the project (`ovra_core::protected::PLACEHOLDERS`): not a change
/// of the person's, and "Commit all" must not commit it.
fn placeholder(root: &Path, file: &StatusFile) -> bool {
    file.staged.is_none()
        && file.unstaged == Some(GitChange::Untracked)
        && PLACEHOLDERS.contains(&file.path.as_str())
        && std::fs::symlink_metadata(root.join(&file.path))
            .is_ok_and(|m| m.is_file() && m.len() == 0)
}

/// What git is in the middle of, from the files it keeps while it is.
fn operation(dirs: &Dirs) -> Option<GitOperation> {
    let has = |name: &str| dirs.git_dir.join(name).exists();
    if has("rebase-merge") || has("rebase-apply") {
        Some(GitOperation::Rebase)
    } else if has("MERGE_HEAD") {
        Some(GitOperation::Merge)
    } else if has("CHERRY_PICK_HEAD") {
        Some(GitOperation::CherryPick)
    } else if has("REVERT_HEAD") {
        Some(GitOperation::Revert)
    } else if has("BISECT_LOG") {
        Some(GitOperation::Bisect)
    } else {
        None
    }
}

fn change(code: u8) -> Option<GitChange> {
    Some(match code {
        b'M' => GitChange::Modified,
        b'T' => GitChange::TypeChanged,
        b'A' => GitChange::Added,
        b'D' => GitChange::Deleted,
        b'R' => GitChange::Renamed,
        b'C' => GitChange::Copied,
        _ => return None,
    })
}

/// Porcelain v2 `-z` output; paths in it are from the top of the
/// repository and become relative to the project at `prefix`.
fn parse(out: &[u8], prefix: &str, keep: impl Fn(&StatusFile) -> bool) -> RepoStatus {
    let mut status = RepoStatus::default();
    let mut fields = out
        .split(|b| *b == 0)
        .filter(|f| !f.is_empty())
        .map(String::from_utf8_lossy);
    while let Some(field) = fields.next() {
        if let Some(header) = field.strip_prefix("# ") {
            branch_header(&mut status, header);
            continue;
        }
        let xy = field.split(' ').nth(1).unwrap_or_default().as_bytes();
        let (staged, unstaged) = (xy.first().copied(), xy.get(1).copied());
        let (path, from, staged, unstaged) = match field.as_bytes().first() {
            Some(b'1') => (
                field.splitn(9, ' ').nth(8),
                None,
                staged.and_then(change),
                unstaged.and_then(change),
            ),
            Some(b'2') => (
                field.splitn(10, ' ').nth(9),
                fields.next(),
                staged.and_then(change),
                unstaged.and_then(change),
            ),
            Some(b'u') => (
                field.splitn(11, ' ').nth(10),
                None,
                None,
                Some(GitChange::Conflicted),
            ),
            Some(b'?') => (field.get(2..), None, None, Some(GitChange::Untracked)),
            _ => continue,
        };
        let Some(path) = path.and_then(|p| p.strip_prefix(prefix)) else {
            continue;
        };
        let file = StatusFile {
            path: path.to_owned(),
            from: from
                .as_deref()
                .and_then(|f| f.strip_prefix(prefix))
                .map(str::to_owned),
            staged,
            unstaged,
        };
        // Left out before counting, so "and N more" counts only real files.
        if !keep(&file) {
            continue;
        }
        if status.files.len() >= MAX_STATUS_FILES as usize {
            status.truncated += 1;
            continue;
        }
        status.files.push(file);
    }
    status
}

fn branch_header(status: &mut RepoStatus, header: &str) {
    let Some((key, value)) = header.split_once(' ') else {
        return;
    };
    match key {
        "branch.oid" => status.head = (value != "(initial)").then(|| value.to_owned()),
        "branch.head" => status.branch = (value != "(detached)").then(|| value.to_owned()),
        "branch.upstream" => status.upstream = Some(value.to_owned()),
        "branch.ab" => {
            let mut counts = value.split(' ');
            let mut count = |sign: char| {
                counts
                    .next()
                    .and_then(|c| c.strip_prefix(sign))
                    .and_then(|c| c.parse().ok())
                    .unwrap_or(0)
            };
            status.ahead = count('+');
            status.behind = count('-');
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_out_files_do_not_count_toward_the_cut() {
        let max = MAX_STATUS_FILES as usize;
        let mut out = Vec::new();
        // A left-out file first, then exactly as many real files as fit.
        out.extend_from_slice(b"? CLAUDE.md\0");
        for n in 0..max {
            out.extend_from_slice(format!("? f{n}\0").as_bytes());
        }
        let status = parse(&out, "", |f| f.path != "CLAUDE.md");
        assert_eq!(status.files.len(), max);
        assert_eq!(status.truncated, 0);
    }
}
