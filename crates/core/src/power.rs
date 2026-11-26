//! Keeping the computer awake (`power.*`): the person picks a mode, the core
//! holds or releases the [`KeepAwake`] hold as sessions start and end.

use std::sync::{Arc, Mutex, PoisonError};

use crate::Core;
use crate::protocol::power::{KeepAwakeMode, PowerEvent, PowerRequest, PowerResponse, PowerState};
use crate::protocol::{ProtocolError, Response};
use crate::services::KeepAwake;

#[derive(Debug)]
pub(crate) struct State {
    service: Arc<dyn KeepAwake>,
    now: Mutex<PowerState>,
}

impl State {
    pub(crate) fn new(service: Arc<dyn KeepAwake>) -> Self {
        Self {
            service,
            now: Mutex::new(PowerState {
                mode: KeepAwakeMode::Off,
                awake: false,
                problem: None,
            }),
        }
    }
}

pub(crate) fn handle(core: &Core, request: PowerRequest) -> Result<Response, ProtocolError> {
    let state = match request {
        PowerRequest::Get => core
            .power()
            .now
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
        PowerRequest::SetMode { mode } => update(core, Some(mode)),
    };
    Ok(Response::Power(PowerResponse::State(state)))
}

/// Holds or releases the hold after sessions changed state.
pub(crate) fn sessions_changed(core: &Core) {
    update(core, None);
}

/// Applies `mode` (or keeps the current one), holds or releases the hold
/// to match, and says so when anything changed.
fn update(core: &Core, mode: Option<KeepAwakeMode>) -> PowerState {
    let power = core.power();
    // Sessions are read under this lock so two updates cannot apply their
    // answers out of order; nothing holds the sessions' lock while it
    // takes this one.
    let mut now = power.now.lock().unwrap_or_else(PoisonError::into_inner);
    let before = now.clone();
    if let Some(mode) = mode {
        now.mode = mode;
    }
    let want = match now.mode {
        KeepAwakeMode::Off => false,
        KeepAwakeMode::Always => true,
        KeepAwakeMode::WhileAgentsWork => core.sessions().any_working(),
    };
    match power.service.set_awake(want) {
        Ok(()) => {
            now.awake = want;
            now.problem = None;
        }
        Err(e) => {
            now.awake = false;
            now.problem = Some(e.message);
        }
    }
    let state = now.clone();
    drop(now);
    if state != before {
        core.emit(PowerEvent::Changed(state.clone()));
    }
    state
}

#[cfg(test)]
mod tests;
