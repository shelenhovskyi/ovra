//! File access for projects: directory listings that honour `.gitignore`,
//! git status, reading and writing files, and watching for changes.
//!
//! Contract: `ovra_core::services::FsService`; the in-memory reference
//! behaviour is `ovra_core::fakes::MemoryFs`. Paths arrive validated, and
//! symlinks are not followed outside the root. Blocking work runs on tokio's
//! blocking pool.

mod files;
mod git;
mod listing;
mod resolve;
mod watch;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ovra_core::path::RelPath;
use ovra_core::protocol::fs::{DirListing, FileContent};
use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, EventSink, FsService, Result};

use crate::watch::{GitCache, Timing, Watch};

/// The real file service.
#[derive(Debug, Default)]
pub struct FsIndex {
    watches: Mutex<HashMap<ProjectId, Watch>>,
    timing: Timing,
}

impl FsIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shorter delays, so watcher tests finish quickly.
    #[cfg(test)]
    fn fast() -> Self {
        Self {
            timing: Timing {
                git_quiet: std::time::Duration::from_millis(100),
                ..Timing::default()
            },
            ..Self::default()
        }
    }

    fn watches(&self) -> MutexGuard<'_, HashMap<ProjectId, Watch>> {
        self.watches.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The watcher-maintained git status for `root`, if it is watched.
    fn git_cache(&self, root: &PathBuf) -> Option<Arc<GitCache>> {
        self.watches()
            .values()
            .find(|w| &w.given_root == root)
            .map(|w| Arc::clone(&w.git))
    }
}

fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> BoxFuture<'static, Result<T>> {
    Box::pin(async move {
        tokio::task::spawn_blocking(work)
            .await
            .unwrap_or_else(|e| Err(ProtocolError::new(ErrorCode::Internal, e.to_string())))
    })
}

impl FsService for FsIndex {
    fn list_dir(&self, root: PathBuf, path: RelPath) -> BoxFuture<'_, Result<DirListing>> {
        let cache = self.git_cache(&root);
        blocking(move || {
            let status = match cache {
                Some(cache) => cache.current(),
                None => git::Repo::discover(&resolve::root(&root)?).and_then(|r| r.status()),
            };
            listing::list(&root, &path, status.as_ref())
        })
    }

    fn read_file(&self, root: PathBuf, path: RelPath) -> BoxFuture<'_, Result<FileContent>> {
        blocking(move || files::read(&root, &path))
    }

    fn write_file(
        &self,
        root: PathBuf,
        path: RelPath,
        text: String,
        expected_mtime_ms: Option<u64>,
    ) -> BoxFuture<'_, Result<u64>> {
        blocking(move || files::write(&root, &path, &text, expected_mtime_ms))
    }

    fn watch(&self, project: ProjectId, root: PathBuf, sink: EventSink) -> Result<()> {
        let watch = Watch::start(project.clone(), root, sink, self.timing)?;
        self.watches().insert(project, watch);
        Ok(())
    }

    fn unwatch(&self, project: &ProjectId) {
        self.watches().remove(project);
    }
}

#[cfg(test)]
mod tests;
