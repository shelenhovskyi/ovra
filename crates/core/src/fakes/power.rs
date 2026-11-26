use std::sync::Mutex;

use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{KeepAwake, Result};

use super::lock;

/// Records each change of the hold; with `failing`, refuses to hold.
#[derive(Debug, Default)]
pub struct FakeKeepAwake {
    /// Every state it was asked for that differed from the last one.
    pub changes: Mutex<Vec<bool>>,
    awake: Mutex<bool>,
    failing: bool,
}

impl FakeKeepAwake {
    /// One whose system cannot keep the computer awake.
    pub fn failing() -> Self {
        Self {
            failing: true,
            ..Self::default()
        }
    }

    pub fn awake(&self) -> bool {
        *lock(&self.awake)
    }
}

impl KeepAwake for FakeKeepAwake {
    fn set_awake(&self, awake: bool) -> Result<()> {
        if awake && self.failing {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "this system cannot keep the computer awake",
            ));
        }
        let mut now = lock(&self.awake);
        if *now != awake {
            *now = awake;
            lock(&self.changes).push(awake);
        }
        Ok(())
    }
}
