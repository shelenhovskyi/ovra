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

use ovra_core::protocol::pr::{GhStatus, PrDraft, PrInfo, PrStatus};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, Forge, Result};

use run::{GH_TIMEOUT, GIT_TIMEOUT, Programs, failed, run};

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
            let args = ["remote", "get-url", remote];
            let url = run(git, root, "git remote", &args, None, GH_TIMEOUT).await?;
            // The URL may hold a token: only its host is kept.
            host = url.success.then(|| parse::host(&url.stdout)).flatten();
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
        let args = ["fetch", remote, branch];
        let fetched = run(&git, root, "git fetch", &args, None, GIT_TIMEOUT).await?;
        if !fetched.success {
            if fetched.stderr.contains("couldn't find remote ref") {
                return Err(ProtocolError::new(
                    ErrorCode::NotFound,
                    "the remote has no branch of that name",
                ));
            }
            return Err(failed("git fetch", &fetched));
        }
        let args = ["rev-parse", "--verify", "--quiet", "FETCH_HEAD^{commit}"];
        let head = run(&git, root, "git rev-parse", &args, None, GH_TIMEOUT).await?;
        if !head.success {
            return Err(failed("git rev-parse", &head));
        }
        Ok(head.stdout.trim().to_owned())
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
        let title = format!("--title={}", draft.title);
        let mut args = vec![
            "pr",
            "create",
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
        parse::created(&made.stdout)
    }

    async fn view_pr(&self, root: &Path, number: u64) -> Result<PrStatus> {
        let gh = self.gh()?;
        let number = number.to_string();
        let fields =
            "state,isDraft,url,headRefOid,statusCheckRollup,reviewDecision,mergeStateStatus";
        let args = ["pr", "view", &number, "--json", fields];
        let view = run(&gh, root, "gh pr view", &args, None, GH_TIMEOUT).await?;
        if !view.success {
            return Err(failed("gh pr view", &view));
        }
        parse::pr_status(&view.stdout)
    }

    async fn run_log(&self, root: &Path, id: u64) -> Result<String> {
        let gh = self.gh()?;
        let id = id.to_string();
        let args = ["run", "view", &id, "--log-failed"];
        let log = run(&gh, root, "gh run view", &args, None, GH_TIMEOUT).await?;
        if !log.success {
            return Err(failed("gh run view", &log));
        }
        // Scrubbed whole: a cut inside a URL would hide its `://`.
        Ok(parse::log_tail(&parse::scrub(&log.stdout)).to_owned())
    }
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

    fn view(&self, root: PathBuf, number: u64) -> BoxFuture<'_, Result<PrStatus>> {
        Box::pin(async move { self.view_pr(&root, number).await })
    }

    fn failed_log(&self, root: PathBuf, run: u64) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move { self.run_log(&root, run).await })
    }
}
