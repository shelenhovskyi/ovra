//! Watches a project folder and pushes `fs.changed` / `fs.git_changed`.
//!
//! Only directories that are not ignored are watched (so `target/` or
//! `node_modules/` cost nothing), plus the git directory for `index` and
//! `HEAD`. Events are batched until the folder is quiet for [`QUIET`].

use std::collections::{BTreeSet, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ignore::WalkBuilder;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use ovra_core::protocol::fs::FsEvent;
use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{EventSink, Result};

use crate::files::TMP_SUFFIX;
use crate::git::{Repo, Statuses};
use crate::listing::visible_names;

/// A batch is sent once no new event arrived for this long...
const QUIET: Duration = Duration::from_millis(20);
/// ...or once it has been collecting for this long.
const MAX_DELAY: Duration = Duration::from_millis(200);
/// Larger batches are reported as their top-level entries.
const MAX_PATHS: usize = 512;

/// Git status of a watched project, kept fresh by the watcher.
#[derive(Debug)]
pub(crate) struct GitCache {
    repo: Option<Repo>,
    status: Mutex<Option<Statuses>>,
}

impl GitCache {
    pub(crate) fn current(&self) -> Option<Statuses> {
        let repo = self.repo.as_ref()?;
        let mut status = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        if status.is_none() {
            *status = repo.status();
        }
        status.clone()
    }

    /// Re-reads the status; true if it changed.
    fn refresh(&self) -> bool {
        let Some(repo) = &self.repo else {
            return false;
        };
        let fresh = repo.status();
        let mut status = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        let changed = *status != fresh;
        *status = fresh;
        changed
    }
}

/// A running watch. Dropping it stops the watcher thread.
#[derive(Debug)]
pub(crate) struct Watch {
    /// The root as the core passed it, to match later requests.
    pub(crate) given_root: PathBuf,
    pub(crate) git: Arc<GitCache>,
    tx: mpsc::Sender<Msg>,
    stopped: Arc<AtomicBool>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Msg::Stop);
    }
}

enum Msg {
    Fs(notify::Result<notify::Event>),
    Stop,
}

fn internal(err: &notify::Error) -> ProtocolError {
    ProtocolError::new(ErrorCode::Internal, format!("cannot watch: {err}"))
}

impl Watch {
    /// Sets up the watches before returning, so changes made right after
    /// this call are reported.
    pub(crate) fn start(project: ProjectId, given_root: PathBuf, sink: EventSink) -> Result<Self> {
        let root = crate::resolve::root(&given_root)?;
        let git = Arc::new(GitCache {
            repo: Repo::discover(&root),
            status: Mutex::new(None),
        });
        let (tx, rx) = mpsc::channel();
        let events = tx.clone();
        let watcher = notify::recommended_watcher(move |e| {
            let _ = events.send(Msg::Fs(e));
        })
        .map_err(|e| internal(&e))?;
        let mut worker = Worker {
            project,
            root,
            sink,
            git: Arc::clone(&git),
            watcher,
            dirs: HashSet::new(),
            stopped: Arc::new(AtomicBool::new(false)),
        };
        let root = worker.root.clone();
        worker.watch_tree(&root);
        if let Some(repo) = &git.repo {
            // Best effort: without it, commits and checkouts surface on the
            // next worktree change.
            let _ = worker
                .watcher
                .watch(&repo.git_dir, RecursiveMode::NonRecursive);
        }
        let stopped = Arc::clone(&worker.stopped);
        std::thread::Builder::new()
            .name("ovra-fs-watch".to_owned())
            .spawn(move || worker.run(&rx))
            .map_err(|e| ProtocolError::new(ErrorCode::Internal, e.to_string()))?;
        Ok(Self {
            given_root,
            git,
            tx,
            stopped,
        })
    }
}

struct Worker {
    project: ProjectId,
    root: PathBuf,
    sink: EventSink,
    git: Arc<GitCache>,
    watcher: RecommendedWatcher,
    dirs: HashSet<PathBuf>,
    stopped: Arc<AtomicBool>,
}

impl Worker {
    /// Watches `dir` and every directory below it that is not ignored.
    fn watch_tree(&mut self, dir: &Path) {
        let walk = WalkBuilder::new(dir)
            .hidden(false)
            .require_git(false)
            .follow_links(false)
            .filter_entry(|e| e.file_name() != ".git")
            .build();
        for entry in walk.filter_map(std::result::Result::ok) {
            if entry.file_type().is_some_and(|t| t.is_dir()) {
                let path = entry.into_path();
                if !self.dirs.contains(&path)
                    && self
                        .watcher
                        .watch(&path, RecursiveMode::NonRecursive)
                        .is_ok()
                {
                    self.dirs.insert(path);
                }
            }
        }
    }

    fn run(mut self, rx: &mpsc::Receiver<Msg>) {
        self.git.current();
        loop {
            let Ok(Msg::Fs(first)) = rx.recv() else {
                return;
            };
            let mut batch = vec![first];
            let started = Instant::now();
            loop {
                let left = MAX_DELAY.saturating_sub(started.elapsed());
                match rx.recv_timeout(QUIET.min(left)) {
                    Ok(Msg::Fs(e)) => batch.push(e),
                    Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => break,
                }
            }
            self.process(batch);
        }
    }

    fn process(&mut self, batch: Vec<notify::Result<notify::Event>>) {
        let mut changed = BTreeSet::new();
        for event in batch.into_iter().filter_map(std::result::Result::ok) {
            if matches!(event.kind, EventKind::Access(_)) {
                continue;
            }
            for path in event.paths {
                if let Some(rel) = self.relative(&path) {
                    changed.insert(rel);
                    self.track_dir(path);
                }
            }
        }
        let git_changed = self.git.refresh();
        if self.stopped.load(Ordering::SeqCst) {
            return;
        }
        if !changed.is_empty() {
            let mut paths: Vec<String> = changed.into_iter().collect();
            if paths.len() > MAX_PATHS {
                let top: BTreeSet<String> = paths
                    .iter()
                    .map(|p| p.split('/').next().unwrap_or(p).to_owned())
                    .collect();
                paths = top.into_iter().collect();
            }
            self.sink.emit(FsEvent::Changed {
                project: self.project.clone(),
                paths,
            });
        }
        if git_changed {
            self.sink.emit(FsEvent::GitChanged {
                project: self.project.clone(),
            });
        }
    }

    /// The project-relative path of a changed file, or `None` for paths that
    /// clients never see (outside the root, inside `.git`, our temp files).
    fn relative(&self, path: &Path) -> Option<String> {
        let rel = path.strip_prefix(&self.root).ok()?;
        let mut parts = Vec::new();
        for part in rel.components() {
            let Component::Normal(part) = part else {
                return None;
            };
            let part = part.to_str()?;
            if part == ".git" {
                return None;
            }
            parts.push(part);
        }
        let last = parts.last()?;
        if last.ends_with(TMP_SUFFIX) {
            return None;
        }
        Some(parts.join("/"))
    }

    /// Starts watching new directories that are not ignored and forgets
    /// removed ones.
    fn track_dir(&mut self, path: PathBuf) {
        if path.is_dir() {
            let visible = path
                .parent()
                .zip(path.file_name())
                .is_some_and(|(parent, name)| {
                    name.to_str()
                        .is_some_and(|name| visible_names(parent).contains(name))
                });
            if visible && !self.dirs.contains(&path) {
                self.watch_tree(&path);
            }
        } else if self.dirs.remove(&path) {
            let _ = self.watcher.unwatch(&path);
        }
    }
}
