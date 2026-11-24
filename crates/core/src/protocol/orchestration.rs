//! `orchestration.*`: a coordinator agent and the workers it starts
//! (`docs/specs/orchestration.md`).

use serde::{Deserialize, Serialize};

use super::agents::SessionId;
use super::projects::ProjectId;
use super::review::ReviewId;

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub project: ProjectId,
    pub coordinator: SessionId,
    pub workers: Vec<Worker>,
    pub limits: TaskLimits,
    /// Spent so far by all of its sessions.
    pub spent_micro_usd: u64,
}

/// What `open_in_ui` asks the UI to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShowTarget {
    File { path: String, line: Option<u32> },
    Agent { session: SessionId },
    Review { id: ReviewId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum OrchestrationRequest {
    /// Starts a coordinator session on a task.
    #[serde(rename = "orchestration.start")]
    Start {
        project: ProjectId,
        prompt: String,
        #[serde(default)]
        limits: TaskLimits,
    },
    /// Tasks of a project, oldest first.
    #[serde(rename = "orchestration.list")]
    List { project: ProjectId },
    /// Stops the coordinator and every worker. Answers `ok`.
    #[serde(rename = "orchestration.stop")]
    Stop { task: TaskId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum OrchestrationResponse {
    #[serde(rename = "orchestration.task")]
    Task(Task),
    #[serde(rename = "orchestration.list")]
    List(Vec<Task>),
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
