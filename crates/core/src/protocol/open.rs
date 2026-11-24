//! `open.*`: hand things over to other applications.

use serde::{Deserialize, Serialize};

use super::projects::ProjectId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum OpenRequest {
    /// Opens a project file in the user's external editor.
    #[serde(rename = "open.external")]
    External {
        project: ProjectId,
        path: String,
        line: Option<u32>,
    },
}
