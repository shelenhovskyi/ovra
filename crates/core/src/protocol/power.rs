//! `power.*`: keeping the computer awake while agents work.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum PowerRequest {
    #[serde(rename = "power.get")]
    Get,
    /// Changes the mode for this run of Ovra; every start is `off`.
    #[serde(rename = "power.set_mode")]
    SetMode { mode: KeepAwakeMode },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum PowerResponse {
    #[serde(rename = "power.state")]
    State(PowerState),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum PowerEvent {
    /// The mode changed, or the computer started or stopped being kept awake.
    #[serde(rename = "power.changed")]
    Changed(PowerState),
}

/// When Ovra keeps the computer from sleeping while idle. The display may
/// still sleep; closing a laptop's lid may still put it to sleep.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeepAwakeMode {
    #[default]
    Off,
    /// While any agent is queued, starting, working or waiting for a person.
    WhileAgentsWork,
    /// Until turned off or Ovra quits.
    Always,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerState {
    pub mode: KeepAwakeMode,
    /// Whether Ovra keeps the computer awake right now.
    pub awake: bool,
    /// Why the computer cannot be kept awake, e.g. the system has no way
    /// to ask for it. The mode stays as chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Event, Request, Response};

    #[test]
    fn wire_format_is_stable() {
        let state = PowerState {
            mode: KeepAwakeMode::WhileAgentsWork,
            awake: true,
            problem: None,
        };
        insta::assert_json_snapshot!(serde_json::json!({
            "requests": [
                Request::Power(PowerRequest::Get),
                Request::Power(PowerRequest::SetMode { mode: KeepAwakeMode::Always }),
            ],
            "responses": [Response::Power(PowerResponse::State(state.clone()))],
            "events": [Event::from(PowerEvent::Changed(PowerState {
                mode: KeepAwakeMode::Off,
                awake: false,
                problem: Some("no logind".into()),
            }))],
        }));
    }
}
