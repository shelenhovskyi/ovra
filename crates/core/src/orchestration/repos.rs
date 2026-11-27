//! A solo task across several repositories (`docs/specs/multi-repo.md`,
//! ADR 0015): the worker's copies of the linked repositories, made, named
//! to the agent and given to its sandbox.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Result, Workspace, prepare, tasks};
use crate::Core;
use crate::protocol::agents::{AgentKind, SessionId};
use crate::protocol::orchestration::LinkedCopy;
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::IncludedFile;

/// Linked repositories a task may take besides its own.
pub(super) const MAX_LINKED: usize = 3;

/// What Ovra keeps of one linked copy besides [`LinkedCopy`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Kept {
    pub(super) project: ProjectId,
    /// What `.worktreeinclude` copied in, which does not block removal.
    #[serde(default)]
    pub(super) included: Vec<IncludedFile>,
}

/// One linked copy as made for a new worker.
#[derive(Debug)]
pub(super) struct Made {
    pub(super) copy: LinkedCopy,
    pub(super) workspace: Workspace,
    pub(super) kept: Kept,
    /// Ovra's words for the first message if `.worktreeinclude` failed.
    pub(super) note: Option<String>,
}

fn bad(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, message)
}

/// Refuses `repos` for a task of `project` unless each is another
/// project of the person's list that is a git repository, none twice, at
/// most [`MAX_LINKED`], for a provider that can add folders.
pub(super) async fn check(
    core: &Core,
    project: &ProjectId,
    repos: &[ProjectId],
    agent: AgentKind,
) -> Result<()> {
    if repos.len() > MAX_LINKED {
        return Err(bad(format!(
            "a task changes at most {MAX_LINKED} other repositories"
        )));
    }
    if agent != AgentKind::Claude {
        return Err(bad(
            "only Claude can work in several repositories at once for now",
        ));
    }
    for (i, repo) in repos.iter().enumerate() {
        if repo == project || repos[..i].contains(repo) {
            return Err(bad("each repository can be in a task once"));
        }
        let found = core.project_store().get(repo)?;
        if core
            .worktrees()
            .snapshot(PathBuf::from(&found.root))
            .await
            .is_err()
        {
            return Err(bad(format!(
                "{} is not a git repository, so the agent cannot get its own copy of it",
                found.name
            )));
        }
    }
    Ok(())
}

/// Makes `name`'s copy of each of `repos` for `task`, like its worktree:
/// a snapshot of that repository's main checkout, branch
/// `ovra/<task>/<name>`. If one fails, none stay.
pub(super) async fn make(
    core: &Core,
    task: &tasks::Entry,
    worktrees_dir: &Path,
    name: &str,
    repos: &[ProjectId],
) -> Result<Vec<Made>> {
    let mut made: Vec<Made> = Vec::new();
    for repo in repos {
        match make_one(core, task, worktrees_dir, name, repo).await {
            Ok(one) => made.push(one),
            Err(err) => {
                remove(core, made).await;
                let what = core
                    .project_store()
                    .get(repo)
                    .map_or_else(|_| repo.0.clone(), |p| p.name);
                return Err(ProtocolError::new(
                    err.code,
                    format!("the copy of {what} could not be made: {}", err.message),
                ));
            }
        }
    }
    Ok(made)
}

async fn make_one(
    core: &Core,
    task: &tasks::Entry,
    worktrees_dir: &Path,
    name: &str,
    repo: &ProjectId,
) -> Result<Made> {
    let root = core.root(repo)?;
    let worktrees = core.worktrees();
    let base = worktrees.snapshot(root.clone()).await?;
    let dir = worktrees_dir.join(&repo.0).join(&task.id.0).join(name);
    let branch = format!("ovra/{}/{name}", task.id.0);
    let reason = format!("Ovra: {}/{name}", task.id.0);
    let made = prepare::make(core, &root, dir, branch, base, reason).await?;
    let git = worktrees.git_files(root).await.ok().map(|g| g.files);
    Ok(Made {
        copy: LinkedCopy {
            project: repo.clone(),
            worktree: made.path.to_string_lossy().into_owned(),
            branch: made.branch,
        },
        workspace: Workspace {
            dir: made.path,
            base: made.base,
            git,
        },
        kept: Kept {
            project: repo.clone(),
            included: made.included,
        },
        note: made.note,
    })
}

/// Removes copies that were made for a worker that never started.
pub(super) async fn remove(core: &Core, made: Vec<Made>) {
    for one in made {
        if let Ok(root) = core.root(&one.copy.project) {
            let _ = tasks::remove(core, &root, one.workspace.dir, one.kept.included).await;
        }
    }
}

/// Ovra's words for the agent's first message: where each linked copy is.
/// Project names are the person's own; they are escaped all the same.
pub(super) fn note(core: &Core, made: &[Made]) -> Option<String> {
    if made.is_empty() {
        return None;
    }
    let mut text = String::from(
        "Ovra: this task also changes other repositories. Each has its own copy, which you \
         can read and change like the folder you work in:",
    );
    for one in made {
        let name = core
            .project_store()
            .get(&one.copy.project)
            .map_or_else(|_| one.copy.project.0.clone(), |p| p.name);
        text.push_str(&format!(
            "\n- {}: {}",
            crate::provenance::escaped(&name),
            crate::provenance::escaped(&one.copy.worktree)
        ));
    }
    for note in made.iter().filter_map(|m| m.note.as_deref()) {
        text.push('\n');
        text.push_str(note);
    }
    Some(text)
}

/// The linked copies of the worker `session` and the git folders of their
/// main checkouts, which a worktree's history lives in: its sandbox writes
/// the copies and reads those folders, and its agent adds the copies.
pub(crate) fn linked(core: &Core, session: &SessionId) -> Vec<(PathBuf, PathBuf)> {
    let copies: Vec<LinkedCopy> = {
        let state = core.orchestration_state();
        let guard = state.lock();
        guard
            .tasks
            .iter()
            .flat_map(|t| &t.workers)
            .find(|w| &w.session == session)
            .map(|w| w.repos.clone())
            .unwrap_or_default()
    };
    copies
        .into_iter()
        .filter_map(|c| {
            let root = core.root(&c.project).ok()?;
            Some((PathBuf::from(c.worktree), root.join(".git")))
        })
        .collect()
}

/// Every repository of `session`'s task, the main one first, if it is a
/// worker across repositories; empty otherwise.
pub(super) fn task_repos(core: &Core, session: &SessionId) -> Vec<ProjectId> {
    let state = core.orchestration_state();
    let guard = state.lock();
    guard
        .tasks
        .iter()
        .find_map(|t| {
            let worker = t.workers.iter().find(|w| &w.session == session)?;
            (!worker.repos.is_empty()).then(|| {
                std::iter::once(t.project.clone())
                    .chain(worker.repos.iter().map(|c| c.project.clone()))
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// The workers across repositories that `project` is one of the
/// repositories of, and all of their repositories.
pub(super) fn sessions_with(core: &Core, project: &ProjectId) -> Vec<(SessionId, Vec<ProjectId>)> {
    let state = core.orchestration_state();
    let guard = state.lock();
    guard
        .tasks
        .iter()
        .flat_map(|t| t.workers.iter().map(move |w| (t, w)))
        .filter(|(_, w)| !w.repos.is_empty())
        .filter_map(|(t, w)| {
            let repos: Vec<ProjectId> = std::iter::once(t.project.clone())
                .chain(w.repos.iter().map(|c| c.project.clone()))
                .collect();
            repos.contains(project).then(|| (w.session.clone(), repos))
        })
        .collect()
}

/// The base of each of `worker`'s linked copies' last decision, if every
/// one is settled ([`super::review::settled`]); `None` if any waits.
pub(super) async fn settled(
    core: &Core,
    worker: &crate::protocol::orchestration::Worker,
) -> Option<Vec<(LinkedCopy, String)>> {
    let mut bases = Vec::new();
    for copy in &worker.repos {
        let base = super::review::settled(core, &worker.session, &copy.project).await?;
        bases.push((copy.clone(), base));
    }
    Some(bases)
}

/// Removes the settled linked copies of `worker` of `task` and their
/// branches unless those hold other commits, as its own worktree's. A copy
/// removed is no longer listed. `Err` with git's reason if one stays
/// (`None`: retried later without a reason).
pub(super) async fn remove_settled(
    core: &Core,
    task: &crate::protocol::orchestration::TaskId,
    worker: &crate::protocol::orchestration::Worker,
    copies: Vec<(LinkedCopy, String)>,
) -> std::result::Result<(), Option<String>> {
    for (copy, base) in copies {
        let included = {
            let state = core.orchestration_state();
            let guard = state.lock();
            guard
                .tasks
                .iter()
                .filter(|t| &t.id == task)
                .flat_map(|t| &t.prep)
                .filter(|p| p.session == worker.session)
                .flat_map(|p| &p.linked)
                .find(|k| k.project == copy.project)
                .map(|k| k.included.clone())
                .unwrap_or_default()
        };
        let root = core.root(&copy.project).map_err(|_| None)?;
        match tasks::remove(core, &root, PathBuf::from(&copy.worktree), included).await {
            Ok(()) => {
                let _ = core
                    .worktrees()
                    .delete_branch(root, copy.branch.clone(), base)
                    .await;
            }
            Err(err) if err.code == ErrorCode::Conflict => return Err(Some(err.message)),
            Err(_) => return Err(None),
        }
        let state = core.orchestration_state();
        let mut guard = state.lock();
        if let Some(entry) = guard.tasks.iter_mut().find(|t| &t.id == task) {
            for w in entry
                .workers
                .iter_mut()
                .filter(|w| w.session == worker.session)
            {
                w.repos.retain(|c| c.project != copy.project);
            }
            for p in entry
                .prep
                .iter_mut()
                .filter(|p| p.session == worker.session)
            {
                p.linked.retain(|k| k.project != copy.project);
            }
        }
        state.save(&guard);
    }
    Ok(())
}
