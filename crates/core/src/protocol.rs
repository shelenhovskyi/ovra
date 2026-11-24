//! Messages exchanged between a front end and the core.
//!
//! The wire format is JSON. Requests carry a client-chosen `id` that the
//! matching response echoes back; events are pushed without an id.

use serde::{Deserialize, Serialize};

/// A request from a front end to the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    /// Ask the core who it is.
    GetInfo,
}

/// The core's answer to a [`Request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Response {
    Info(CoreInfo),
}

/// Static facts about the running core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreInfo {
    pub name: String,
    pub version: String,
    /// How the front end is connected, e.g. `tauri` or `headless`.
    pub mode: String,
}

/// A notification pushed by the core without a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// The core is up and accepting requests.
    Ready,
}

/// An error returned instead of a [`Response`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The message could not be parsed.
    BadRequest,
}
