//! `git.*`: checks requests from the git panel and hands them to the
//! [`GitService`](crate::services::GitService). The core adds what needs
//! its state: path checks, the discard snapshot (through `Worktrees`),
//! refusing to switch branches or pull under a direct session, and pull and
//! push through the `Forge` (the only way to the remote).

use crate::Core;
use crate::path::RelPath;
use crate::protocol::fs::FsEvent;
use crate::protocol::git::{GitRequest, GitResponse, MAX_LOG};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};
use crate::services::{Ancestry, RemoteBranch};

/// The longest commit message, in bytes.
const MAX_MESSAGE: usize = 64 * 1024;
/// The most paths one request may name.
const MAX_PATHS: usize = 10_000;

pub(crate) async fn handle(core: &Core, request: GitRequest) -> Result<Response, ProtocolError> {
    let git = core.git_service();
    let ok = || Response::Ok(OkResponse::Ok);
    let answer = |r: GitResponse| Response::Git(r);
    Ok(match request {
        GitRequest::Status { project } => answer(match git.status(core.root(&project)?).await? {
            Some(status) => GitResponse::Status(status),
            None => GitResponse::NoRepo,
        }),
        GitRequest::Init { project } => {
            git.init(core.root(&project)?).await?;
            // The fs watcher only follows repositories it saw at the start.
            core.emit(FsEvent::GitChanged { project });
            ok()
        }
        GitRequest::Stage { project, paths } => {
            git.stage(core.root(&project)?, paths_of(&paths)?).await?;
            ok()
        }
        GitRequest::Unstage { project, paths } => {
            git.unstage(core.root(&project)?, paths_of(&paths)?).await?;
            ok()
        }
        GitRequest::StageHunk {
            project,
            path,
            hunk,
            unstage,
        } => {
            hunk.check()?;
            let root = core.root(&project)?;
            git.stage_hunk(root, path_of(&path)?, hunk, unstage).await?;
            ok()
        }
        GitRequest::Discard { project, paths } => {
            let (root, paths) = (core.root(&project)?, paths_of(&paths)?);
            let undo = core.worktrees().snapshot(root.clone()).await?;
            git.discard(root, paths).await?;
            answer(GitResponse::Discarded { undo })
        }
        GitRequest::UndoDiscard {
            project,
            undo,
            paths,
        } => {
            if !is_commit_id(&undo) {
                return Err(bad("not a snapshot id"));
            }
            let (root, paths) = (core.root(&project)?, paths_of(&paths)?);
            core.worktrees().restore(root, undo, paths).await?;
            ok()
        }
        GitRequest::Commit {
            project,
            message,
            amend,
        } => {
            // An empty message keeps the last one when amending.
            if (message.trim().is_empty() && !amend) || message.len() > MAX_MESSAGE {
                return Err(bad(format!(
                    "the commit message must have 1 to {MAX_MESSAGE} bytes"
                )));
            }
            let committed = git.commit(core.root(&project)?, message, amend).await?;
            answer(GitResponse::Committed(committed))
        }
        GitRequest::Branches { project } => answer(GitResponse::Branches {
            branches: git.branches(core.root(&project)?).await?,
        }),
        GitRequest::Switch { project, branch } => {
            check_branch_name(&branch)?;
            if core.sessions().direct_writer(&project, None) {
                return Err(ProtocolError::new(
                    ErrorCode::Conflict,
                    "an agent session is changing this project's files; \
                     switch branches when it has ended",
                ));
            }
            git.switch(core.root(&project)?, branch).await?;
            ok()
        }
        GitRequest::Cancel { project } => {
            git.cancel(core.root(&project)?).await?;
            ok()
        }
        GitRequest::CreateBranch {
            project,
            name,
            switch,
        } => {
            check_branch_name(&name)?;
            git.create_branch(core.root(&project)?, name, switch)
                .await?;
            ok()
        }
        GitRequest::Diff {
            project,
            path,
            staged,
        } => answer(GitResponse::Diff(
            git.diff(core.root(&project)?, path_of(&path)?, staged)
                .await?,
        )),
        GitRequest::Show { project, path, rev } => answer(GitResponse::File(
            git.show(core.root(&project)?, path_of(&path)?, rev).await?,
        )),
        GitRequest::Blame { project, path } => answer(GitResponse::Blame(
            git.blame(core.root(&project)?, path_of(&path)?).await?,
        )),
        GitRequest::Log { project, limit } => answer(GitResponse::Log {
            commits: git
                .log(core.root(&project)?, limit.clamp(1, MAX_LOG))
                .await?,
        }),
        GitRequest::Pulled { project } => {
            answer(GitResponse::Pulled(git.pulled(core.root(&project)?).await?))
        }
        GitRequest::Pull { project } => {
            refuse_under_direct_session(core, &project)?;
            let root = core.root(&project)?;
            let (expect, _) = upstream(core, &root).await?;
            let RemoteBranch { remote, branch } = expect.clone();
            let commit = core
                .forge()
                .fetch(root.clone(), remote.clone(), branch.clone())
                .await?;
            match git
                .record_upstream(root.clone(), expect.clone(), commit.clone())
                .await?
            {
                Ancestry::Has => {}
                Ancestry::Diverged => return Err(diverged(&remote, &branch)),
                Ancestry::Behind => {
                    // The fetch may have taken a while: a session may have started.
                    refuse_under_direct_session(core, &project)?;
                    git.fast_forward(root, expect, commit).await?;
                }
            }
            ok()
        }
        GitRequest::Push { project } => {
            let root = core.root(&project)?;
            let (expect, head) = upstream(core, &root).await?;
            let RemoteBranch { remote, branch } = expect.clone();
            let forge = core.forge();
            // Where the remote branch is now: the push goes only on top of it.
            let there = forge
                .fetch(root.clone(), remote.clone(), branch.clone())
                .await?;
            let ancestry = git.record_upstream(root.clone(), expect.clone(), there.clone());
            if ancestry.await? != Ancestry::Has {
                return Err(ProtocolError::new(
                    ErrorCode::Conflict,
                    format!(
                        "{remote}/{branch} has new commits that this branch lacks: \
                         get them first, then send"
                    ),
                ));
            }
            let pushed = forge
                .push(
                    root.clone(),
                    remote.clone(),
                    head.clone(),
                    branch.clone(),
                    Some(there),
                )
                .await;
            if let Err(e) = pushed {
                return Err(match e.code {
                    ErrorCode::Conflict => ProtocolError::new(
                        ErrorCode::Conflict,
                        format!(
                            "{remote}/{branch} changed while sending: get its new commits, \
                             then send again"
                        ),
                    ),
                    _ => e,
                });
            }
            git.record_upstream(root, expect, head).await?;
            ok()
        }
    })
}

fn refuse_under_direct_session(core: &Core, project: &ProjectId) -> Result<(), ProtocolError> {
    if core.sessions().direct_writer(project, None) {
        return Err(ProtocolError::new(
            ErrorCode::Conflict,
            "an agent session is changing this project's files; \
             get new commits when it has ended",
        ));
    }
    Ok(())
}

fn diverged(remote: &str, branch: &str) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Conflict,
        format!(
            "this branch and {remote}/{branch} both have new commits: \
             merge or rebase them in a terminal"
        ),
    )
}

/// The current branch's upstream on its remote, and HEAD's commit.
async fn upstream(
    core: &Core,
    root: &std::path::Path,
) -> Result<(RemoteBranch, String), ProtocolError> {
    let git = core.git_service();
    let not_connected = || {
        bad("this branch is not connected to a branch on a remote; \
             send it once in a terminal (git push -u)")
    };
    let upstream = git
        .upstream(root.to_path_buf())
        .await?
        .ok_or_else(not_connected)?;
    let status = git.status(root.to_path_buf()).await?;
    let head = status.and_then(|s| s.head).ok_or_else(not_connected)?;
    Ok((upstream, head))
}

fn bad(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, message)
}

/// A file or folder in the project, never the root itself, anything in a
/// `.git` folder, or a name with control characters.
fn path_of(raw: &str) -> Result<RelPath, ProtocolError> {
    let path = RelPath::parse(raw)?;
    if path.is_root() {
        return Err(bad("name a file or folder, not the project root"));
    }
    if raw.chars().any(char::is_control) {
        return Err(bad(format!("path {raw:?} contains a control character")));
    }
    // Also a nested repository's `.git`, such as `vendor/lib/.git/config`.
    if path
        .as_str()
        .split('/')
        .any(|s| s.eq_ignore_ascii_case(".git"))
    {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            format!("path {raw:?} is inside .git"),
        ));
    }
    Ok(path)
}

fn paths_of(raw: &[String]) -> Result<Vec<RelPath>, ProtocolError> {
    if raw.is_empty() || raw.len() > MAX_PATHS {
        return Err(bad(format!("name 1 to {MAX_PATHS} paths")));
    }
    raw.iter().map(|p| path_of(p)).collect()
}

/// Rejects what git would read as an option or a revision expression;
/// the service asks git for the rest (`check-ref-format --branch`).
fn check_branch_name(name: &str) -> Result<(), ProtocolError> {
    if name.is_empty() || name.len() > 255 || name.starts_with('-') || name.contains(['\0', '\n']) {
        return Err(bad(format!("{name:?} is not a valid branch name")));
    }
    Ok(())
}

fn is_commit_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests;
