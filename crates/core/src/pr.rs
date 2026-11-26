//! `pr.*`: pull requests of reviewed workers (`docs/specs/pr-flow.md`).
//! `pr.status` asks the forge; the rest works on review items
//! (`orchestration::publish`).

use std::path::PathBuf;

use crate::Core;
use crate::protocol::pr::{BranchPr, CheckLog, CheckState, PrRequest, PrResponse};
use crate::protocol::{ErrorCode, ProtocolError, Response};

/// The remote a request names none: the one `git clone` sets up.
pub(crate) const DEFAULT_REMOTE: &str = "origin";

pub(crate) async fn handle(core: &Core, request: PrRequest) -> Result<Response, ProtocolError> {
    match request {
        PrRequest::Status { project, remote } => {
            let root = core.root(&project)?;
            let remote = remote_name(remote)?;
            let status = core.forge().status(root, remote).await?;
            Ok(Response::Pr(PrResponse::Status(status)))
        }
        PrRequest::Issues { project } => {
            let root = core.root(&project)?;
            let issues = core.forge().issues(root, DEFAULT_REMOTE.to_owned());
            Ok(Response::Pr(PrResponse::Issues(issues.await?)))
        }
        PrRequest::Issue { project, number } => {
            let root = core.root(&project)?;
            let issue = core.forge().issue(root, DEFAULT_REMOTE.to_owned(), number);
            let mut issue = issue.await?;
            issue.title = cut_end(&issue.title, MAX_TITLE).to_owned();
            issue.text = cut_end(&issue.text, MAX_LOG).to_owned();
            Ok(Response::Pr(PrResponse::Issue(issue)))
        }
        PrRequest::BranchChecks { project } => {
            let root = core.root(&project)?;
            let found = branch_pr(core, root).await?.map(|(pr, _)| pr);
            Ok(Response::Pr(PrResponse::BranchChecks(found)))
        }
        PrRequest::CheckLog { project, run } => {
            let root = core.root(&project)?;
            let (pr, repo) = branch_pr(core, root.clone()).await?.ok_or_else(|| {
                ProtocolError::new(ErrorCode::NotFound, "the branch has no open pull request")
            })?;
            let check = pr
                .failed
                .into_iter()
                .find(|c| c.run == Some(run))
                .ok_or_else(|| {
                    ProtocolError::new(ErrorCode::NotFound, format!("no failed check of run {run}"))
                })?;
            let log = core.forge().failed_log(root, repo, run).await?;
            Ok(Response::Pr(PrResponse::CheckLog(CheckLog {
                pr: pr.number,
                name: cut_end(&check.name, MAX_TITLE).to_owned(),
                url: check.url,
                text: cut_start(&log, MAX_LOG).to_owned(),
            })))
        }
        request => crate::orchestration::pull_request(core, request).await,
    }
}

/// A title or a check's name, at most: they go to the agent with the text.
const MAX_TITLE: usize = 300;
/// An issue's text or a log's end, at most: one context item holds 16 KiB.
const MAX_LOG: usize = 15 * 1024;

/// The start of `text`, at most `max` bytes.
fn cut_end(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The end of `text`, at most `max` bytes: where a log says what failed.
fn cut_start(text: &str, max: usize) -> &str {
    let mut start = text.len().saturating_sub(max);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// The open pull request from the main checkout's branch, found by the
/// branch's upstream, with its failed checks that have a log, and the
/// repository it is in; `None` without an upstream or a pull request.
async fn branch_pr(
    core: &Core,
    root: PathBuf,
) -> Result<Option<(BranchPr, String)>, ProtocolError> {
    let Some(upstream) = core.git_service().upstream(root.clone()).await? else {
        return Ok(None);
    };
    let remote = remote_name(Some(upstream.remote))?;
    let branch = ref_name(&upstream.branch)?;
    let forge = core.forge();
    let Some(info) = forge.find(root.clone(), remote, branch.clone()).await? else {
        return Ok(None);
    };
    let status = forge.view(root, info.repo.clone(), info.number).await?;
    let failed = status
        .checks
        .into_iter()
        .filter(|c| c.state == CheckState::Failed && c.run.is_some())
        .collect();
    let pr = BranchPr {
        number: info.number,
        url: info.url,
        branch,
        failed,
    };
    Ok(Some((pr, info.repo)))
}

/// A remote's name as git allows it on a command line: never a URL, a
/// path or something `git` or `gh` would read as an option.
pub(crate) fn remote_name(remote: Option<String>) -> Result<String, ProtocolError> {
    let remote = remote.unwrap_or_else(|| DEFAULT_REMOTE.to_owned());
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    if remote.is_empty() || remote.starts_with(['-', '.']) || !remote.chars().all(plain) {
        // Not echoed: a caller may have passed a URL with a token in it.
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "a remote must be given by its name, such as origin",
        ));
    }
    Ok(remote)
}

/// A branch name git and `gh` take as nothing but a branch.
pub(crate) fn ref_name(name: &str) -> Result<String, ProtocolError> {
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/');
    let bad = name.is_empty()
        || name.len() > 200
        || name.starts_with(['-', '/', '.'])
        || name.ends_with(['/', '.'])
        || name.ends_with(".lock")
        || name.contains("..")
        || name.contains("//")
        || name.contains("/.")
        || !name.chars().all(plain);
    if bad {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!("{name:?} is not a branch name Ovra pushes to"),
        ));
    }
    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use crate::Core;
    use crate::fakes::{MemoryForge, MemoryProjects};
    use crate::protocol::pr::PrResponse;
    use crate::protocol::{ErrorCode, Request, Response};

    async fn call(core: &Core, json: serde_json::Value) -> Result<Response, ErrorCode> {
        let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
        core.handle(request).await.map_err(|e| e.code)
    }

    #[tokio::test]
    async fn status_asks_the_forge_and_refuses_what_is_not_a_remote() {
        let forge = Arc::new(MemoryForge::default());
        let core = Core::builder("test")
            .projects(Arc::new(MemoryProjects::default()))
            .forge(forge.clone())
            .build();
        let root = crate::test_path!("/work/a");
        let add = json!({"method": "projects.add", "params": {"root": root}});
        assert!(call(&core, add).await.is_ok());

        let status = json!({"method": "pr.status", "params": {"project": "p1"}});
        let Ok(Response::Pr(PrResponse::Status(gh))) = call(&core, status).await else {
            panic!("no status");
        };
        assert!(gh.signed_in);
        assert_eq!(
            *forge.asked.lock().unwrap_or_else(|e| e.into_inner()),
            [(root.into(), "origin".to_owned())]
        );
        for remote in ["https://x@github.com/o/r", "--upload-pack=x", "", "../r"] {
            let asked =
                json!({"method": "pr.status", "params": {"project": "p1", "remote": remote}});
            assert_eq!(call(&core, asked).await.err(), Some(ErrorCode::BadRequest));
        }
        let secret = "https://x:ghp_SECRET@github.com/o/r";
        let refused = core
            .handle(Request::Pr(crate::protocol::pr::PrRequest::Status {
                project: crate::protocol::projects::ProjectId("p1".into()),
                remote: Some(secret.into()),
            }))
            .await;
        let message = refused.err().map(|e| e.message).unwrap_or_default();
        assert!(
            !message.contains("SECRET") && !message.is_empty(),
            "{message}"
        );
        let prepare = json!({"method": "pr.prepare", "params": {"id": "r1"}});
        assert_eq!(call(&core, prepare).await.err(), Some(ErrorCode::NotFound));
    }

    #[tokio::test]
    async fn issues_and_the_branchs_failed_checks_are_read_for_the_at_menu() {
        use crate::fakes::MemoryGit;
        use crate::protocol::git::RepoStatus;
        use crate::protocol::pr::{CheckState, Issue, PrCheck, PrDraft, PrState, PrStatus};

        let forge = Arc::new(MemoryForge::default());
        let git = Arc::new(MemoryGit::repo());
        let core = Core::builder("test")
            .projects(Arc::new(MemoryProjects::default()))
            .forge(forge.clone())
            .git(git.clone())
            .build();
        let root = crate::test_path!("/work/a");
        let add = json!({"method": "projects.add", "params": {"root": root}});
        assert!(call(&core, add).await.is_ok());
        fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
            m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
        }
        lock(&forge.issues).extend([
            Issue {
                number: 12,
                title: "Login fails".into(),
                url: "https://github.com/o/r/issues/12".into(),
                text: "x".repeat(20 * 1024),
            },
            Issue {
                number: 9,
                title: "Dark theme".into(),
                url: "https://github.com/o/r/issues/9".into(),
                text: "hard to read".into(),
            },
        ]);

        let list = json!({"method": "pr.issues", "params": {"project": "p1"}});
        let Ok(Response::Pr(PrResponse::Issues(found))) = call(&core, list).await else {
            panic!("no issues");
        };
        assert_eq!(found.iter().map(|i| i.number).collect::<Vec<_>>(), [12, 9]);
        let view = json!({"method": "pr.issue", "params": {"project": "p1", "number": 12}});
        let Ok(Response::Pr(PrResponse::Issue(issue))) = call(&core, view).await else {
            panic!("no issue");
        };
        // Cut to fit one context item.
        assert_eq!(issue.text.len(), super::MAX_LOG);

        // No upstream: no pull request to look for.
        let checks = json!({"method": "pr.branch_checks", "params": {"project": "p1"}});
        let none = call(&core, checks.clone()).await;
        assert!(matches!(
            none,
            Ok(Response::Pr(PrResponse::BranchChecks(None)))
        ));
        if let Some(status) = lock(&git.status).as_mut() {
            *status = RepoStatus {
                branch: Some("fix".into()),
                upstream: Some("origin/fix".into()),
                ..status.clone()
            };
        }
        lock(&forge.created).push(PrDraft {
            remote: "origin".into(),
            base: "main".into(),
            branch: "fix".into(),
            title: "Fix".into(),
            body: String::new(),
            draft: false,
        });
        let check = |name: &str, state, run| PrCheck {
            name: name.into(),
            state,
            url: None,
            run,
        };
        lock(&forge.pulls).insert(
            1,
            PrStatus {
                state: PrState::Open,
                draft: false,
                url: "https://github.com/o/r/pull/1".into(),
                head: "c1".into(),
                checks: vec![
                    check("build", CheckState::Passed, Some(1)),
                    check("test", CheckState::Failed, Some(2)),
                    check("external", CheckState::Failed, None),
                ],
                review_decision: None,
                merge_state: None,
            },
        );
        lock(&forge.logs).insert(1, "build ok".into());
        lock(&forge.logs).insert(
            2,
            format!("{}\nerror: 1 test failed", "y".repeat(20 * 1024)),
        );
        let Ok(Response::Pr(PrResponse::BranchChecks(Some(pr)))) = call(&core, checks).await else {
            panic!("no pull request");
        };
        assert_eq!((pr.number, pr.branch.as_str()), (1, "fix"));
        assert_eq!(
            pr.failed
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["test"]
        );

        let log = json!({"method": "pr.check_log", "params": {"project": "p1", "run": 2}});
        let Ok(Response::Pr(PrResponse::CheckLog(log))) = call(&core, log).await else {
            panic!("no log");
        };
        assert_eq!((log.pr, log.name.as_str()), (1, "test"));
        assert!(log.text.len() == super::MAX_LOG && log.text.ends_with("1 test failed"));
        // Only a failed check of the branch's pull request.
        let passed = json!({"method": "pr.check_log", "params": {"project": "p1", "run": 1}});
        assert_eq!(call(&core, passed).await.err(), Some(ErrorCode::NotFound));
    }
}
