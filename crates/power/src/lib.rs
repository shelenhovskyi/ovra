//! Keeps the computer from sleeping while it is idle, through the OS's own
//! hold: an IOKit `PreventUserIdleSystemSleep` assertion on macOS,
//! `SetThreadExecutionState(ES_SYSTEM_REQUIRED)` on Windows, and a logind
//! `idle` inhibitor on Linux. The display may still sleep, and a person
//! who puts the computer to sleep still can. Check a hold with
//! `pmset -g assertions`, `powercfg /requests` or `systemd-inhibit --list`.

use std::fmt;
use std::sync::{Mutex, PoisonError};

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{KeepAwake, Result};

/// The real [`KeepAwake`]. The hold ends when this is dropped or the
/// process ends.
#[derive(Default)]
pub struct SystemKeepAwake {
    hold: Mutex<Option<keepawake::KeepAwake>>,
}

impl fmt::Debug for SystemKeepAwake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let held = self
            .hold
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some();
        f.debug_struct("SystemKeepAwake")
            .field("held", &held)
            .finish()
    }
}

impl KeepAwake for SystemKeepAwake {
    fn set_awake(&self, awake: bool) -> Result<()> {
        let mut hold = self.hold.lock().unwrap_or_else(PoisonError::into_inner);
        if !awake {
            *hold = None;
            return Ok(());
        }
        if hold.is_none() {
            let created = keepawake::Builder::default()
                .idle(true)
                .reason("Agents are working")
                .app_name("Ovra")
                .app_reverse_domain("dev.ovra.Ovra")
                .create()
                .map_err(|e| {
                    ProtocolError::new(
                        ErrorCode::Unavailable,
                        format!("this system cannot keep the computer awake: {e}"),
                    )
                })?;
            *hold = Some(created);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Holding needs the OS (and on Linux a system bus), so tests only
    // check what needs none: releasing without a hold.
    #[test]
    fn releasing_without_a_hold_does_nothing() {
        let keep = SystemKeepAwake::default();
        assert!(keep.set_awake(false).is_ok());
        assert_eq!(format!("{keep:?}"), "SystemKeepAwake { held: false }");
    }
}
