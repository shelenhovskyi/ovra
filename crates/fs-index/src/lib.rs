//! File access for projects: directory listings that honour `.gitignore`,
//! git status, reading and writing files, and watching for changes.
//!
//! Contract: `ovra_core::services::FsService`; the in-memory reference
//! behaviour is `ovra_core::fakes::MemoryFs`. Paths arrive validated, and
//! symlinks are not followed outside the root. Blocking work runs on tokio's
//! blocking pool.

mod file_list;
mod files;
mod git;
mod listing;
mod resolve;
mod search;
mod watch;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ovra_core::path::RelPath;
use ovra_core::protocol::fs::{DirListing, FileContent, FileList};
use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::search::{SearchId, SearchQuery};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, EventSink, FsService, Result, SearchCancel};

use crate::watch::{SharedFiles, Timing, Watch};

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

    /// Something from the watch on `root`, if it is watched.
    fn watched<T>(&self, root: &PathBuf, get: impl FnOnce(&Watch) -> T) -> Option<T> {
        self.watches()
            .values()
            .find(|w| &w.given_root == root)
            .map(get)
    }
}

fn not_yet() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        "file operations are not available yet",
    )
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
        if path.is_root() {
            self.watched(&root, Watch::remind_unwatched);
        }
        let watched = self.watched(&root, |w| (Arc::clone(&w.git), Arc::clone(&w.dirs)));
        blocking(move || {
            let status = match watched {
                Some((cache, dirs)) => {
                    // A listed directory is one the user looks at: watch it
                    // now rather than when the background walk gets there.
                    if let Ok((_, dir)) = resolve::existing(&root, &path)
                        && dir.is_dir()
                    {
                        dirs.add(&dir);
                    }
                    cache.current()
                }
                None => git::Repo::discover(&resolve::root(&root)?).and_then(|r| r.status()),
            };
            listing::list(&root, &path, status.as_ref())
        })
    }

    fn read_file(&self, root: PathBuf, path: RelPath) -> BoxFuture<'_, Result<FileContent>> {
        blocking(move || files::read(&root, &path))
    }

    fn list_files(
        &self,
        root: PathBuf,
        offset: u32,
        limit: u32,
    ) -> BoxFuture<'_, Result<FileList>> {
        let watched = self.watched(&root, |w| Arc::clone(&w.files));
        blocking(move || match watched.as_deref().map(SharedFiles::lock) {
            // Version 0: the watcher is still indexing.
            Some(files) if files.version > 0 => {
                Ok(file_list::page(&files.paths, offset, limit, files.version))
            }
            _ => {
                let files = file_list::scan(&resolve::root(&root)?);
                Ok(file_list::page(&files, offset, limit, 0))
            }
        })
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

    // File operations land in their own commit; until then they are unavailable.
    fn create(&self, _: PathBuf, _: RelPath, _: bool) -> BoxFuture<'_, Result<()>> {
        blocking(|| Err(not_yet()))
    }

    fn rename(&self, _: PathBuf, _: RelPath, _: RelPath) -> BoxFuture<'_, Result<()>> {
        blocking(|| Err(not_yet()))
    }

    fn copy(&self, _: PathBuf, _: RelPath, _: RelPath) -> BoxFuture<'_, Result<()>> {
        blocking(|| Err(not_yet()))
    }

    fn trash(&self, _: PathBuf, _: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        blocking(|| Err(not_yet()))
    }

    fn search(
        &self,
        id: SearchId,
        root: PathBuf,
        query: SearchQuery,
        sink: EventSink,
        cancel: SearchCancel,
    ) -> Result<()> {
        let plan = search::Plan::new(&query)?;
        let watched = self.watched(&root, |w| Arc::clone(&w.files));
        let run = search::Run {
            id,
            plan,
            root: resolve::root(&root)?,
            sink,
            cancel,
        };
        std::thread::Builder::new()
            .name("ovra-search".into())
            .spawn(move || {
                // Copied under the lock; a walk while indexing runs without it.
                let cached = watched.as_deref().and_then(|files| {
                    let files = files.lock();
                    (files.version > 0).then(|| files.paths.iter().cloned().collect())
                });
                let files: Vec<String> =
                    cached.unwrap_or_else(|| file_list::scan(&run.root).into_iter().collect());
                run.run(&files);
            })
            .map(drop)
            .map_err(|e| ProtocolError::new(ErrorCode::Internal, format!("cannot search: {e}")))
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
