//! In-memory fakes of every service, for tests in this and other crates
//! (enable the `fakes` feature in `[dev-dependencies]`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::path::RelPath;
use crate::protocol::fs::{DirEntry, DirListing, EntryKind, FileBody, FileContent};
use crate::protocol::projects::{Project, ProjectId, ProjectState, ProjectStatePatch};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{BoxFuture, EventSink, FsService, Opener, ProjectStore, Result};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn not_found(what: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::NotFound, format!("{what} not found"))
}

/// Projects kept in memory; ids are `p1`, `p2`, ... and time is a counter.
#[derive(Debug, Default)]
pub struct MemoryProjects {
    state: Mutex<(Vec<Project>, u64)>,
    saved: Mutex<BTreeMap<ProjectId, ProjectState>>,
}

impl ProjectStore for MemoryProjects {
    fn list(&self) -> Result<Vec<Project>> {
        let mut list = lock(&self.state).0.clone();
        list.sort_by_key(|p| std::cmp::Reverse(p.last_opened_ms));
        Ok(list)
    }

    fn add(&self, root: &Path) -> Result<Project> {
        let mut state = lock(&self.state);
        let root = root.to_string_lossy().into_owned();
        if let Some(existing) = state.0.iter().find(|p| p.root == root) {
            return Ok(existing.clone());
        }
        let project = Project {
            id: ProjectId(format!("p{}", state.0.len() + 1)),
            name: root.rsplit('/').next().unwrap_or(&root).to_owned(),
            root,
            last_opened_ms: None,
        };
        state.0.push(project.clone());
        Ok(project)
    }

    fn remove(&self, id: &ProjectId) -> Result<()> {
        let mut state = lock(&self.state);
        let before = state.0.len();
        state.0.retain(|p| &p.id != id);
        if state.0.len() == before {
            return Err(not_found(&id.0));
        }
        lock(&self.saved).remove(id);
        Ok(())
    }

    fn get(&self, id: &ProjectId) -> Result<Project> {
        lock(&self.state)
            .0
            .iter()
            .find(|p| &p.id == id)
            .cloned()
            .ok_or_else(|| not_found(&id.0))
    }

    fn touch(&self, id: &ProjectId) -> Result<Project> {
        let mut state = lock(&self.state);
        state.1 += 1;
        let now = state.1;
        let project = state
            .0
            .iter_mut()
            .find(|p| &p.id == id)
            .ok_or_else(|| not_found(&id.0))?;
        project.last_opened_ms = Some(now);
        Ok(project.clone())
    }

    fn state(&self, id: &ProjectId) -> Result<ProjectState> {
        self.get(id)?;
        Ok(lock(&self.saved).get(id).cloned().unwrap_or_default())
    }

    fn update_state(&self, id: &ProjectId, patch: ProjectStatePatch) -> Result<()> {
        self.get(id)?;
        patch.apply(lock(&self.saved).entry(id.clone()).or_default());
        Ok(())
    }
}

/// Text files in memory, keyed by root and relative path. Directories are
/// implied by file paths. Records watch calls.
#[derive(Debug, Default)]
pub struct MemoryFs {
    files: Mutex<BTreeMap<(PathBuf, String), (String, u64)>>,
    clock: Mutex<u64>,
    /// Projects currently watched, with the sink they emit to.
    pub watched: Mutex<Vec<(ProjectId, EventSink)>>,
}

impl MemoryFs {
    /// Adds or replaces a file.
    pub fn put(&self, root: &Path, path: &str, text: &str) {
        let mtime = self.tick();
        lock(&self.files).insert(
            (root.to_path_buf(), path.to_owned()),
            (text.to_owned(), mtime),
        );
    }

    fn tick(&self) -> u64 {
        let mut clock = lock(&self.clock);
        *clock += 1;
        *clock
    }

    pub fn watched_ids(&self) -> Vec<ProjectId> {
        lock(&self.watched)
            .iter()
            .map(|(id, _)| id.clone())
            .collect()
    }
}

impl FsService for MemoryFs {
    fn list_dir(&self, root: PathBuf, path: RelPath) -> BoxFuture<'_, Result<DirListing>> {
        Box::pin(async move {
            let prefix = if path.is_root() {
                String::new()
            } else {
                format!("{path}/")
            };
            let mut entries: BTreeMap<String, EntryKind> = BTreeMap::new();
            for (r, p) in lock(&self.files).keys() {
                if *r != root {
                    continue;
                }
                if let Some(rest) = p.strip_prefix(&prefix) {
                    if let Some((dir, _)) = rest.split_once('/') {
                        entries.insert(dir.to_owned(), EntryKind::Dir);
                    } else {
                        entries.entry(rest.to_owned()).or_insert(EntryKind::File);
                    }
                }
            }
            if entries.is_empty() && !path.is_root() {
                return Err(not_found(&path));
            }
            let mut entries: Vec<DirEntry> = entries
                .into_iter()
                .map(|(name, kind)| DirEntry {
                    name,
                    kind,
                    ignored: false,
                    git: None,
                })
                .collect();
            entries.sort_by_key(|e| (e.kind != EntryKind::Dir, e.name.clone()));
            Ok(DirListing {
                path: path.to_string(),
                entries,
            })
        })
    }

    fn read_file(&self, root: PathBuf, path: RelPath) -> BoxFuture<'_, Result<FileContent>> {
        Box::pin(async move {
            let files = lock(&self.files);
            let (text, mtime) = files
                .get(&(root, path.to_string()))
                .ok_or_else(|| not_found(&path))?;
            Ok(FileContent {
                path: path.to_string(),
                size: text.len() as u64,
                mtime_ms: *mtime,
                body: FileBody::Text { text: text.clone() },
            })
        })
    }

    fn write_file(
        &self,
        root: PathBuf,
        path: RelPath,
        text: String,
        expected_mtime_ms: Option<u64>,
    ) -> BoxFuture<'_, Result<u64>> {
        Box::pin(async move {
            let key = (root, path.to_string());
            let current = lock(&self.files).get(&key).map(|(_, mtime)| *mtime);
            if expected_mtime_ms.is_some_and(|expected| current.is_some_and(|c| c != expected)) {
                return Err(ProtocolError::new(
                    ErrorCode::Conflict,
                    format!("{path} changed on disk"),
                ));
            }
            let mtime = self.tick();
            lock(&self.files).insert(key, (text, mtime));
            Ok(mtime)
        })
    }

    fn watch(&self, project: ProjectId, _root: PathBuf, sink: EventSink) -> Result<()> {
        lock(&self.watched).push((project, sink));
        Ok(())
    }

    fn unwatch(&self, project: &ProjectId) {
        lock(&self.watched).retain(|(id, _)| id != project);
    }
}

/// Records every file and URL it is asked to open.
#[derive(Debug, Default)]
pub struct RecordingOpener {
    pub opened: Mutex<Vec<(PathBuf, Option<u32>)>>,
    pub urls: Mutex<Vec<String>>,
}

impl Opener for RecordingOpener {
    fn open_in_editor(&self, file: &Path, line: Option<u32>) -> Result<()> {
        lock(&self.opened).push((file.to_path_buf(), line));
        Ok(())
    }
    fn open_url(&self, url: &str) -> Result<()> {
        lock(&self.urls).push(url.to_owned());
        Ok(())
    }
}
