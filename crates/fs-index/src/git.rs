//! Git status through the `git` CLI (`git status --porcelain=v2 -z`).
//! Folders that are not in a repository, or a missing `git`, have no status.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ovra_core::path::RelPath;
use ovra_core::protocol::fs::GitStatus;

/// The repository a project folder belongs to.
#[derive(Debug)]
pub(crate) struct Repo {
    /// Canonical project root; `git` runs there.
    workdir: PathBuf,
    /// The project root relative to the repository root, `""` or ending in `/`.
    prefix: String,
    /// Holds `index` and `HEAD`, also for linked worktrees.
    pub(crate) git_dir: PathBuf,
}

fn git(dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .arg("--no-optional-locks")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

impl Repo {
    /// Finds the repository for a canonical project root, if any.
    pub(crate) fn discover(root: &Path) -> Option<Self> {
        let out = git(root, &["rev-parse", "--show-prefix", "--absolute-git-dir"])?;
        let out = String::from_utf8(out).ok()?;
        let mut lines = out.lines();
        let prefix = lines.next()?.to_owned();
        let git_dir = PathBuf::from(lines.next()?);
        Some(Self {
            workdir: root.to_path_buf(),
            prefix,
            git_dir,
        })
    }

    /// Current status of the project's files, or `None` if `git` failed.
    pub(crate) fn status(&self) -> Option<Statuses> {
        let out = git(
            &self.workdir,
            &[
                "status",
                "--porcelain=v2",
                "-z",
                "--untracked-files=normal",
                "--",
                ".",
            ],
        )?;
        Some(Statuses::parse(&out, &self.prefix))
    }
}

/// Statuses keyed by project-relative path. Untracked directories (which git
/// reports without listing their contents) end with `/`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Statuses {
    /// Status and, for renames inside the project, the old path.
    paths: BTreeMap<String, (GitStatus, Option<String>)>,
    /// The project folder itself lies inside an untracked directory.
    all_untracked: bool,
}

/// Higher wins when a directory holds files with different statuses.
fn rank(status: GitStatus) -> u8 {
    match status {
        GitStatus::Untracked => 1,
        GitStatus::Added => 2,
        GitStatus::Renamed => 3,
        GitStatus::Deleted => 4,
        GitStatus::Modified => 5,
        GitStatus::Conflicted => 6,
    }
}

fn from_xy(xy: &str) -> GitStatus {
    let mut chars = xy.chars();
    let (x, y) = (chars.next(), chars.next());
    if x == Some('D') || y == Some('D') {
        GitStatus::Deleted
    } else if matches!(x, Some('R' | 'C')) {
        GitStatus::Renamed
    } else if x == Some('A') {
        GitStatus::Added
    } else {
        GitStatus::Modified
    }
}

impl Statuses {
    /// Parses `git status --porcelain=v2 -z` output. Paths in it are relative
    /// to the repository root; `prefix` is the project's place in it.
    pub(crate) fn parse(out: &[u8], prefix: &str) -> Self {
        let mut statuses = Self::default();
        let mut fields = out
            .split(|b| *b == 0)
            .filter_map(|f| std::str::from_utf8(f).ok());
        while let Some(record) = fields.next() {
            let mut from = None;
            let parsed = match record.split_once(' ') {
                Some(("1", rest)) => rest.splitn(8, ' ').nth(7).map(|p| (p, from_xy(rest))),
                Some(("2", rest)) => {
                    // With -z the original path follows as its own field.
                    from = fields.next().and_then(|f| f.strip_prefix(prefix));
                    rest.splitn(9, ' ').nth(8).map(|p| (p, from_xy(rest)))
                }
                Some(("u", rest)) => rest
                    .splitn(10, ' ')
                    .nth(9)
                    .map(|p| (p, GitStatus::Conflicted)),
                Some(("?", path)) => Some((path, GitStatus::Untracked)),
                _ => None,
            };
            let Some((path, status)) = parsed else {
                continue;
            };
            if path.ends_with('/') && prefix.starts_with(path) {
                statuses.all_untracked = true;
            } else if let Some(path) = path.strip_prefix(prefix) {
                let from = from
                    .filter(|_| status == GitStatus::Renamed)
                    .map(str::to_owned);
                statuses.paths.insert(path.to_owned(), (status, from));
            }
        }
        statuses
    }

    /// Status of each direct child of `dir` that has one, by name. The second
    /// value is true when `dir` itself lies inside an untracked directory.
    pub(crate) fn children(&self, dir: &RelPath) -> (HashMap<String, Child>, bool) {
        let prefix = if dir.is_root() {
            String::new()
        } else {
            format!("{dir}/")
        };
        if self.all_untracked {
            return (HashMap::new(), true);
        }
        let mut children: HashMap<String, Child> = HashMap::new();
        for (path, (status, from)) in &self.paths {
            let status = *status;
            if path.ends_with('/') && prefix.starts_with(path.as_str()) {
                return (HashMap::new(), true);
            }
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            let (name, below) = match rest.split_once('/') {
                Some((name, below)) => (name, !below.is_empty()),
                None => (rest, false),
            };
            if name.is_empty() {
                continue;
            }
            let child = children.entry(name.to_owned()).or_insert(Child {
                status,
                renamed_from: None,
                dir: false,
                deleted: false,
            });
            if rank(status) > rank(child.status) {
                child.status = status;
            }
            child.dir |= below || rest.ends_with('/');
            child.deleted |= status == GitStatus::Deleted;
            if !below {
                child.renamed_from.clone_from(from);
            }
        }
        (children, false)
    }
}

/// What git says about one entry of a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Child {
    /// The strongest status of the entry and, for a directory, its contents.
    pub(crate) status: GitStatus,
    pub(crate) renamed_from: Option<String>,
    pub(crate) dir: bool,
    /// The entry, or something inside it, is a tracked file deleted from disk.
    pub(crate) deleted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: &[u8] = b"1 .M N... 100644 100644 100644 aaa aaa sub/a.txt\0\
1 A. N... 000000 100644 100644 000 bbb sub/new file.txt\0\
1 D. N... 100644 000000 000000 ccc 000 gone/x\0\
2 R. N... 100644 100644 100644 ddd ddd R100 sub/to.txt\0sub/from.txt\0\
u UU N... 100644 100644 100644 100644 e1 e2 e3 merge.txt\0\
? notes.md\0\
? sub/fresh/\0";

    #[test]
    fn parses_porcelain_v2() {
        let statuses = Statuses::parse(OUT, "");
        insta::assert_debug_snapshot!(statuses);
    }

    #[test]
    fn strips_the_project_prefix() {
        let statuses = Statuses::parse(OUT, "sub/");
        let keys: Vec<_> = statuses.paths.keys().map(String::as_str).collect();
        assert_eq!(keys, ["a.txt", "fresh/", "new file.txt", "to.txt"]);
        let to = &statuses.paths["to.txt"];
        assert_eq!(to, &(GitStatus::Renamed, Some("from.txt".to_owned())));
    }

    #[test]
    fn folds_children_by_strongest_status() {
        let statuses = Statuses::parse(OUT, "");
        let (root, inside_untracked) = statuses.children(&RelPath::root());
        assert!(!inside_untracked);
        let mut root: Vec<_> = root.into_iter().collect();
        root.sort_by(|a, b| a.0.cmp(&b.0));
        let (sub, _) = statuses.children(&RelPath::parse("sub").expect("valid path"));
        assert_eq!(sub["to.txt"].renamed_from.as_deref(), Some("sub/from.txt"));
        insta::assert_debug_snapshot!(root);

        let fresh = RelPath::parse("sub/fresh/deeper").expect("valid path");
        assert_eq!(statuses.children(&fresh), (HashMap::new(), true));
    }

    #[test]
    fn project_inside_an_untracked_dir() {
        let statuses = Statuses::parse(b"? new/\0", "new/app/");
        assert_eq!(statuses.children(&RelPath::root()), (HashMap::new(), true));
    }
}
