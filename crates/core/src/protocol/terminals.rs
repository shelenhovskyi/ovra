//! `terminals.*`: shells in a pseudo-terminal, one per terminal tab.

use serde::{Deserialize, Serialize};

use super::projects::ProjectId;

/// Assigned by the terminal service when a terminal opens.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TerminalId(pub String);

/// Size in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum TerminalsRequest {
    /// Starts the user's shell in the project root.
    #[serde(rename = "terminals.open")]
    Open { project: ProjectId, size: TermSize },
    /// Types into the terminal. Answers `ok`.
    #[serde(rename = "terminals.input")]
    Input { id: TerminalId, data: String },
    /// Answers `ok`.
    #[serde(rename = "terminals.resize")]
    Resize { id: TerminalId, size: TermSize },
    /// Ends the shell. Answers `ok`; `terminals.exited` follows.
    #[serde(rename = "terminals.close")]
    Close { id: TerminalId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum TerminalsResponse {
    #[serde(rename = "terminals.opened")]
    Opened { id: TerminalId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum TerminalsEvent {
    /// Output as UTF-8 text. The service holds back a multi-byte character
    /// split across reads until it is complete.
    #[serde(rename = "terminals.output")]
    Output { id: TerminalId, data: String },
    /// The shell ended. `code` is missing when it was killed by a signal.
    #[serde(rename = "terminals.exited")]
    Exited { id: TerminalId, code: Option<i32> },
}
