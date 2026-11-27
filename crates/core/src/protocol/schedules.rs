//! `schedules.*`: prompts that start an agent at days and times the person
//! set, while Ovra runs (ADR 0015, `docs/specs/schedules.md`). Off until the
//! person turns them on.

use serde::{Deserialize, Serialize};

use super::agents::{AgentKind, SessionId};
use super::projects::ProjectId;

/// `sc1`, `sc2`, ... in the order schedules were made.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScheduleId(pub String);

/// What a run starts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleKind {
    /// A research session: reads and reports, changes nothing (ADR 0013).
    #[default]
    Research,
    /// One agent in its own worktree; its changes wait in Reviews.
    OwnCopy,
}

/// When a schedule runs: on these days, at this local time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleTime {
    /// ISO weekdays, 1 (Monday) to 7 (Sunday); at least one.
    pub days: Vec<u8>,
    /// The time of day, 24-hour `HH:MM`.
    pub time: String,
    /// The IANA time zone `time` is in, such as `Europe/Kyiv`: the one the
    /// person's computer had when they saved it.
    pub time_zone: String,
}

/// What the person fills in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleDraft {
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub kind: ScheduleKind,
    #[serde(default)]
    pub agent: AgentKind,
    pub when: ScheduleTime,
}

/// How a run went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RunOutcome {
    Started {
        session: SessionId,
    },
    /// Not started, in words for people (the last run still works, its
    /// changes still wait in Reviews, the research places stayed full).
    Skipped {
        reason: String,
    },
    /// Its time passed more than an hour before Ovra could start it (Ovra
    /// was not running, or the computer slept).
    Missed,
    /// Starting failed, in words for people.
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastRun {
    /// When it ran or was given up, in ms since the Unix epoch.
    pub at_ms: u64,
    #[serde(flatten)]
    pub outcome: RunOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    pub id: ScheduleId,
    pub project: ProjectId,
    #[serde(flatten)]
    pub draft: ScheduleDraft,
    #[serde(default)]
    pub paused: bool,
    /// The next run, in ms since the Unix epoch; `None` while paused or
    /// while scheduled tasks are off.
    #[serde(default)]
    pub next_ms: Option<u64>,
    #[serde(default)]
    pub last: Option<LastRun>,
    /// The session of the latest run that started, kept when later runs
    /// are skipped.
    #[serde(default)]
    pub last_session: Option<SessionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum SchedulesRequest {
    /// Whether scheduled tasks are on (off by default).
    #[serde(rename = "schedules.enabled")]
    Enabled,
    /// Turns them on or off; schedules stay either way. Answers
    /// `schedules.enabled`.
    #[serde(rename = "schedules.set_enabled")]
    SetEnabled { enabled: bool },
    /// The project's schedules, oldest first.
    #[serde(rename = "schedules.list")]
    List { project: ProjectId },
    /// Makes a schedule, or with `id` replaces that one's draft. Answers
    /// `schedules.schedule`.
    #[serde(rename = "schedules.save")]
    Save {
        project: ProjectId,
        #[serde(default)]
        id: Option<ScheduleId>,
        draft: ScheduleDraft,
    },
    /// Answers `schedules.schedule`.
    #[serde(rename = "schedules.set_paused")]
    SetPaused { id: ScheduleId, paused: bool },
    /// Answers `ok`.
    #[serde(rename = "schedules.remove")]
    Remove { id: ScheduleId },
    /// The person starts a run now, even while paused or switched off; the
    /// skip rules still hold. Answers `schedules.schedule` with the run in
    /// `last`.
    #[serde(rename = "schedules.run_now")]
    RunNow { id: ScheduleId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SchedulesResponse {
    #[serde(rename = "schedules.enabled")]
    Enabled { enabled: bool },
    #[serde(rename = "schedules.list")]
    List(Vec<Schedule>),
    #[serde(rename = "schedules.schedule")]
    Schedule(Box<Schedule>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SchedulesEvent {
    /// A schedule was saved, removed or ran (`project`), or scheduled tasks
    /// were turned on or off (`project: null`).
    #[serde(rename = "schedules.changed")]
    Changed { project: Option<ProjectId> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_schedule_reads_and_writes_flat() {
        let json = serde_json::json!({
            "id": "sc1", "project": "p1", "name": "Bugs", "prompt": "Find bugs",
            "kind": "own_copy", "agent": "claude",
            "when": {"days": [1, 3], "time": "03:00", "time_zone": "Europe/Kyiv"},
            "paused": false, "next_ms": null,
            "last": {"at_ms": 5, "outcome": "skipped", "reason": "busy"},
            "last_session": "s1",
        });
        let schedule: Schedule =
            serde_json::from_value(json.clone()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(schedule.draft.kind, ScheduleKind::OwnCopy);
        assert_eq!(serde_json::to_value(&schedule).unwrap_or_default(), json);
    }
}
