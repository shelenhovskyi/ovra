//! A reviewed worker's changes as one commit on another base, for a pull
//! request (`docs/specs/pr-flow.md`, "What is pushed" steps 2 and 4).

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use ovra_core::services::{CommitAuthor, Picked, Result};

use crate::git::{self, Ended, Git, internal};

/// How long signing may take: gpg can wait for a passphrase Ovra cannot give.
const SIGN_LIMIT: Duration = Duration::from_secs(120);

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
) -> Result<Picked> {
    let base = git::commit_id(root, base)?;
    let start = git::commit_id(root, start)?;
    let reviewed = git::commit_id(root, reviewed)?;
    let out = Git::new(root, "merge-tree")
        .args(["--write-tree", "-z", "--name-only", "--no-messages"])
        .arg(format!("--merge-base={start}"))
        .args([&base, &reviewed])
        .output()?;
    // `<tree>NUL<conflicted path>NUL...`; exit 1 means conflicts.
    let mut fields = git::fields(&out.stdout);
    let tree = match out.status.code() {
        Some(0) => fields.next(),
        Some(1) => {
            let files: BTreeSet<String> = fields.skip(1).collect();
            return Ok(Picked::Conflicts(files.into_iter().collect()));
        }
        _ => None,
    };
    let Some(tree) = tree else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(internal(format!("git merge-tree: {}", stderr.trim())));
    };
    let mut commit = Git::new(root, "commit-tree")
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
    let out = match commit.output_until(&AtomicBool::new(false), SIGN_LIMIT)? {
        Ended::Exited(out) => out,
        Ended::Stopped(_) | Ended::TimedOut(_) => {
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
    use crate::tests::{Repo, git, put};

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
