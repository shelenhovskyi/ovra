//! `info.*`: who the core is.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum InfoRequest {
    #[serde(rename = "info.get")]
    Get,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum InfoResponse {
    #[serde(rename = "info")]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum InfoEvent {
    /// The core is up and accepting requests.
    #[serde(rename = "info.ready")]
    Ready,
}
