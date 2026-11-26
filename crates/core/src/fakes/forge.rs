//! [`MemoryForge`]: a remote and its pull requests as maps.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{lock, not_found};
use crate::protocol::pr::{GhStatus, Issue, IssueSummary, PrDraft, PrInfo, PrState, PrStatus};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{BoxFuture, Forge, Result};

/// Records every call. Branches are `(remote, branch)` to commit; `push`
/// moves one only from where the caller expects it. `create` opens pull
/// requests 1, 2, ... as open with no checks; tests change `pulls` to move
/// them on. The remote's repository is [`MemoryForge::REPO`]; `view` and
/// `failed_log` find nothing in another.
#[derive(Debug)]
pub struct MemoryForge {
    /// What `status` answers; signed in to `github.com` as `octo` at first.
    pub gh: Mutex<GhStatus>,
    /// `(root, remote)` of every `status` call.
    pub asked: Mutex<Vec<(PathBuf, String)>>,
    pub branches: Mutex<BTreeMap<(String, String), String>>,
    /// `(remote, commit, branch)` of every push that went through.
    pub pushed: Mutex<Vec<(String, String, String)>>,
    pub created: Mutex<Vec<PrDraft>>,
    /// By number.
    pub pulls: Mutex<BTreeMap<u64, PrStatus>>,
    /// Failed logs by workflow run.
    pub logs: Mutex<BTreeMap<u64, String>>,
    /// The remote's open issues, newest first.
    pub issues: Mutex<Vec<Issue>>,
    /// While set, `create` fails (`Unavailable`), as when GitHub is down.
    pub refuse_create: Mutex<bool>,
    /// While set, `create` opens the pull request but fails, as when
    /// `gh pr create` timed out after GitHub made it.
    pub lose_create: Mutex<bool>,
}

impl Default for MemoryForge {
    fn default() -> Self {
        Self {
            gh: Mutex::new(GhStatus {
                installed: true,
                version: Some("2.60.0".into()),
                host: Some("github.com".into()),
                login: Some("octo".into()),
                signed_in: true,
            }),
            asked: Mutex::default(),
            branches: Mutex::default(),
            pushed: Mutex::default(),
            created: Mutex::default(),
            pulls: Mutex::default(),
            logs: Mutex::default(),
            issues: Mutex::default(),
            refuse_create: Mutex::default(),
            lose_create: Mutex::default(),
        }
    }
}

impl MemoryForge {
    pub const REPO: &str = "github.com/o/r";

    /// The open pull request from `remote`'s `branch`.
    fn open_from(&self, remote: &str, branch: &str) -> Option<PrInfo> {
        let created = lock(&self.created);
        let pulls = lock(&self.pulls);
        let numbers = (1..).zip(created.iter());
        let mut from = numbers.filter(|(_, d)| d.remote == remote && d.branch == branch);
        from.find_map(|(number, _)| {
            let pull = pulls.get(&number).filter(|p| p.state == PrState::Open)?;
            Some(PrInfo {
                number,
                url: pull.url.clone(),
                repo: Self::REPO.to_owned(),
            })
        })
    }
}

impl Forge for MemoryForge {
    fn status(&self, root: PathBuf, remote: String) -> BoxFuture<'_, Result<GhStatus>> {
        lock(&self.asked).push((root, remote));
        let gh = lock(&self.gh).clone();
        Box::pin(async move { Ok(gh) })
    }

    fn fetch(&self, _: PathBuf, remote: String, branch: String) -> BoxFuture<'_, Result<String>> {
        let found = lock(&self.branches)
            .get(&(remote.clone(), branch.clone()))
            .cloned();
        Box::pin(async move { found.ok_or_else(|| not_found(format!("{remote}/{branch}"))) })
    }

    fn push(
        &self,
        _: PathBuf,
        remote: String,
        commit: String,
        branch: String,
        expect: Option<String>,
    ) -> BoxFuture<'_, Result<()>> {
        let mut branches = lock(&self.branches);
        let key = (remote.clone(), branch.clone());
        let result = if branches.get(&key) == expect.as_ref() {
            branches.insert(key, commit.clone());
            lock(&self.pushed).push((remote, commit, branch));
            Ok(())
        } else {
            Err(ProtocolError::new(
                ErrorCode::Conflict,
                format!("{remote}/{branch} moved"),
            ))
        };
        Box::pin(async move { result })
    }

    fn create(&self, _: PathBuf, draft: PrDraft) -> BoxFuture<'_, Result<PrInfo>> {
        let head = lock(&self.branches)
            .get(&(draft.remote.clone(), draft.branch.clone()))
            .cloned();
        let open = self.open_from(&draft.remote, &draft.branch);
        let result = match head {
            _ if *lock(&self.refuse_create) => Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "gh pr create failed",
            )),
            _ if open.is_some() => Err(ProtocolError::new(
                ErrorCode::Internal,
                "gh pr create failed: a pull request for this branch already exists",
            )),
            None => Err(not_found(format!("{}/{}", draft.remote, draft.branch))),
            Some(head) => {
                let mut created = lock(&self.created);
                let number = created.len() as u64 + 1;
                let url = format!("https://github.com/o/r/pull/{number}");
                let status = PrStatus {
                    state: PrState::Open,
                    draft: draft.draft,
                    url: url.clone(),
                    head,
                    checks: Vec::new(),
                    review_decision: None,
                    merge_state: None,
                };
                lock(&self.pulls).insert(number, status);
                created.push(draft);
                if *lock(&self.lose_create) {
                    Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "gh pr create timed out",
                    ))
                } else {
                    Ok(PrInfo {
                        number,
                        url,
                        repo: Self::REPO.to_owned(),
                    })
                }
            }
        };
        Box::pin(async move { result })
    }

    fn find(
        &self,
        _: PathBuf,
        remote: String,
        branch: String,
    ) -> BoxFuture<'_, Result<Option<PrInfo>>> {
        let open = self.open_from(&remote, &branch);
        Box::pin(async move { Ok(open) })
    }

    fn view(&self, _: PathBuf, repo: String, number: u64) -> BoxFuture<'_, Result<PrStatus>> {
        let found = lock(&self.pulls)
            .get(&number)
            .filter(|_| repo == Self::REPO)
            .cloned();
        Box::pin(async move { found.ok_or_else(|| not_found(format!("pull request {number}"))) })
    }

    fn failed_log(&self, _: PathBuf, repo: String, run: u64) -> BoxFuture<'_, Result<String>> {
        let found = lock(&self.logs)
            .get(&run)
            .filter(|_| repo == Self::REPO)
            .cloned();
        Box::pin(async move { found.ok_or_else(|| not_found(format!("run {run}"))) })
    }

    fn issues(&self, _: PathBuf, _: String) -> BoxFuture<'_, Result<Vec<IssueSummary>>> {
        let found = lock(&self.issues)
            .iter()
            .take(30)
            .map(|i| IssueSummary {
                number: i.number,
                title: i.title.clone(),
                url: i.url.clone(),
            })
            .collect();
        Box::pin(async move { Ok(found) })
    }

    fn issue(&self, _: PathBuf, _: String, number: u64) -> BoxFuture<'_, Result<Issue>> {
        let found = lock(&self.issues)
            .iter()
            .find(|i| i.number == number)
            .cloned();
        Box::pin(async move { found.ok_or_else(|| not_found(format!("issue {number}"))) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_push_moves_the_branch_only_from_where_it_was_expected() {
        let forge = MemoryForge::default();
        let root = PathBuf::from("/p");
        let (remote, branch) = ("origin".to_owned(), "ovra/t1/w".to_owned());
        let push = |commit: &str, expect: Option<&str>| {
            forge.push(
                root.clone(),
                remote.clone(),
                commit.into(),
                branch.clone(),
                expect.map(str::to_owned),
            )
        };
        assert!(push("c1", None).await.is_ok());
        assert!(push("c2", Some("c1")).await.is_ok());
        let moved = push("c3", Some("c1")).await;
        assert_eq!(moved.err().map(|e| e.code), Some(ErrorCode::Conflict));
        let head = forge
            .fetch(root.clone(), remote.clone(), branch.clone())
            .await;
        assert_eq!(head.ok().as_deref(), Some("c2"));

        let draft = PrDraft {
            remote,
            base: "main".into(),
            branch,
            title: "t".into(),
            body: "b".into(),
            draft: true,
        };
        let (remote, branch) = (draft.remote.clone(), draft.branch.clone());
        let info = forge.create(root.clone(), draft.clone()).await;
        assert_eq!(info.map(|i| i.number).ok(), Some(1));
        let again = forge.create(root.clone(), draft).await;
        assert!(again.is_err());
        let found = forge.find(root.clone(), remote, branch).await;
        assert_eq!(found.ok().flatten().map(|i| i.number), Some(1));
        let elsewhere = forge.view(root.clone(), "github.com/up/r".into(), 1).await;
        assert!(elsewhere.is_err());
        let view = forge.view(root, MemoryForge::REPO.into(), 1).await;
        assert_eq!(
            view.map(|v| (v.head, v.draft)).ok(),
            Some(("c2".into(), true))
        );
    }
}
