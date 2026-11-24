//! `projects.*`: the list of projects and the active one.

use serde::{Deserialize, Serialize};

/// Stable project id assigned by the project store.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    /// Display name, by default the folder name.
    pub name: String,
    /// Absolute path of the project root.
    pub root: String,
    /// Unix time in milliseconds when the project was last opened.
    pub last_opened_ms: Option<u64>,
}

/// What a project looked like when it was last left, restored when it is
/// opened again. Paths are relative to the project root.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectState {
    /// Open tabs, in tab order.
    pub open_files: Vec<String>,
    /// The focused tab, if any.
    pub active_file: Option<String>,
    /// Expanded folders in the file tree.
    pub expanded_dirs: Vec<String>,
}

/// A partial [`ProjectState`]: only the fields present are written, so panels
/// can save their part without overwriting each other's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectStatePatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_files: Option<Vec<String>>,
    /// `null` clears the active file; leaving the field out keeps it.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub active_file: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_dirs: Option<Vec<String>>,
}

impl ProjectStatePatch {
    /// Writes the present fields into `state`.
    pub fn apply(self, state: &mut ProjectState) {
        if let Some(open_files) = self.open_files {
            state.open_files = open_files;
        }
        if let Some(active_file) = self.active_file {
            state.active_file = active_file;
        }
        if let Some(expanded_dirs) = self.expanded_dirs {
            state.expanded_dirs = expanded_dirs;
        }
    }
}

/// Tells a present field (even `null`) from a missing one.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum ProjectsRequest {
    /// All projects, most recently opened first.
    #[serde(rename = "projects.list")]
    List,
    /// Registers a folder as a project; returns the existing one if already added.
    #[serde(rename = "projects.add")]
    Add { root: String },
    /// Forgets a project. Files on disk are untouched.
    #[serde(rename = "projects.remove")]
    Remove { id: ProjectId },
    /// Makes a project the active one and starts watching it.
    #[serde(rename = "projects.open")]
    Open { id: ProjectId },
    /// The saved state of a project; empty if none was saved.
    #[serde(rename = "projects.get_state")]
    GetState { id: ProjectId },
    /// Writes the fields present in `state`, keeping the others. Answers `ok`.
    #[serde(rename = "projects.set_state")]
    SetState {
        id: ProjectId,
        state: ProjectStatePatch,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ProjectsResponse {
    #[serde(rename = "projects.list")]
    List(Vec<Project>),
    #[serde(rename = "projects.project")]
    Project(Project),
    #[serde(rename = "projects.state")]
    State(ProjectState),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ProjectsEvent {
    /// The list or the active project changed; clients should re-fetch.
    #[serde(rename = "projects.changed")]
    Changed { active: Option<ProjectId> },
}
