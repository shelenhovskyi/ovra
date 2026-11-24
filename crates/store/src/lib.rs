//! Persistent project list.
//!
//! Status: stub (task 1.B implements it with SQLite in the data directory).
//! Contract: `ovra_core::services::ProjectStore`; the in-memory reference
//! behaviour is `ovra_core::fakes::MemoryProjects`.

use std::path::{Path, PathBuf};

use ovra_core::protocol::projects::{Project, ProjectId};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{ProjectStore, Result};

/// The real project store, kept in `data_dir`.
#[derive(Debug)]
pub struct Store {
    data_dir: PathBuf,
}

impl Store {
    /// Opens (creating if needed) the store in `data_dir`.
    pub fn open(data_dir: &Path) -> Result<Self> {
        Ok(Self {
            data_dir: data_dir.to_path_buf(),
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

fn pending<T>() -> Result<T> {
    Err(ProtocolError::new(
        ErrorCode::Unavailable,
        "the project store is not implemented yet (task 1.B)",
    ))
}

impl ProjectStore for Store {
    fn list(&self) -> Result<Vec<Project>> {
        Ok(Vec::new())
    }

    fn add(&self, _root: &Path) -> Result<Project> {
        pending()
    }

    fn remove(&self, _id: &ProjectId) -> Result<()> {
        pending()
    }

    fn get(&self, _id: &ProjectId) -> Result<Project> {
        pending()
    }

    fn touch(&self, _id: &ProjectId) -> Result<Project> {
        pending()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_lists_nothing() {
        let store = Store::open(Path::new("/tmp/ovra-test")).ok();
        assert_eq!(store.map(|s| s.list()), Some(Ok(Vec::new())));
    }
}
