//! Pushes and pull requests with the person's installed `gh` and `git`
//! (`docs/specs/pr-flow.md`, ADR 0010): the only place Ovra starts
//! processes that use the person's GitHub login. It never reads, copies or
//! passes on their credentials (no `gh` config file, keychain or token
//! variable is read), never calls GitHub's API itself and never passes
//! `--show-token`. Remotes are passed by name; URLs lose their user part
//! before they reach an error. Security code: changes need a separate
//! human review.
//! Contract: `ovra_core::services::Forge`; fake:
//! `ovra_core::fakes::MemoryForge`.

mod parse;
mod run;
#[cfg(all(test, unix))]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ovra_core::protocol::pr::{GhStatus, PrDraft, PrInfo, PrStatus};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, Forge, Result};

use run::{GH_TIMEOUT, GIT_TIMEOUT, Programs, failed, run};

/// Given to the git calls that update refs: no hook of the repository runs
/// (`reference-transaction` runs on every ref update, `pre-push` is also
/// skipped by `--no-verify`). On Windows `NUL` is a device in every
/// folder, so no hook can be found below it either.
const NO_HOOKS: [&str; 2] = [
    "-c",
    if cfg!(windows) {
        "core.hooksPath=NUL"
    } else {
        "core.hooksPath=/dev/null"
    },
];

/// Numbers this process's fetches, for their private refs.
static FETCHES: AtomicU64 = AtomicU64::new(0);

/// The forge through the `gh` and `git` on the person's `PATH`.
#[derive(Debug, Default)]
pub struct GhForge {
    /// Fixed programs for tests; `None` looks them up on every call.
    fixed: Option<Programs>,
}

impl GhForge {
    pub fn new() -> Self {
        Self { fixed: None }
    }

    #[cfg(test)]
    fn with(programs: Programs) -> Self {
        Self {
            fixed: Some(programs),
        }
    }

    fn programs(&self) -> Programs {
        self.fixed.clone().unwrap_or_else(Programs::find)
    }

    fn gh(&self) -> Result<PathBuf> {
        self.programs().gh.ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::Unavailable,
                "Install GitHub CLI (gh) and run `gh auth login` in a terminal",
            )
        })
    }

    fn git(&self) -> Result<PathBuf> {
        let git = self.programs().git;
        git.ok_or_else(|| ProtocolError::new(ErrorCode::Unavailable, "git was not found"))
    }

    async fn gh_status(&self, root: &Path, remote: &str) -> Result<GhStatus> {
        parse::remote(remote)?;
        let programs = self.programs();
        let mut host = None;
        if let Some(git) = &programs.git {
            // The URL may hold a token: only its host is kept.
            host = remote_url(git, root, remote)
                .await?
                .as_deref()
                .and_then(parse::host);
        }
        let Some(gh) = &programs.gh else {
            return Ok(GhStatus {
                host,
                ..GhStatus::default()
            });
        };
        let version = run(gh, root, "gh --version", &["--version"], None, GH_TIMEOUT).await?;
        let mut status = GhStatus {
            installed: true,
            version: parse::gh_version(&version.stdout),
            ..GhStatus::default()
        };
        if let Some(host) = &host {
            let args = [
                "auth",
                "status",
                "--active",
                "--hostname",
                host,
                "--json",
                "hosts",
            ];
            let auth = run(gh, root, "gh auth status", &args, None, GH_TIMEOUT).await?;
            (status.login, status.signed_in) = parse::gh_auth(&auth.stdout, host);
        }
        status.host = host;
        Ok(status)
    }

    async fn fetch_branch(&self, root: &Path, remote: &str, branch: &str) -> Result<String> {
        parse::remote(remote)?;
        parse::branch(branch)?;
        let git = self.git()?;
        // A ref of this call's own: `FETCH_HEAD` is rewritten by any other
        // fetch in the repository (the person's, an editor's) meanwhile.
        let fetches = FETCHES.fetch_add(1, Ordering::Relaxed);
        let private = format!("refs/ovra/fetch/{}-{fetches}", std::process::id());
        let fetched = fetch_into(&git, root, remote, branch, &private).await;
        // Deleted however the fetch ended: a failed one may have written it.
        let args = [NO_HOOKS[0], NO_HOOKS[1], "update-ref", "-d", &private];
        let deleted = run(&git, root, "git update-ref", &args, None, GH_TIMEOUT).await?;
        let head = fetched?;
        if !deleted.success {
            return Err(failed("git update-ref", &deleted));
        }
        Ok(head)
    }

    async fn push_commit(
        &self,
        root: &Path,
        remote: &str,
        commit: &str,
        branch: &str,
        expect: Option<&str>,
    ) -> Result<()> {
        parse::remote(remote)?;
        parse::object_id(commit)?;
        parse::branch(branch)?;
        let git = self.git()?;
        if let Some(old) = expect {
            parse::object_id(old)?;
            // With the lease below, a push that is not a fast-forward
            // would overwrite: refuse it before anything is sent.
            let args = ["merge-base", "--is-ancestor", old, commit];
            let ancestor = run(&git, root, "git merge-base", &args, None, GH_TIMEOUT).await?;
            match ancestor.code {
                Some(0) => {}
                Some(1) => {
                    return Err(ProtocolError::new(
                        ErrorCode::BadRequest,
                        "the commit does not build on the branch's last pushed commit",
                    ));
                }
                _ => return Err(failed("git merge-base", &ancestor)),
            }
        }
        // `refs/heads/<branch>:` with nothing after it: the branch must not
        // exist yet.
        let lease = format!(
            "--force-with-lease=refs/heads/{branch}:{}",
            expect.unwrap_or("")
        );
        let refspec = format!("{commit}:refs/heads/{branch}");
        let args = [
            NO_HOOKS[0],
            NO_HOOKS[1],
            "push",
            "--no-verify",
            "--porcelain",
            &lease,
            remote,
            &refspec,
        ];
        let pushed = run(&git, root, "git push", &args, None, GIT_TIMEOUT).await?;
        if pushed.success {
            return Ok(());
        }
        let rejected = pushed
            .stdout
            .lines()
            .any(|line| line.starts_with('!') && line.contains("[rejected]"));
        if rejected {
            return Err(ProtocolError::new(
                ErrorCode::Conflict,
                "the remote branch changed since Ovra last pushed to it",
            ));
        }
        Err(failed("git push", &pushed))
    }

    async fn create_pr(&self, root: &Path, draft: &PrDraft) -> Result<PrInfo> {
        parse::remote(&draft.remote)?;
        parse::branch(&draft.base)?;
        parse::branch(&draft.branch)?;
        let gh = self.gh()?;
        let repo = self.repo(root, &draft.remote).await?;
        let title = format!("--title={}", draft.title);
        let mut args = vec![
            "pr",
            "create",
            "--repo",
            &repo,
            "--base",
            &draft.base,
            "--head",
            &draft.branch,
        ];
        args.extend([title.as_str(), "--body-file", "-"]);
        if draft.draft {
            args.push("--draft");
        }
        let made = run(
            &gh,
            root,
            "gh pr create",
            &args,
            Some(&draft.body),
            GH_TIMEOUT,
        )
        .await?;
        if !made.success {
            return Err(failed("gh pr create", &made));
        }
        parse::created(&made.stdout, repo)
    }

    /// The repository `remote`'s URL names, `HOST/OWNER/REPO`. Named on
    /// every gh call: without it gh may pick another remote, such as
    /// `upstream`.
    async fn repo(&self, root: &Path, remote: &str) -> Result<String> {
        let url = remote_url(&self.git()?, root, remote).await?;
        url.as_deref().and_then(parse::repo).ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::BadRequest,
                "the remote's URL names no repository on a host, such as github.com/owner/repo",
            )
        })
    }

    async fn find_pr(&self, root: &Path, remote: &str, branch: &str) -> Result<Option<PrInfo>> {
        parse::remote(remote)?;
        parse::branch(branch)?;
        let gh = self.gh()?;
        let repo = self.repo(root, remote).await?;
        let args = [
            "pr",
            "list",
            "--repo",
            &repo,
            "--head",
            branch,
            "--state",
            "open",
            "--json",
            "number,url,isCrossRepository",
        ];
        let list = run(&gh, root, "gh pr list", &args, None, GH_TIMEOUT).await?;
        if !list.success {
            return Err(failed("gh pr list", &list));
        }
        parse::found(&list.stdout, repo)
    }

    async fn view_pr(&self, root: &Path, repo: &str, number: u64) -> Result<PrStatus> {
        parse::repo_name(repo)?;
        let gh = self.gh()?;
        let number = number.to_string();
        let fields =
            "state,isDraft,url,headRefOid,statusCheckRollup,reviewDecision,mergeStateStatus";
        let args = ["pr", "view", &number, "--repo", repo, "--json", fields];
        let view = run(&gh, root, "gh pr view", &args, None, GH_TIMEOUT).await?;
        if !view.success {
            return Err(failed("gh pr view", &view));
        }
        parse::pr_status(&view.stdout)
    }

    async fn run_log(&self, root: &Path, repo: &str, id: u64) -> Result<String> {
        parse::repo_name(repo)?;
        let gh = self.gh()?;
        let id = id.to_string();
        let args = ["run", "view", &id, "--repo", repo, "--log-failed"];
        let log = run(&gh, root, "gh run view", &args, None, GH_TIMEOUT).await?;
        if !log.success {
            return Err(failed("gh run view", &log));
        }
        // Scrubbed whole: a cut inside a URL would hide its `://`.
        Ok(parse::log_tail(&parse::scrub(&log.stdout)).to_owned())
    }
}

/// `remote`'s URL; `None` when git knows no such remote. It may hold a
/// token: callers keep only what `parse` takes from it.
async fn remote_url(git: &Path, root: &Path, remote: &str) -> Result<Option<String>> {
    let args = ["remote", "get-url", remote];
    let url = run(git, root, "git remote", &args, None, GH_TIMEOUT).await?;
    Ok(url.success.then_some(url.stdout))
}

/// Fetches `branch` of `remote` into the ref `into` and returns its commit.
async fn fetch_into(
    git: &Path,
    root: &Path,
    remote: &str,
    branch: &str,
    into: &str,
) -> Result<String> {
    let refspec = format!("+refs/heads/{branch}:{into}");
    let args = [NO_HOOKS[0], NO_HOOKS[1], "fetch", remote, &refspec];
    let fetched = run(git, root, "git fetch", &args, None, GIT_TIMEOUT).await?;
    if !fetched.success {
        if fetched.stderr.contains("couldn't find remote ref") {
            return Err(ProtocolError::new(
                ErrorCode::NotFound,
                "the remote has no branch of that name",
            ));
        }
        return Err(failed("git fetch", &fetched));
    }
    let commit = format!("{into}^{{commit}}");
    let args = ["rev-parse", "--verify", "--quiet", &commit];
    let head = run(git, root, "git rev-parse", &args, None, GH_TIMEOUT).await?;
    if !head.success {
        return Err(failed("git rev-parse", &head));
    }
    Ok(head.stdout.trim().to_owned())
}

impl Forge for GhForge {
    fn status(&self, root: PathBuf, remote: String) -> BoxFuture<'_, Result<GhStatus>> {
        Box::pin(async move { self.gh_status(&root, &remote).await })
    }

    fn fetch(
        &self,
        root: PathBuf,
        remote: String,
        branch: String,
    ) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move { self.fetch_branch(&root, &remote, &branch).await })
    }

    fn push(
        &self,
        root: PathBuf,
        remote: String,
        commit: String,
        branch: String,
        expect: Option<String>,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.push_commit(&root, &remote, &commit, &branch, expect.as_deref())
                .await
        })
    }

    fn create(&self, root: PathBuf, draft: PrDraft) -> BoxFuture<'_, Result<PrInfo>> {
        Box::pin(async move { self.create_pr(&root, &draft).await })
    }

    fn find(
        &self,
        root: PathBuf,
        remote: String,
        branch: String,
    ) -> BoxFuture<'_, Result<Option<PrInfo>>> {
        Box::pin(async move { self.find_pr(&root, &remote, &branch).await })
    }

    fn view(&self, root: PathBuf, repo: String, number: u64) -> BoxFuture<'_, Result<PrStatus>> {
        Box::pin(async move { self.view_pr(&root, &repo, number).await })
    }

    fn failed_log(&self, root: PathBuf, repo: String, run: u64) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move { self.run_log(&root, &repo, run).await })
    }
}
