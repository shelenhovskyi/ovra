//! Watches a project folder and pushes `fs.changed` / `fs.git_changed`.
//!
//! Only directories that are not ignored are watched (so `target/` or
//! `node_modules/` cost nothing), plus the git directory for `index` and
//! `HEAD`. File events are batched briefly; `git status` runs on its own
//! thread, debounced and one at a time (see [`Timing`]).

use std::collections::{BTreeSet, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
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

/// How long the watcher waits before reporting.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    /// A batch of file events is sent once none arrived for this long...
    pub(crate) quiet: Duration,
    /// ...or once it has been collecting for this long.
    pub(crate) max_delay: Duration,
    /// `git status` runs once file batches stopped for this long...
    pub(crate) git_quiet: Duration,
    /// ...or at the latest this long after the first change, so a long
    /// stream of writes (an agent at work) still refreshes the status.
    pub(crate) git_max_delay: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(20),
            max_delay: Duration::from_millis(200),
            git_quiet: Duration::from_millis(300),
            git_max_delay: Duration::from_secs(2),
        }
    }
}

/// Larger batches are reported as their top-level entries.
const MAX_PATHS: usize = 512;

/// Git status of a watched project, kept fresh by the watcher. The lock is
/// held while `git status` runs, so at most one runs per project.
#[derive(Debug)]
pub(crate) struct GitCache {
    repo: Option<Repo>,
    status: Mutex<Option<Statuses>>,
    /// How many times `git status` ran.
    pub(crate) runs: AtomicUsize,
}

impl GitCache {
    fn lock(&self) -> MutexGuard<'_, Option<Statuses>> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn run(&self, repo: &Repo) -> Option<Statuses> {
        self.runs.fetch_add(1, Ordering::Relaxed);
        repo.status()
    }

    pub(crate) fn current(&self) -> Option<Statuses> {
        let repo = self.repo.as_ref()?;
        let mut status = self.lock();
        if status.is_none() {
            *status = self.run(repo);
        }
        status.clone()
    }

    /// Re-reads the status; true if it changed.
    fn refresh(&self) -> bool {
        let Some(repo) = &self.repo else {
            return false;
        };
        let mut status = self.lock();
        let fresh = self.run(repo);
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
    pub(crate) fn start(
        project: ProjectId,
        given_root: PathBuf,
        sink: EventSink,
        timing: Timing,
    ) -> Result<Self> {
        let root = crate::resolve::root(&given_root)?;
        let git = Arc::new(GitCache {
            repo: Repo::discover(&root),
            status: Mutex::new(None),
            runs: AtomicUsize::new(0),
        });
        let stopped = Arc::new(AtomicBool::new(false));
        let git_tx = match git.repo {
            Some(_) => Some(spawn_git_refresher(
                project.clone(),
                sink.clone(),
                Arc::clone(&git),
                Arc::clone(&stopped),
                timing,
            )?),
            None => None,
        };
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
            git_tx,
            watcher,
            dirs: HashSet::new(),
            stopped: Arc::clone(&stopped),
            timing,
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
        std::thread::Builder::new()
            .name("ovra-fs-watch".to_owned())
            .spawn(move || worker.run(&rx))
            .map_err(|e| thread_error(&e))?;
        Ok(Self {
            given_root,
            git,
            tx,
            stopped,
        })
    }
}

fn thread_error(err: &std::io::Error) -> ProtocolError {
    ProtocolError::new(ErrorCode::Internal, err.to_string())
}

/// Runs `git status` on its own thread so file events never wait for git.
/// Signals that arrive while it waits or runs fold into one refresh.
fn spawn_git_refresher(
    project: ProjectId,
    sink: EventSink,
    git: Arc<GitCache>,
    stopped: Arc<AtomicBool>,
    timing: Timing,
) -> Result<mpsc::Sender<()>> {
    let (tx, rx) = mpsc::channel::<()>();
    std::thread::Builder::new()
        .name("ovra-git-status".to_owned())
        .spawn(move || {
            git.current();
            while rx.recv().is_ok() {
                let started = Instant::now();
                loop {
                    let left = timing.git_max_delay.saturating_sub(started.elapsed());
                    match rx.recv_timeout(timing.git_quiet.min(left)) {
                        Ok(()) if !left.is_zero() => {}
                        Ok(()) | Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                if git.refresh() && !stopped.load(Ordering::SeqCst) {
                    sink.emit(FsEvent::GitChanged {
                        project: project.clone(),
                    });
                }
            }
        })
        .map_err(|e| thread_error(&e))?;
    Ok(tx)
}

struct Worker {
    project: ProjectId,
    root: PathBuf,
    sink: EventSink,
    /// Asks for a git status refresh; `None` outside a repository.
    git_tx: Option<mpsc::Sender<()>>,
    watcher: RecommendedWatcher,
    dirs: HashSet<PathBuf>,
    stopped: Arc<AtomicBool>,
    timing: Timing,
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
        loop {
            let Ok(Msg::Fs(first)) = rx.recv() else {
                return;
            };
            let mut batch = vec![first];
            let started = Instant::now();
            loop {
                let left = self.timing.max_delay.saturating_sub(started.elapsed());
                match rx.recv_timeout(self.timing.quiet.min(left)) {
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
        // Changes inside `.git` (commits, checkouts) count too.
        if let Some(git) = &self.git_tx {
            let _ = git.send(());
        }
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
