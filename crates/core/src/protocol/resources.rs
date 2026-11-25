//! `resources.*`: how much memory and CPU agent sessions take, and whether
//! the computer is short of either (`docs/specs/agent-resources.md`).

use serde::{Deserialize, Serialize};

use super::agents::SessionId;
use super::projects::ProjectId;

/// What one process tree takes: a root process and every descendant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Resident memory summed over the tree. Shared pages count once per
    /// process, so this is an upper bound.
    pub memory_bytes: u64,
    /// CPU since the previous sample; 100 is one core busy.
    pub cpu_percent: u32,
    /// Processes in the tree.
    pub processes: u32,
}

/// One session's share. A part is `None` while it does not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionResources {
    pub session: SessionId,
    /// The agent's process and every command it runs.
    pub agent: Option<Usage>,
    /// The dev servers of an isolated session's worktree.
    pub servers: Option<Usage>,
    /// The session's browser.
    pub browser: Option<Usage>,
}

/// The whole computer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemLoad {
    pub memory_total_bytes: u64,
    /// Memory that can be given to programs without swapping.
    pub memory_available_bytes: u64,
    /// How busy all cores are together, 0 to 100.
    pub cpu_percent: u32,
}

/// Why queued workers wait although a place may be free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pressure {
    /// Little memory is left.
    LowMemory,
    /// The CPU has been nearly fully busy for a while.
    BusyCpu,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ResourcesEvent {
    /// A sample of a project with sessions that have not ended, every few
    /// seconds while there are any.
    #[serde(rename = "resources.sample")]
    Sample {
        project: ProjectId,
        system: SystemLoad,
        /// While set, queued workers do not start.
        pressure: Option<Pressure>,
        sessions: Vec<SessionResources>,
    },
}
