//! `orchestration.*`: a coordinator agent and the workers it starts
//! (`docs/specs/orchestration.md`).

use serde::{Deserialize, Serialize};

use super::agents::{AgentKind, AgentModel, SessionId};
use super::context::ContextItem;
use super::projects::ProjectId;
use super::review::{ChangeKind, ReviewId};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskLimits {
    /// Workers that may run at once.
    pub max_workers: u32,
    /// Spending cap for all of the task's sessions, in millionths of a dollar.
    pub budget_micro_usd: Option<u64>,
}

impl Default for TaskLimits {
    fn default() -> Self {
        Self {
            max_workers: 3,
            budget_micro_usd: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worker {
    pub session: SessionId,
    /// The label the coordinator gave it.
    pub name: String,
    pub branch: String,
    /// Absolute path of its worktree.
    pub worktree: String,
    /// Why its worktree stays although its task ended (for example files
    /// git ignores, such as `.env`, that only exist there).
    #[serde(default)]
    pub kept: Option<String>,
    /// Its worktree's setup while it runs, and after it failed; `None`
    /// without setup steps and once they succeeded.
    #[serde(default)]
    pub setup: Option<SetupProgress>,
    /// Its worktree's size on disk; `None` until it was counted.
    #[serde(default)]
    pub disk: Option<DiskUsage>,
    /// Why it waits `queued` although a place may be free, in words for
    /// people: the data folder's volume has too little free space for its
    /// worktree, which is not made yet.
    #[serde(default)]
    pub waiting: Option<String>,
    /// Paths (relative to its worktree, `/`-separated) copied into it from
    /// the main checkout through `.worktreeinclude`, shallowest first, at
    /// most 100; files inside a cloned folder count too. The UI names the
    /// secret-looking ones, which the worker's agent can read.
    #[serde(default)]
    pub included: Vec<String>,
    /// Its copies of the task's linked repositories, in the order the
    /// person picked them (`docs/specs/multi-repo.md`); empty for one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repos: Vec<LinkedCopy>,
}

/// A worker's copy of one linked repository (`docs/specs/multi-repo.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedCopy {
    pub project: ProjectId,
    /// Absolute path of its worktree in that repository.
    pub worktree: String,
    pub branch: String,
}

/// Where a worktree's setup steps (`.ovra/worktree.json`) are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupProgress {
    /// The step that runs, or that failed, from 0.
    pub step: u32,
    /// How many steps there are.
    pub steps: u32,
    /// That step's program and arguments, joined with spaces, for people.
    pub command: String,
    /// The last lines of its output (at most 50), without line endings.
    pub log: Vec<String>,
    /// Why it failed or timed out, in words for people; `None` while it runs.
    pub failed: Option<String>,
}

/// The size of a folder on disk, counted by `Worktrees::usage`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskUsage {
    pub bytes: u64,
    /// `true`: every file was counted, and since cloned blocks count in full
    /// the size is an upper bound ("up to"). `false`: counting stopped at its
    /// time limit, so the size is a lower bound ("more than").
    pub complete: bool,
}

/// A folder under Ovra's worktrees folder for a project that no task knows
/// (a crash, an older version): kept until the person removes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leftover {
    /// Absolute path.
    pub path: String,
    /// `None` when it could not be counted.
    pub disk: Option<DiskUsage>,
}

/// A worker the coordinator asked for that could not start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedWorker {
    /// The label the coordinator gave it.
    pub name: String,
    /// Why it did not start, in words for people.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub project: ProjectId,
    /// `None` for one agent in its own copy (`solo`), which has one worker
    /// and no coordinator.
    pub coordinator: Option<SessionId>,
    pub workers: Vec<Worker>,
    /// Workers that never started, so they have no session to show.
    #[serde(default)]
    pub failed: Vec<FailedWorker>,
    pub limits: TaskLimits,
    /// Spent so far by all of its sessions.
    pub spent_micro_usd: u64,
    /// Another attempt at that solo task's prompt
    /// (`docs/specs/attempts.md`): the person keeps the best of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_of: Option<TaskId>,
}

/// What `open_in_ui` asks the UI to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShowTarget {
    File { path: String, line: Option<u32> },
    Agent { session: SessionId },
    Review { id: ReviewId },
}

/// A worker's `push` waiting for the person (`docs/specs/agent-push.md`):
/// the `input` of its permission request, whose `tool` is `Push`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushPlan {
    /// The remote's name, such as `origin`.
    pub remote: String,
    /// The host its URL names, if Ovra could read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub branch: String,
    /// The remote has no such branch yet: it starts from `base`.
    pub new_branch: bool,
    /// What a new branch starts from, on the same remote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// The commit the push builds on: the branch's head, or `base`'s.
    pub onto: String,
    /// The one commit Ovra made of the worker's changes.
    pub commit: String,
    /// Its message, written by the agent.
    pub message: String,
    /// The files the commit changes on top of `onto`.
    pub files: Vec<PushedFile>,
    /// A shared branch such as `main`, or the branch of the item's pull
    /// request: only a rule naming it exactly pushes to it without asking.
    pub protected_branch: bool,
    /// The worker's own branch, `ovra/<task>/<name>`. Any other branch is
    /// pushed to without asking only by a rule naming it exactly.
    #[serde(default)]
    pub own_branch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushedFile {
    pub path: String,
    pub kind: ChangeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum OrchestrationRequest {
    /// Starts a coordinator session on a task, or with `solo` one worker.
    #[serde(rename = "orchestration.start")]
    Start {
        project: ProjectId,
        prompt: String,
        #[serde(default)]
        limits: TaskLimits,
        /// What the person added with `@`; it follows the prompt as Ovra's.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        context: Vec<ContextItem>,
        /// The provider that works on it.
        #[serde(default, skip_serializing_if = "is_default")]
        agent: AgentKind,
        /// One agent in its own worktree, without a coordinator: the task's
        /// only worker gets the prompt (`docs/specs/more-agents.md`).
        #[serde(default, skip_serializing_if = "is_default")]
        solo: bool,
        /// With `solo`: one more attempt at that solo task, the first of
        /// its attempts (`docs/specs/attempts.md`). At most three
        /// tasks try one prompt.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attempt_of: Option<TaskId>,
        /// Claude only: the model its sessions ask for; a coordinator's
        /// workers use their coordinator's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<AgentModel>,
        /// With `solo`: the worker proposes a plan first and changes no
        /// file until the person approves it (`docs/specs/plan-first.md`).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        plan_first: bool,
        /// With `solo`: other projects of the person's list the worker
        /// changes too, each in its own copy, at most three
        /// (`docs/specs/multi-repo.md`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        repos: Vec<ProjectId>,
    },
    /// Tasks of a project, oldest first.
    #[serde(rename = "orchestration.list")]
    List { project: ProjectId },
    /// Stops the coordinator and every worker. Answers `ok`.
    #[serde(rename = "orchestration.stop")]
    Stop { task: TaskId },
    /// The person's Retry (or with `skip_setup`, Start anyway) for a worker
    /// whose worktree setup failed. Answers `ok`.
    #[serde(rename = "orchestration.retry_setup")]
    RetrySetup {
        session: SessionId,
        #[serde(default)]
        skip_setup: bool,
    },
    /// The project's leftover worktrees.
    #[serde(rename = "orchestration.leftovers")]
    Leftovers { project: ProjectId },
    /// Removes one leftover worktree, by its `Leftover::path`; only the
    /// person asks. Answers `ok`.
    #[serde(rename = "orchestration.remove_leftover")]
    RemoveLeftover { project: ProjectId, path: String },
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum OrchestrationResponse {
    #[serde(rename = "orchestration.task")]
    Task(Task),
    #[serde(rename = "orchestration.list")]
    List(Vec<Task>),
    #[serde(rename = "orchestration.leftovers")]
    Leftovers(Vec<Leftover>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum OrchestrationEvent {
    /// A task started, gained or lost a worker, or spent more.
    #[serde(rename = "orchestration.changed")]
    Changed { project: ProjectId, task: TaskId },
    /// An agent asked to show something (`open_in_ui`).
    #[serde(rename = "orchestration.show")]
    Show {
        project: ProjectId,
        target: ShowTarget,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_worker_saved_before_setup_and_disk_reads_without_them() {
        let old = serde_json::json!({
            "session": "s2", "name": "w", "branch": "ovra/t1/w", "worktree": "/wt", "kept": null,
        });
        let worker: Worker = serde_json::from_value(old).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((worker.setup, worker.disk), (None, None));
        let setup = SetupProgress {
            step: 1,
            steps: 2,
            command: "pnpm install".into(),
            log: vec!["ERR_PNPM".into()],
            failed: Some("exited with code 1".into()),
        };
        let disk = DiskUsage {
            bytes: 4096,
            complete: false,
        };
        assert_eq!(worker.waiting, None);
        let worker = Worker {
            setup: Some(setup),
            disk: Some(disk),
            ..worker
        };
        let json = serde_json::to_value(&worker).unwrap_or_default();
        assert_eq!(json["setup"]["failed"], "exited with code 1");
        assert_eq!(
            json["disk"],
            serde_json::json!({"bytes": 4096, "complete": false})
        );
    }

    #[test]
    fn retry_setup_skips_only_when_asked() {
        let parse = |json: serde_json::Value| {
            serde_json::from_value::<OrchestrationRequest>(json).unwrap_or_else(|e| panic!("{e}"))
        };
        let retry = parse(serde_json::json!({
            "method": "orchestration.retry_setup", "params": {"session": "s2"}}));
        assert_eq!(
            retry,
            OrchestrationRequest::RetrySetup {
                session: SessionId("s2".into()),
                skip_setup: false
            }
        );
        let leftovers = OrchestrationResponse::Leftovers(vec![Leftover {
            path: "/data/worktrees/p1/t1/w".into(),
            disk: None,
        }]);
        assert_eq!(
            serde_json::to_value(&leftovers).unwrap_or_default(),
            serde_json::json!({"type": "orchestration.leftovers",
                "data": [{"path": "/data/worktrees/p1/t1/w", "disk": null}]})
        );
    }
}
