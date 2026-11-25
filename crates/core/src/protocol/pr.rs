//! `pr.*`: pull requests of reviewed workers through the person's own
//! `gh` (`docs/specs/pr-flow.md`, ADR 0010).

use serde::{Deserialize, Serialize};

use super::projects::ProjectId;
use super::review::{FileChange, ReviewId};

/// The person's GitHub CLI, as `gh --version` and `gh auth status` say.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GhStatus {
    /// `gh` was found.
    pub installed: bool,
    pub version: Option<String>,
    /// The remote's host, such as `github.com`; `None` when the remote's
    /// URL names none.
    pub host: Option<String>,
    /// The account `gh` is signed in with on that host.
    pub login: Option<String>,
    pub signed_in: bool,
}

/// A pull request to open (`gh pr create`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDraft {
    /// A remote's name, such as `origin`; never a URL.
    pub remote: String,
    /// The branch it asks to merge into.
    pub base: String,
    /// The pushed branch it comes from.
    pub branch: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
}

/// A pull request `gh pr create` opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrInfo {
    pub number: u64,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrState {
    Open,
    Closed,
    Merged,
}

/// What a check says. Closed set: the UI counts and words each one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    /// Queued or running.
    Pending,
    Passed,
    /// Failed, cancelled, timed out or waiting for an action.
    Failed,
    /// Skipped or neutral.
    Skipped,
}

/// One check of a pull request's head commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrCheck {
    pub name: String,
    pub state: CheckState,
    /// Its page, opened through `open.url`.
    pub url: Option<String>,
    /// The workflow run it belongs to, for `pr.send_log`; `None` for a
    /// check that is not a GitHub Actions run.
    pub run: Option<u64>,
}

/// What `gh pr view` says about a pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrStatus {
    pub state: PrState,
    pub draft: bool,
    pub url: String,
    /// The commit the PR's branch is at.
    pub head: String,
    pub checks: Vec<PrCheck>,
    /// GitHub's `reviewDecision`, such as `APPROVED`; `None` without one.
    pub review_decision: Option<String>,
    /// GitHub's `mergeStateStatus`, such as `CLEAN` or `BLOCKED`.
    pub merge_state: Option<String>,
}

/// The pull request a review item was published as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
    pub remote: String,
    pub base: String,
    pub branch: String,
    /// The last commit Ovra pushed to `branch`: an update is refused when
    /// the remote branch moved away from it.
    pub pushed: String,
    /// What it said when last asked; `None` until then.
    pub status: Option<PrStatus>,
}

/// What `pr.prepare` drafted for the dialog. The click that sends it is
/// the person's approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrPrepared {
    pub remote: String,
    /// The remote's host, such as `github.com`.
    pub host: Option<String>,
    pub base: String,
    pub branch: String,
    /// Drafted by the worker; the person edits them.
    pub title: String,
    pub body: String,
    /// What the commit holds.
    pub files: Vec<FileChange>,
    /// Files the worker's changes conflict in with the fetched base: the
    /// pull request cannot be made until the worker merged it.
    pub conflicts: Vec<String>,
    /// The `ReviewItem::fingerprint` these files were read from;
    /// `pr.create` sends it back.
    pub fingerprint: Option<String>,
    pub gh: GhStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum PrRequest {
    /// Whether `gh` is installed and signed in for `remote`'s host
    /// (`origin` when `None`).
    #[serde(rename = "pr.status")]
    Status {
        project: ProjectId,
        #[serde(default)]
        remote: Option<String>,
    },
    /// Fetches the base branch and drafts a pull request of a worker's
    /// open item; `None` picks `origin` and its default branch.
    #[serde(rename = "pr.prepare")]
    Prepare {
        id: ReviewId,
        #[serde(default)]
        remote: Option<String>,
        #[serde(default)]
        base: Option<String>,
    },
    /// Commits what the person reviewed on the fetched base, pushes it to
    /// `branch` and opens the pull request; the item becomes `published`.
    #[serde(rename = "pr.create")]
    Create {
        id: ReviewId,
        draft: PrDraft,
        /// The `PrPrepared::fingerprint` the person saw.
        fingerprint: Option<String>,
    },
    /// Pushes the worker's newer reviewed changes to the item's pull
    /// request as a fast-forward; refused (`conflict`) when the remote
    /// branch moved.
    #[serde(rename = "pr.update")]
    Update {
        id: ReviewId,
        fingerprint: Option<String>,
    },
    /// Asks `gh` about the item's pull request now.
    #[serde(rename = "pr.refresh")]
    Refresh { id: ReviewId },
    /// Sends a failed check's log to the worker as a comment, framed as CI
    /// output. Answers `ok`.
    #[serde(rename = "pr.send_log")]
    SendLog { id: ReviewId, run: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum PrResponse {
    #[serde(rename = "pr.status")]
    Status(GhStatus),
    #[serde(rename = "pr.prepared")]
    Prepared(PrPrepared),
    /// After create, update or refresh.
    #[serde(rename = "pr.pull_request")]
    PullRequest(PullRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum PrEvent {
    /// An item's pull request or its checks changed; clients re-fetch
    /// `review.list`.
    #[serde(rename = "pr.changed")]
    Changed { project: ProjectId, id: ReviewId },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Event, Request, Response};

    #[test]
    fn wire_format_is_stable() {
        let id = || ReviewId("r1".into());
        let status = PrStatus {
            state: PrState::Open,
            draft: true,
            url: "https://github.com/o/r/pull/7".into(),
            head: "abc".into(),
            checks: vec![PrCheck {
                name: "test".into(),
                state: CheckState::Failed,
                url: Some("https://github.com/o/r/actions/runs/9".into()),
                run: Some(9),
            }],
            review_decision: None,
            merge_state: Some("BLOCKED".into()),
        };
        let draft = PrDraft {
            remote: "origin".into(),
            base: "main".into(),
            branch: "ovra/t1/fix".into(),
            title: "Fix the login".into(),
            body: "Why.".into(),
            draft: true,
        };
        insta::assert_json_snapshot!(serde_json::json!({
            "requests": [
                Request::Pr(PrRequest::Status { project: ProjectId("p1".into()), remote: None }),
                Request::Pr(PrRequest::Create { id: id(), draft, fingerprint: Some("f1".into()) }),
                Request::Pr(PrRequest::SendLog { id: id(), run: 9 }),
            ],
            "responses": [
                Response::Pr(PrResponse::Status(GhStatus {
                    installed: true,
                    version: Some("2.60.0".into()),
                    host: Some("github.com".into()),
                    login: Some("octo".into()),
                    signed_in: true,
                })),
                Response::Pr(PrResponse::PullRequest(PullRequest {
                    number: 7,
                    url: status.url.clone(),
                    remote: "origin".into(),
                    base: "main".into(),
                    branch: "ovra/t1/fix".into(),
                    pushed: "abc".into(),
                    status: Some(status),
                })),
            ],
            "events": [Event::from(PrEvent::Changed { project: ProjectId("p1".into()), id: id() })],
        }));
        let parsed: Request = serde_json::from_value(serde_json::json!({
            "method": "pr.prepare", "params": {"id": "r1"}}))
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(
            parsed,
            Request::Pr(PrRequest::Prepare {
                remote: None,
                base: None,
                ..
            })
        ));
    }
}
