//! A reviewed worker's changes as one commit on another base, for a pull
//! request (`docs/specs/pr-flow.md`, "What is pushed" steps 2 and 4).

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ovra_core::services::{CommitAuthor, Merged, Picked, Result};

use crate::git::{self, Ended, Git, internal};

/// How long signing may take: gpg can wait for a passphrase Ovra cannot give.
const SIGN_LIMIT: Duration = Duration::from_secs(120);

/// How long the merge may take, however large the repository.
const MERGE_LIMIT: Duration = Duration::from_secs(300);

/// Sets its flag when dropped: a pick whose call was dropped (the person
/// closed the dialog, the request was cancelled) stops its git and gpg.
#[derive(Debug)]
pub(crate) struct StopOnDrop(pub(crate) Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Why merge-tree printed no tree. `--merge-base` came with git 2.40: an
/// older one answers with a usage error (exit 129, "unknown option").
fn merge_failed(code: Option<i32>, stderr: &[u8]) -> ovra_core::protocol::ProtocolError {
    let stderr = String::from_utf8_lossy(stderr);
    if code == Some(129) || stderr.contains("unknown option") {
        return ovra_core::protocol::ProtocolError::new(
            ovra_core::protocol::ErrorCode::Unavailable,
            "Pull requests and updates from the project need git 2.40 or later",
        );
    }
    internal(format!("git merge-tree: {}", stderr.trim()))
}

fn stopped(what: &str) -> ovra_core::protocol::ProtocolError {
    internal(format!("git {what} was stopped"))
}

/// `git merge-tree --write-tree` of `ours` and `theirs` with `start` as
/// the merge base, no hooks: the tree, conflict markers in the files listed.
fn merge_tree(
    root: &Path,
    start: &str,
    ours: &str,
    theirs: &str,
    stop: &AtomicBool,
) -> Result<(String, Vec<String>)> {
    let out = Git::without_hooks(root, "merge-tree")
        .args(["--write-tree", "-z", "--name-only", "--no-messages"])
        .arg(format!("--merge-base={start}"))
        .args([ours, theirs])
        .output_until(stop, MERGE_LIMIT)?;
    let out = match out {
        Ended::Exited(out) => out,
        Ended::Stopped(_) => return Err(stopped("merge-tree")),
        Ended::TimedOut(_) => {
            return Err(internal(
                "git merge-tree was still running after 5 minutes and was stopped",
            ));
        }
    };
    // `<tree>NUL<conflicted path>NUL...`; exit 1 means conflicts.
    let mut fields = git::fields(&out.stdout);
    let tree = match out.status.code() {
        Some(0 | 1) => fields.next(),
        _ => None,
    };
    let Some(tree) = tree else {
        return Err(merge_failed(out.status.code(), &out.stderr));
    };
    let files: BTreeSet<String> = fields.filter(|f| !f.is_empty()).collect();
    Ok((tree, files.into_iter().collect()))
}

/// The changes from `base` to `main` merged into those from `base` to
/// `mine`, as a commit on `main` by Ovra (like a snapshot), unsigned.
pub(crate) fn merge(
    root: &Path,
    base: &str,
    mine: &str,
    main: &str,
    stop: &AtomicBool,
) -> Result<Merged> {
    let base = git::commit_id(root, base)?;
    let mine = git::commit_id(root, mine)?;
    let main = git::commit_id(root, main)?;
    let (tree, conflicts) = merge_tree(root, &base, &mine, &main, stop)?;
    let commit = Git::without_hooks(root, "commit-tree")
        .args(["--no-gpg-sign", &tree, "-p", &main, "-m", "Ovra update"])
        .env("GIT_AUTHOR_NAME", "Ovra")
        .env("GIT_AUTHOR_EMAIL", "ovra@localhost")
        .env("GIT_COMMITTER_NAME", "Ovra")
        .env("GIT_COMMITTER_EMAIL", "ovra@localhost")
        .text()?;
    git::known_commit(root, &commit);
    let mut whole = Vec::new();
    for path in &conflicts {
        let blob = Git::new(root, "cat-file")
            .args(["blob", &format!("{commit}:{path}")])
            .try_text()?;
        let marked =
            blob.is_some_and(|text| text.starts_with("<<<<<<< ") || text.contains("\n<<<<<<< "));
        if !marked {
            whole.push(path.clone());
        }
    }
    Ok(Merged {
        commit,
        conflicts,
        whole,
    })
}

/// `git merge-tree --write-tree` with `start` as the merge base, `base` as
/// ours and `reviewed` as theirs; then `commit-tree` on `base`. Neither runs
/// hooks; the commit is signed when `commit.gpgsign` says so.
pub(crate) fn pick(
    root: &Path,
    base: &str,
    start: &str,
    reviewed: &str,
    message: &str,
    author: Option<&CommitAuthor>,
    stop: &AtomicBool,
) -> Result<Picked> {
    let base = git::commit_id(root, base)?;
    let start = git::commit_id(root, start)?;
    let reviewed = git::commit_id(root, reviewed)?;
    let (tree, conflicts) = merge_tree(root, &start, &base, &reviewed, stop)?;
    if !conflicts.is_empty() {
        return Ok(Picked::Conflicts(conflicts));
    }
    let mut commit = Git::without_hooks(root, "commit-tree")
        .args([&tree, "-p", &base, "-F", "-"])
        .input(message);
    // Plumbing ignores `commit.gpgsign`; git signs with the configured key
    // and format (`user.signingkey`, `gpg.format`) when asked.
    let sign = Git::new(root, "config")
        .args(["--type=bool", "--get", "commit.gpgsign"])
        .try_text()?;
    if sign.as_deref() == Some("true") {
        commit = commit.arg("-S");
    }
    if let Some(author) = author {
        for who in ["AUTHOR", "COMMITTER"] {
            commit = commit
                .env(&format!("GIT_{who}_NAME"), &author.name)
                .env(&format!("GIT_{who}_EMAIL"), &author.email);
        }
    }
    let out = match commit.output_until(stop, SIGN_LIMIT)? {
        Ended::Exited(out) => out,
        Ended::Stopped(_) => return Err(stopped("commit-tree")),
        Ended::TimedOut(_) => {
            return Err(internal(
                "git commit-tree was still running after 2 minutes and was stopped: \
                 gpg may be waiting for a passphrase",
            ));
        }
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(internal(format!("git commit-tree: {}", stderr.trim())));
    }
    let id = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    git::known_commit(root, &id);
    Ok(Picked::Commit(id))
}

#[cfg(test)]
mod tests {
    use ovra_core::services::{CommitAuthor, Picked, Worktrees};

    use crate::GitWorktrees;
    use ovra_core::path::RelPath;

    use crate::tests::{Repo, git, put, read};

    #[tokio::test]
    async fn a_pick_keeps_the_workers_changes_and_drops_the_persons() {
        let repo = Repo::new(&[("a.txt", "a\n"), ("b.txt", "b\n"), ("c.txt", "c\n")]);
        let root = &repo.root;
        let w = GitWorktrees::new();
        // The person's uncommitted work is in the worker's start snapshot.
        put(root, "a.txt", "mine\n");
        put(root, "mine.txt", "mine\n");
        let (start, wt) = repo.worker(&w).await;
        put(&wt, "b.txt", "worker\n");
        put(&wt, "new.txt", "new\n");
        let reviewed = w
            .snapshot(wt.clone())
            .await
            .unwrap_or_else(|e| panic!("{e:?}"));
        // The remote's base moved on meanwhile.
        put(root, "c.txt", "upstream\n");
        git(
            root,
            &["commit", "--quiet", "-m", "upstream", "--", "c.txt"],
        );
        let base = git(root, &["rev-parse", "HEAD"]);

        let message = "Add new\n\nThe body.".to_owned();
        let picked = w.pick(
            root.clone(),
            base.clone(),
            start.clone(),
            reviewed.clone(),
            message,
            None,
        );
        let Ok(Picked::Commit(id)) = picked.await else {
            panic!("no commit");
        };
        let show = |what: &str| git(root, &["show", &format!("{id}:{what}")]);
        assert_eq!(
            (show("a.txt"), show("b.txt"), show("c.txt"), show("new.txt")),
            ("a".into(), "worker".into(), "upstream".into(), "new".into())
        );
        let files = git(root, &["ls-tree", "--name-only", &id]);
        assert!(!files.contains("mine.txt"), "{files}");
        let head = git(root, &["log", "-1", "--format=%P|%an <%ae>|%cn|%B", &id]);
        assert_eq!(
            head,
            format!("{base}|Test <test@example.com>|Test|Add new\n\nThe body.")
        );
        // Nothing in the checkout moved.
        assert_eq!(git(root, &["rev-parse", "HEAD"]), base);

        let author = CommitAuthor {
            name: "Ann".into(),
            email: "ann@example.com".into(),
        };
        let picked = w.pick(
            root.clone(),
            base,
            start,
            reviewed,
            "x".into(),
            Some(author),
        );
        let Ok(Picked::Commit(id)) = picked.await else {
            panic!("no commit");
        };
        let who = git(root, &["log", "-1", "--format=%an <%ae>|%cn <%ce>", &id]);
        assert_eq!(who, "Ann <ann@example.com>|Ann <ann@example.com>");
    }

    #[tokio::test]
    async fn a_merge_brings_the_project_in_and_marks_what_both_changed() {
        let lines = "1\n2\n3\n4\n5\n6\n7\n";
        let repo = Repo::new(&[("a.txt", lines), ("b.txt", "b\n")]);
        let root = &repo.root;
        let w = GitWorktrees::new();
        let (base, wt) = repo.worker(&w).await;
        put(&wt, "a.txt", "worker\n2\n3\n4\n5\n6\n7\n");
        put(&wt, "b.txt", "worker\n");
        let mine = w
            .snapshot(wt.clone())
            .await
            .unwrap_or_else(|e| panic!("{e:?}"));
        // The person changes other lines, b.txt too, and adds a file.
        put(root, "a.txt", "1\n2\n3\n4\n5\n6\nmine\n");
        put(root, "b.txt", "mine\n");
        put(root, "c.txt", "c\n");
        let main = w
            .snapshot(root.clone())
            .await
            .unwrap_or_else(|e| panic!("{e:?}"));

        let mine_again = mine.clone();
        let merged = w.merge(root.clone(), base, mine, main.clone()).await;
        let merged = merged.unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(merged.conflicts, ["b.txt"]);
        assert!(merged.whole.is_empty());
        let id = &merged.commit;
        let show = |what: &str| git(root, &["show", &format!("{id}:{what}")]);
        assert_eq!(show("a.txt"), "worker\n2\n3\n4\n5\n6\nmine");
        assert_eq!(show("c.txt"), "c");
        let b = show("b.txt");
        assert!(
            b.starts_with("<<<<<<<") && b.contains("worker") && b.contains("mine"),
            "{b}"
        );
        assert_eq!(
            git(root, &["log", "-1", "--format=%P|%an", id]),
            format!("{main}|Ovra")
        );

        // The core writes it into the worktree with `apply`.
        let paths = ["a.txt", "b.txt", "c.txt"]
            .map(|p| RelPath::parse(p).unwrap_or_else(|e| panic!("{e:?}")));
        let skipped = w
            .apply(id.clone(), mine_again, wt.clone(), paths.to_vec())
            .await;
        assert_eq!(
            skipped.unwrap_or_else(|e| panic!("{e:?}")),
            Vec::<String>::new()
        );
        assert_eq!(read(&wt, "c.txt").as_deref(), Some("c\n"));
        assert_eq!(
            read(&wt, "a.txt").as_deref(),
            Some("worker\n2\n3\n4\n5\n6\nmine\n")
        );
    }

    #[tokio::test]
    async fn a_merge_names_files_it_could_not_mark() {
        let repo = Repo::new(&[("a.txt", "a\n")]);
        let root = &repo.root;
        let w = GitWorktrees::new();
        let (base, wt) = repo.worker(&w).await;
        put(&wt, "a.txt", "worker\n");
        let mine = w
            .snapshot(wt.clone())
            .await
            .unwrap_or_else(|e| panic!("{e:?}"));
        std::fs::remove_file(root.join("a.txt")).unwrap_or_else(|e| panic!("{e}"));
        let main = w
            .snapshot(root.clone())
            .await
            .unwrap_or_else(|e| panic!("{e:?}"));
        let merged = w.merge(root.clone(), base, mine, main).await;
        let merged = merged.unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            (merged.conflicts, merged.whole),
            (vec!["a.txt".to_owned()], vec!["a.txt".to_owned()])
        );
    }

    /// Signed through git, with the person's `gpg.program` (a fake here).
    #[cfg(unix)]
    #[tokio::test]
    async fn a_pick_is_signed_when_the_config_says_so() {
        use std::os::unix::fs::PermissionsExt as _;
        let repo = Repo::new(&[("a.txt", "a\n")]);
        let root = &repo.root;
        let w = GitWorktrees::new();
        let (start, wt) = repo.worker(&w).await;
        put(&wt, "a.txt", "worker\n");
        let reviewed = w
            .snapshot(wt.clone())
            .await
            .unwrap_or_else(|e| panic!("{e:?}"));
        let gpg = repo.tmp.path().join("fake-gpg");
        let script = "#!/bin/sh\ncat >/dev/null\necho '[GNUPG:] SIG_CREATED ' >&2\n\
                      printf -- '-----BEGIN PGP SIGNATURE-----\\nfake\\n-----END PGP SIGNATURE-----\\n'\n";
        std::fs::write(&gpg, script).unwrap_or_else(|e| panic!("{e}"));
        std::fs::set_permissions(&gpg, std::fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| panic!("{e}"));
        git(root, &["config", "gpg.program", &gpg.to_string_lossy()]);
        git(root, &["config", "gpg.format", "openpgp"]);
        git(root, &["config", "user.signingkey", "test"]);
        git(root, &["config", "commit.gpgsign", "true"]);

        let base = git(root, &["rev-parse", "HEAD"]);
        let picked = w.pick(root.clone(), base, start, reviewed, "x".into(), None);
        let Ok(Picked::Commit(id)) = picked.await else {
            panic!("no commit");
        };
        let raw = git(root, &["cat-file", "commit", &id]);
        assert!(
            raw.contains("gpgsig -----BEGIN PGP SIGNATURE-----"),
            "{raw}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_pick_runs_no_hook_of_the_repository() {
        use std::os::unix::fs::PermissionsExt as _;
        let repo = Repo::new(&[("a.txt", "a\n")]);
        let root = &repo.root;
        let w = GitWorktrees::new();
        let (start, wt) = repo.worker(&w).await;
        put(&wt, "a.txt", "worker\n");
        let reviewed = w
            .snapshot(wt.clone())
            .await
            .unwrap_or_else(|e| panic!("{e:?}"));
        let ran = repo.tmp.path().join("ran");
        // `core.hooksPath` of the test repositories.
        for hook in ["reference-transaction", "pre-commit", "post-commit"] {
            let path = format!("no-hooks/{hook}");
            put(
                root,
                &path,
                &format!("#!/bin/sh\necho {hook} >> '{}'\n", ran.display()),
            );
            std::fs::set_permissions(root.join(&path), std::fs::Permissions::from_mode(0o755))
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let base = git(root, &["rev-parse", "HEAD"]);
        let picked = w.pick(root.clone(), base, start, reviewed, "x".into(), None);
        assert!(matches!(picked.await, Ok(Picked::Commit(_))));
        assert!(!ran.exists(), "{:?}", std::fs::read_to_string(&ran));
    }

    /// Whether process `pid` still runs (a zombie does not).
    #[cfg(target_os = "linux")]
    fn runs(pid: &str) -> bool {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
        stat.is_ok_and(|s| s.rsplit(')').next().is_some_and(|r| !r.starts_with(" Z")))
    }

    /// Dropping the call ends its git and the gpg git started.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_dropped_pick_stops_its_git_and_gpg() {
        use std::os::unix::fs::PermissionsExt as _;
        let repo = Repo::new(&[("a.txt", "a\n")]);
        let root = &repo.root;
        let w = GitWorktrees::new();
        let (start, wt) = repo.worker(&w).await;
        put(&wt, "a.txt", "worker\n");
        let reviewed = w
            .snapshot(wt.clone())
            .await
            .unwrap_or_else(|e| panic!("{e:?}"));
        let pid = repo.tmp.path().join("gpg.pid");
        // gpg waiting for a passphrase nobody gives.
        let gpg = repo.tmp.path().join("fake-gpg");
        let script = format!(
            "#!/bin/sh\necho $$ > '{}.tmp'\nmv '{0}.tmp' '{0}'\nexec sleep 30\n",
            pid.display()
        );
        std::fs::write(&gpg, script).unwrap_or_else(|e| panic!("{e}"));
        std::fs::set_permissions(&gpg, std::fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| panic!("{e}"));
        git(root, &["config", "gpg.program", &gpg.to_string_lossy()]);
        git(root, &["config", "gpg.format", "openpgp"]);
        git(root, &["config", "user.signingkey", "test"]);
        git(root, &["config", "commit.gpgsign", "true"]);
        let base = git(root, &["rev-parse", "HEAD"]);

        let mut picked = w.pick(root.clone(), base, start, reviewed, "x".into(), None);
        let waiting = std::time::Instant::now();
        let gpg_pid = loop {
            if let Ok(pid) = std::fs::read_to_string(&pid) {
                break pid.trim().to_owned();
            }
            assert!(waiting.elapsed().as_secs() < 2, "gpg never started");
            let poll = futures_poll(&mut picked);
            assert!(poll.is_none(), "the pick ended: {poll:?}");
            tokio::task::yield_now().await;
        };
        assert!(runs(&gpg_pid));
        drop(picked);
        while runs(&gpg_pid) {
            assert!(waiting.elapsed().as_secs() < 2, "gpg still runs");
            tokio::task::yield_now().await;
        }
    }

    /// Polls `future` once without waiting.
    #[cfg(target_os = "linux")]
    fn futures_poll<T>(future: &mut ovra_core::services::BoxFuture<'_, T>) -> Option<T> {
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match future.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(v) => Some(v),
            std::task::Poll::Pending => None,
        }
    }

    #[test]
    fn an_old_git_is_named_as_too_old() {
        let old = b"error: unknown option `merge-base=abc'\nusage: git merge-tree";
        for (code, stderr) in [(Some(129), &b""[..]), (Some(128), &old[..])] {
            let e = super::merge_failed(code, stderr);
            assert_eq!(
                e.message,
                "Pull requests and updates from the project need git 2.40 or later"
            );
        }
        let other = super::merge_failed(Some(128), b"fatal: bad object x\n");
        assert_eq!(other.message, "git merge-tree: fatal: bad object x");
    }

    #[tokio::test]
    async fn a_pick_names_the_files_both_sides_changed() {
        let repo = Repo::new(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
        let root = &repo.root;
        let w = GitWorktrees::new();
        let (start, wt) = repo.worker(&w).await;
        put(&wt, "a.txt", "worker\n");
        put(&wt, "b.txt", "worker\n");
        let reviewed = w
            .snapshot(wt.clone())
            .await
            .unwrap_or_else(|e| panic!("{e:?}"));
        put(root, "b.txt", "upstream\n");
        git(root, &["commit", "--quiet", "-am", "upstream"]);
        let base = git(root, &["rev-parse", "HEAD"]);

        let picked = w.pick(
            root.clone(),
            base.clone(),
            start,
            reviewed,
            "x".into(),
            None,
        );
        assert_eq!(
            picked.await.ok(),
            Some(Picked::Conflicts(vec!["b.txt".into()]))
        );
        assert_eq!(git(root, &["rev-parse", "HEAD"]), base);
    }
}
