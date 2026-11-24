//! Opens files in other applications.
//!
//! Status: stub (task 1.G implements it per OS: `$VISUAL`/`$EDITOR`, then the
//! platform default handler). Contract: `ovra_core::services::Opener`.

use std::path::Path;

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{Opener, Result};

/// The real opener.
#[derive(Debug, Default)]
pub struct SystemOpener {}

impl SystemOpener {
    pub fn new() -> Self {
        Self {}
    }
}

impl Opener for SystemOpener {
    fn open_in_editor(&self, _file: &Path, _line: Option<u32>) -> Result<()> {
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "opening files is not implemented yet (task 1.G)",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_reports_unavailable() {
        let r = SystemOpener::new().open_in_editor(Path::new("/x"), None);
        assert_eq!(r.err().map(|e| e.code), Some(ErrorCode::Unavailable));
    }
}
