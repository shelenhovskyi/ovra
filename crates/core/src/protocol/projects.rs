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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ProjectsResponse {
    #[serde(rename = "projects.list")]
    List(Vec<Project>),
    #[serde(rename = "projects.project")]
    Project(Project),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ProjectsEvent {
    /// The list or the active project changed; clients should re-fetch.
    #[serde(rename = "projects.changed")]
    Changed { active: Option<ProjectId> },
}
