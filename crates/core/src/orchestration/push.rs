//! `push`: a worker asks Ovra to publish its changes to a branch on a
//! remote (`docs/specs/agent-push.md`, ADR 0010 as amended). Ovra picks
//! the worker's changes onto the branch as one commit by the person, puts
//! it to the person on an approval card unless a `Push` rule decides, and
//! pushes it with the person's `git` through the forge: never forced,
//! without hooks, never with protected files.

use super::{Result, conflict, forbidden, publish, review};
use crate::Core;
use crate::agents::{AgentHost, ToolRequest};
use crate::policy::{self, PUSH_TOOL};
use crate::protocol::agents::{SessionMode, ToolCall};
use crate::protocol::audit::{AuditEvent, DecidedBy};
use crate::protocol::orchestration::{PushPlan, PushedFile};
use crate::protocol::permissions::Decision;
use crate::protocol::policy::PolicyAction;
use crate::protocol::review::ReviewState;
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{ToolCaller, ToolRole};

/// The longest title line of a pushed commit.
const MAX_TITLE: usize = 72;
/// The longest commit message.
const MAX_MESSAGE: usize = 20_000;

/// A worker's push, kept on its item: what went where, from which
/// snapshot of its worktree.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct AgentPush {
    pub(super) remote: String,
    pub(super) branch: String,
    pub(super) commit: String,
    pub(super) snapshot: String,
}

impl AgentPush {
    /// The snapshot to pick from onto `head` of `remote`/`branch`: this
    /// push's when the branch is still where it left it.
    pub(super) fn start_for(&self, remote: &str, branch: &str, head: &str) -> Option<String> {
        (self.remote == remote && self.branch == branch && self.commit == head)
            .then(|| self.snapshot.clone())
    }
}

/// What a worker's `push` asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushArgs {
    pub message: String,
    /// Default the worker's own branch.
    pub branch: Option<String>,
    /// Default `origin`.
    pub remote: Option<String>,
    /// What a new branch starts from.
    pub base: Option<String>,
}

/// What a push sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pushed {
    pub remote: String,
    pub branch: String,
    pub commit: String,
    /// Where a new branch started, `None` when the branch existed.
    pub base: Option<String>,
}

impl Core {
    /// `push`: the caller's changes as one commit on `branch` of `remote`,
    /// after the person's Allow or a `Push` rule. Workers only.
    pub async fn push_work(&self, caller: &ToolCaller, args: PushArgs) -> Result<Pushed> {
        if caller.role != ToolRole::Worker {
            return Err(forbidden(
                "a coordinator has no changes of its own; a worker pushes its work",
            ));
        }
        push(self, caller, args).await
    }
}

/// The commit message as given, checked: a title line of at most
/// [`MAX_TITLE`] characters, then an optional body.
fn commit_message(message: &str) -> Result<String> {
    let message = message.trim();
    let title = message.lines().next().unwrap_or_default().trim();
    let bad = |why: &str| Err(ProtocolError::new(ErrorCode::BadRequest, why));
    if title.is_empty() {
        return bad("the commit message needs a title line");
    }
    if title.chars().count() > MAX_TITLE {
        return bad("the commit message's title line is longer than 72 characters");
    }
    if message.len() > MAX_MESSAGE {
        return bad("the commit message is too long");
    }
    // What the card shows must be what the commit says: no escapes, no
    // text that runs backwards.
    if message.chars().any(crate::untrusted::hides_text) {
        return bad("the commit message holds control or text-direction characters");
    }
    // The trailer promises a person's review (AI_POLICY.md); the commit
    // is signed as the person.
    // git reads a trailer's key up to the `:`, spaces before it allowed
    // (`Reviewed-by : Name`).
    let claims_review = message.lines().any(|line| {
        line.split_once(':').is_some_and(|(key, _)| {
            let key = key.trim().to_ascii_lowercase();
            key == "reviewed-by" || key == "signed-off-by"
        })
    });
    if claims_review {
        return bad("the commit message may not carry Reviewed-by or Signed-off-by trailers");
    }
    Ok(message.to_owned())
}

async fn push(core: &Core, caller: &ToolCaller, args: PushArgs) -> Result<Pushed> {
    let entry = review::of_session(core, &caller.session)
        .filter(|e| e.mode == SessionMode::Isolated)
        .ok_or_else(|| forbidden("only a worker in its own worktree can push its work"))?;
    if !matches!(entry.state, ReviewState::Open | ReviewState::Published) {
        return Err(conflict(
            "the person already decided on this work; it is not pushed from here",
        ));
    }
    let (entry, worker, root) = publish::worker_item(core, &entry.id)?;
    let remote = crate::pr::remote_name(args.remote)?;
    let branch = match args.branch {
        Some(branch) => crate::pr::ref_name(&branch)?,
        None => worker.branch.clone(),
    };
    let message = commit_message(&args.message)?;
    if let Some(owner) = taken_branch(core, &entry, &remote, &branch) {
        return Err(forbidden(format!(
            "{remote}/{branch} is {owner}; push to your own branch ({}) or another one",
            worker.branch
        )));
    }

    let worktrees = core.worktrees();
    let snapshot = worktrees.snapshot(entry.workspace.dir.clone()).await?;
    let (files, _, _) = review::listed(core, &entry, Some(&snapshot)).await?;
    let marked: Vec<&str> = files
        .iter()
        .filter(|f| f.protected)
        .map(|f| f.path.as_str())
        .collect();
    if !marked.is_empty() {
        return Err(forbidden(format!(
            "these changes include protected files ({}), which only the person publishes: \
             ask for review and let them open a pull request",
            marked.join(", ")
        )));
    }
    publish::no_placeholders(core, &entry, &snapshot).await?;

    let forge = core.forge();
    let tracked = publish::tracked(core, root.clone(), &remote).await;
    let fetched = forge.fetch(root.clone(), remote.clone(), branch.clone());
    let (onto, expect, base) = match fetched.await {
        Ok(head) => (head.clone(), Some(head), None),
        Err(e) if e.code == ErrorCode::NotFound => {
            let base = match args.base {
                Some(base) => crate::pr::ref_name(&base)?,
                None => tracked.clone().unwrap_or_else(|| "main".to_owned()),
            };
            let head = forge.fetch(root.clone(), remote.clone(), base.clone());
            (head.await?, None, Some(base))
        }
        Err(e) => return Err(e),
    };
    let against = format!("{remote}/{}", base.as_deref().unwrap_or(&branch));
    // On top of its own last push, only what changed since goes in: the
    // head already holds the rest, and picking it again would conflict.
    // The same for the item's pull request as the person last pushed it.
    let published = || {
        let pr = entry.pull_request.as_ref()?;
        let same = pr.remote == remote && pr.branch == branch && pr.pushed == onto;
        same.then(|| entry.published_from.clone()).flatten()
    };
    let start = entry
        .agent_pushed
        .as_ref()
        .and_then(|p| p.start_for(&remote, &branch, &onto))
        .or_else(published)
        .unwrap_or_else(|| entry.workspace.base.clone());
    let commit = publish::pick(
        core,
        root.clone(),
        onto.clone(),
        start,
        snapshot.clone(),
        message.clone(),
        &against,
    )
    .await?;
    let deltas = worktrees.diff(root.clone(), onto.clone(), commit.clone());
    let deltas = deltas.await?;
    // The commit holds the worker's files only, but what goes out is
    // checked on its own: a protected file never leaves through the tool.
    if let Some(d) = deltas
        .iter()
        .find(|d| crate::protected::changes(&d.path, d.from.as_deref()))
    {
        return Err(forbidden(format!(
            "the push would change the protected file {}, which only the person publishes",
            d.path
        )));
    }
    // The card lists the paths one a line: none may fake or turn a row.
    if let Some(d) = deltas.iter().find(|d| {
        std::iter::once(&d.path).chain(d.from.as_ref()).any(|p| {
            p.chars()
                .any(|c| c == '\n' || c == '\t' || crate::untrusted::hides_text(c))
        })
    }) {
        return Err(forbidden(format!(
            "the push holds a file whose name has control or text-direction characters ({:?}); \
             rename it, or ask for review",
            d.path
        )));
    }
    let host = forge.status(root.clone(), remote.clone()).await.ok();
    // A remote git cannot read a host from may be a folder on this
    // computer: nothing a card can name.
    let Some(host) = host.and_then(|h| h.host) else {
        return Err(forbidden(format!(
            "{remote} is not a remote with a host in its URL; Ovra pushes only to those"
        )));
    };
    let plan = PushPlan {
        own_branch: branch == worker.branch,
        remote: remote.clone(),
        host: Some(host),
        // The item's pull request counts as shared: people may have read
        // or approved it, and auto-merge may be on.
        protected_branch: policy::protected_branch(&branch, tracked.as_deref())
            || entry
                .pull_request
                .as_ref()
                .is_some_and(|pr| pr.remote == remote && pr.branch == branch),
        branch: branch.clone(),
        new_branch: base.is_some(),
        base: base.clone(),
        onto,
        commit: commit.clone(),
        message,
        files: deltas
            .into_iter()
            .map(|d| PushedFile {
                path: d.path,
                kind: d.kind,
                from: d.from,
                additions: d.additions,
                deletions: d.deletions,
            })
            .collect(),
    };
    approved(core, caller, &plan).await?;
    // The card may have waited: work the person kept or discarded
    // meanwhile is not pushed from here.
    let still_open = review::of_session(core, &caller.session).is_some_and(|e| {
        e.id == entry.id && matches!(e.state, ReviewState::Open | ReviewState::Published)
    });
    if !still_open {
        return Err(conflict(
            "the person decided on this work while the push waited; nothing was pushed",
        ));
    }

    forge
        .push(root, remote.clone(), commit.clone(), branch.clone(), expect)
        .await
        .map_err(|e| match e.code {
            ErrorCode::Conflict => conflict(format!(
                "{remote}/{branch} changed since this push was prepared; nothing was pushed. \
                 Call push again to build on it"
            )),
            _ => e,
        })?;
    let pushed = AuditEvent::BranchPushed {
        review: entry.id.clone(),
        remote: remote.clone(),
        branch: branch.clone(),
        commit: commit.clone(),
    };
    crate::audit::record(core, &entry.project, Some(&entry.session), pushed);
    let last = AgentPush {
        remote: remote.clone(),
        branch: branch.clone(),
        commit: commit.clone(),
        snapshot: snapshot.clone(),
    };
    // The item's pull request follows its branch, so the person's next
    // Update builds on this push instead of failing on the lease. Only the
    // lease follows: what the person reviewed stays where it was, so the
    // review still shows what went out without them.
    let pr_branch = entry
        .pull_request
        .as_ref()
        .is_some_and(|pr| pr.remote == remote && pr.branch == branch);
    let updated = review::update(core, &entry.id, |e| {
        e.agent_pushed = Some(last);
        if let Some(pr) = e.pull_request.as_mut().filter(|_| pr_branch) {
            pr.pushed = commit.clone();
        }
    });
    // The push went out either way: an item removed meanwhile has nothing
    // left to record it on.
    if let (true, Ok(entry)) = (pr_branch, &updated) {
        publish::emit(core, entry);
    }
    let what = format!("pushed its work to {remote}/{branch}");
    super::tasks::tell_coordinator(core, &entry.session, &what);
    Ok(Pushed {
        remote,
        branch,
        commit,
        base,
    })
}

/// Whose `branch` on `remote` is, when it is another worker's branch or
/// the branch of another item's pull request: never pushed to from here.
fn taken_branch(core: &Core, entry: &review::Entry, remote: &str, branch: &str) -> Option<String> {
    let state = core.orchestration_state().lock();
    let worker = state
        .tasks
        .iter()
        .flat_map(|t| &t.workers)
        .find(|w| w.session != entry.session && w.branch == branch);
    if let Some(worker) = worker {
        return Some(format!("worker {}'s branch", worker.name));
    }
    state
        .reviews
        .iter()
        .filter(|r| r.session != entry.session)
        .filter_map(|r| r.pull_request.as_ref())
        .find(|pr| pr.remote == remote && pr.branch == branch)
        .map(|pr| format!("the branch of pull request #{}", pr.number))
}

/// Decides `plan` by the project's `Push` rules, else asks the person and
/// waits; `Ok` when it may go out.
async fn approved(core: &Core, caller: &ToolCaller, plan: &PushPlan) -> Result<()> {
    let tool = ToolRequest {
        name: PUSH_TOOL.to_owned(),
        input: serde_json::to_value(plan)
            .map_err(|e| ProtocolError::new(ErrorCode::Internal, format!("push plan: {e}")))?,
        call: ToolCall::default(),
        written: None,
    };
    let sandbox = core
        .sessions()
        .find(&caller.session, |s| s.info().sandbox)
        .map_err(|_| conflict("the session ended"))?;
    let target = plan.target();
    // An unreadable policy decides nothing: the person is asked.
    let decided = core
        .project_store()
        .policy(&caller.project)
        .ok()
        .and_then(|p| policy::decide_push(&p, &target, sandbox.as_ref()));
    let record = |allowed: bool, by: DecidedBy| {
        crate::audit::tool_decided(
            core,
            &caller.project,
            Some(&caller.session),
            &tool,
            allowed,
            by,
        )
    };
    match decided {
        Some((PolicyAction::Deny, rule)) => {
            let _ = record(false, DecidedBy::Policy { rule: rule.clone() }).await;
            return Err(forbidden(format!(
                "the project's policy refuses this push ({rule}); ask the person if it is needed"
            )));
        }
        Some((PolicyAction::Auto, rule)) => {
            let _ = record(true, DecidedBy::Policy { rule }).await;
            return Ok(());
        }
        _ => {}
    }
    let host = AgentHost::new(
        caller.session.clone(),
        caller.project.clone(),
        core.sessions().clone(),
        core.sink(),
    );
    let title = format!("Push to {}/{}", plan.remote, plan.branch);
    let always =
        |_: Option<&std::path::Path>, sandbox: Option<&_>| policy::always_push(&target, sandbox);
    let answer = host.ask_person(&tool, Some(title), None, always).await;
    if answer.decision == Decision::Allow {
        return Ok(());
    }
    let why = answer
        .message
        .filter(|m| !m.trim().is_empty())
        .map(|m| format!(": {m}"))
        .unwrap_or_default();
    Err(forbidden(format!(
        "the person did not allow this push{why}; nothing was pushed"
    )))
}

#[cfg(test)]
mod tests;
