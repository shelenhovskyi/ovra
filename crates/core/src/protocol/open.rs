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
    /// Opens a web page in the system browser. Only `http` and `https` URLs
    /// are accepted.
    #[serde(rename = "open.url")]
    Url { url: String },
    /// Shows a project file or directory in the system file manager
    /// (Finder, Explorer, ...); `""` shows the project folder.
    #[serde(rename = "open.reveal")]
    Reveal { project: ProjectId, path: String },
}
