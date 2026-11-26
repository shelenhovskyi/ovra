//! `context.*`: what the person adds to a task or reply with `@`
//! (`docs/specs/task-context.md`). Paths are referenced, never read; text
//! items carry the text the person chose.

use serde::{Deserialize, Serialize};

use super::agents::SessionId;
use super::browser::PageId;
use super::projects::ProjectId;

/// One thing the person added, and whether they chose to include it
/// although it is sensitive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextItem {
    #[serde(flatten)]
    pub what: ContextWhat,
    /// The person chose Include on its warning.
    #[serde(default)]
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextWhat {
    File {
        path: String,
    },
    Folder {
        path: String,
    },
    /// Lines `from_line` to `to_line` (1-based) of `path`, as the editor
    /// shows them, unsaved changes included.
    Selection {
        path: String,
        from_line: u32,
        to_line: u32,
        text: String,
    },
    /// A language server's problem at `line` (1-based).
    Problem {
        path: String,
        line: u32,
        severity: String,
        message: String,
        #[serde(default)]
        source: Option<String>,
    },
    /// A terminal's selection or last lines.
    Terminal {
        title: String,
        text: String,
    },
    /// A page of the session's own browser: the agent takes its screenshot.
    Page {
        page: PageId,
    },
    /// A GitHub issue as `pr.issue` read it.
    Issue {
        number: u64,
        title: String,
        url: String,
        text: String,
    },
    /// The end of a failed CI check's log as `pr.check_log` read it.
    Check {
        /// The pull request's number.
        pr: u64,
        name: String,
        #[serde(default)]
        url: Option<String>,
        text: String,
    },
    /// A PNG, JPEG, GIF or WebP file of the project, such as a pasted
    /// screenshot: an agent that can see images gets the image itself.
    Image {
        path: String,
    },
}

/// Why an item needs the person's Include before it is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextWarning {
    /// A protected path: agent configuration the sandbox keeps read-only.
    Protected,
    /// A file name that usually holds secrets (`.env`, keys, credentials).
    SecretFile,
    /// The text holds something that looks like a token or a secret.
    SecretText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum ContextRequest {
    /// Checks items as they are added. `forbidden` for a path outside the
    /// project, `bad_request` for an item too large. Answers
    /// `context.checked`.
    #[serde(rename = "context.check")]
    Check {
        project: ProjectId,
        /// The task a reply goes to; a page item needs it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<SessionId>,
        items: Vec<ContextItem>,
    },
    /// Saves a file the person pasted or dropped into a message box as a
    /// new file of the project's attachments folder (`.ovra-attachments/`,
    /// which git ignores), named `name` or a name like it, so a chip can
    /// name it. `data` is the file in standard base64, 10 MiB at most.
    /// Answers `context.attached` with its path.
    #[serde(rename = "context.attach")]
    Attach {
        project: ProjectId,
        name: String,
        data: String,
    },
    /// Makes the attachments folder, for the files the desktop app copies
    /// there from a drop. Answers `context.attached` with its path.
    #[serde(rename = "context.attachments")]
    Attachments { project: ProjectId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ContextResponse {
    /// One entry per item, in order: `None` when it can go as it is.
    #[serde(rename = "context.checked")]
    Checked { notes: Vec<Option<ContextWarning>> },
    /// The path of the saved file or of the attachments folder.
    #[serde(rename = "context.attached")]
    Attached { path: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Request, Response};

    #[test]
    fn wire_format_is_stable() {
        let item = |what| ContextItem {
            what,
            confirmed: false,
        };
        insta::assert_json_snapshot!(serde_json::json!({
            "requests": [Request::Context(ContextRequest::Check {
                project: ProjectId("p1".into()),
                session: Some(SessionId("s1".into())),
                items: vec![
                    item(ContextWhat::File { path: "src/lib.rs".into() }),
                    item(ContextWhat::Folder { path: "src".into() }),
                    ContextItem {
                        what: ContextWhat::Selection {
                            path: ".env".into(),
                            from_line: 1,
                            to_line: 2,
                            text: "A=1".into(),
                        },
                        confirmed: true,
                    },
                    item(ContextWhat::Problem {
                        path: "src/lib.rs".into(),
                        line: 3,
                        severity: "error".into(),
                        message: "unused".into(),
                        source: Some("rustc".into()),
                    }),
                    item(ContextWhat::Terminal { title: "zsh".into(), text: "ok".into() }),
                    item(ContextWhat::Page { page: PageId("g1".into()) }),
                    item(ContextWhat::Issue {
                        number: 12,
                        title: "Login fails".into(),
                        url: "https://github.com/o/r/issues/12".into(),
                        text: "Steps".into(),
                    }),
                    item(ContextWhat::Check {
                        pr: 7,
                        name: "test".into(),
                        url: None,
                        text: "1 failed".into(),
                    }),
                    item(ContextWhat::Image { path: ".ovra-attachments/shot.png".into() }),
                ],
            }), Request::Context(ContextRequest::Attach {
                project: ProjectId("p1".into()),
                name: "shot.png".into(),
                data: "iVBORw0K".into(),
            }), Request::Context(ContextRequest::Attachments {
                project: ProjectId("p1".into()),
            })],
            "responses": [Response::Context(ContextResponse::Checked {
                notes: vec![None, Some(ContextWarning::SecretFile)],
            }), Response::Context(ContextResponse::Attached {
                path: ".ovra-attachments/shot.png".into(),
            })],
        }));
        let parsed: ContextItem =
            serde_json::from_value(serde_json::json!({"kind": "file", "path": "a"}))
                .unwrap_or_else(|e| panic!("{e}"));
        assert!(!parsed.confirmed);
    }
}
