//! File access for projects: directory listings that honour `.gitignore`,
//! git status, reading and writing files, and watching for changes.
//!
//! Status: stub (task 1.A implements it). Contract: `ovra_core::services::FsService`;
//! the in-memory reference behaviour is `ovra_core::fakes::MemoryFs`.
//! Paths arrive validated, but symlinks must not be followed outside the root.

use std::path::PathBuf;

use ovra_core::path::RelPath;
use ovra_core::protocol::fs::{DirListing, FileContent};
use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, EventSink, FsService, Result};

/// The real file service.
#[derive(Debug, Default)]
pub struct FsIndex {}

impl FsIndex {
    pub fn new() -> Self {
        Self {}
    }
}

fn pending<T>() -> Result<T> {
    Err(ProtocolError::new(
        ErrorCode::Unavailable,
        "file access is not implemented yet (task 1.A)",
    ))
}

impl FsService for FsIndex {
    fn list_dir(&self, _root: PathBuf, _path: RelPath) -> BoxFuture<'_, Result<DirListing>> {
        Box::pin(async { pending() })
    }

    fn read_file(&self, _root: PathBuf, _path: RelPath) -> BoxFuture<'_, Result<FileContent>> {
        Box::pin(async { pending() })
    }

    fn write_file(
        &self,
        _root: PathBuf,
        _path: RelPath,
        _text: String,
        _expected_mtime_ms: Option<u64>,
    ) -> BoxFuture<'_, Result<u64>> {
        Box::pin(async { pending() })
    }

    fn watch(&self, _project: ProjectId, _root: PathBuf, _sink: EventSink) -> Result<()> {
        Ok(())
    }

    fn unwatch(&self, _project: &ProjectId) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_reports_unavailable() {
        let err = poll_ready(FsIndex::new().list_dir(PathBuf::from("/"), RelPath::root()));
        assert_eq!(err.err().map(|e| e.code), Some(ErrorCode::Unavailable));
    }

    /// Polls a future that is ready immediately.
    fn poll_ready<T>(fut: BoxFuture<'_, T>) -> T {
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        let mut fut = fut;
        match fut.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(v) => v,
            std::task::Poll::Pending => panic!("stub future should be ready"),
        }
    }
}
