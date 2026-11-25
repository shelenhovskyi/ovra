//! Watches a project folder and pushes `fs.changed` / `fs.git_changed`.
//!
//! Only directories that are not ignored are watched (so `target/` or
//! `node_modules/` cost nothing), plus the git directory for `index` and
//! `HEAD`. File events are batched briefly; `git status` runs on its own
//! thread, debounced and one at a time (see [`Timing`]).
//!
//! Once the OS runs out of watches (inotify's `max_user_watches`), further
//! directories are rescanned every [`Timing::poll`] instead, and clients get
//! `fs.watch_limited` with how many there are.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use ovra_core::protocol::fs::FsEvent;
use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{EventSink, Result};

use crate::file_list;
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
    /// How often directories without a watch are rescanned.
    pub(crate) poll: Duration,
    /// Tests only: how many directories the OS lets us watch, to act out a
    /// full inotify limit.
    #[cfg(test)]
    pub(crate) watch_limit: usize,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(20),
            max_delay: Duration::from_millis(200),
            git_quiet: Duration::from_millis(300),
            git_max_delay: Duration::from_secs(2),
            poll: Duration::from_secs(2),
            #[cfg(test)]
            watch_limit: usize::MAX,
        }
    }
}

/// Larger batches are reported as their top-level entries.
const MAX_PATHS: usize = 512;

/// Git status of a watched project, kept fresh by the watcher. The lock is
/// held while `git status` runs, so at most one runs per project.
#[derive(Debug)]
pub(crate) struct GitCache {
    /// Set once: at the start, or when the folder becomes a repository.
    repo: OnceLock<Repo>,
    status: Mutex<Option<Statuses>>,
    /// When the index and HEAD last changed: staging, committing or
    /// switching can leave the statuses the tree shows as they were, but
    /// the git panel must hear of it.
    stamp: Mutex<Option<Stamp>>,
    /// How many times `git status` ran.
    pub(crate) runs: AtomicUsize,
}

impl GitCache {
    pub(crate) fn new(repo: Option<Repo>) -> Self {
        let cache = Self {
            repo: OnceLock::new(),
            status: Mutex::new(None),
            stamp: Mutex::new(None),
            runs: AtomicUsize::new(0),
        };
        if let Some(repo) = repo {
            let _ = cache.repo.set(repo);
        }
        cache
    }

    fn repo(&self) -> Option<&Repo> {
        self.repo.get()
    }

    fn lock(&self) -> MutexGuard<'_, Option<Statuses>> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn run(&self, repo: &Repo) -> Option<Statuses> {
        self.runs.fetch_add(1, Ordering::Relaxed);
        repo.status()
    }

    pub(crate) fn current(&self) -> Option<Statuses> {
        let repo = self.repo()?;
        let mut status = self.lock();
        if status.is_none() {
            *self.stamp.lock().unwrap_or_else(PoisonError::into_inner) = Some(stamp(repo));
            *status = self.run(repo);
        }
        status.clone()
    }

    /// Re-reads the status; true if it or the index or HEAD changed.
    pub(crate) fn refresh(&self) -> bool {
        let Some(repo) = self.repo() else {
            return false;
        };
        let stamp = Some(stamp(repo));
        let moved = {
            let mut last = self.stamp.lock().unwrap_or_else(PoisonError::into_inner);
            let moved = last.is_some() && *last != stamp;
            *last = stamp;
            moved
        };
        let mut status = self.lock();
        let fresh = self.run(repo);
        let changed = *status != fresh;
        *status = fresh;
        changed || moved
    }
}

type Stamp = (Option<SystemTime>, Option<SystemTime>);

/// When the index and HEAD of `repo` were last written.
fn stamp(repo: &Repo) -> Stamp {
    let modified = |name: &str| {
        std::fs::metadata(repo.git_dir.join(name))
            .and_then(|m| m.modified())
            .ok()
    };
    (modified("index"), modified("HEAD"))
}

/// A running watch. Dropping it stops the watcher thread.
#[derive(Debug)]
pub(crate) struct Watch {
    /// The root as the core passed it, to match later requests.
    pub(crate) given_root: PathBuf,
    pub(crate) git: Arc<GitCache>,
    pub(crate) files: Arc<SharedFiles>,
    pub(crate) dirs: Arc<Dirs>,
    project: ProjectId,
    sink: EventSink,
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
    /// Repeats `fs.watch_limited` for clients that joined after it was
    /// sent (a reload, a second window); they list the root first.
    pub(crate) fn remind_unwatched(&self) {
        let unwatched = self.dirs.unwatched();
        if unwatched > 0 && !self.stopped.load(Ordering::SeqCst) {
            self.sink.emit(FsEvent::WatchLimited {
                project: self.project.clone(),
                unwatched: u32::try_from(unwatched).unwrap_or(u32::MAX),
            });
        }
    }

    /// Watches the root and the git directory, then returns; the rest of
    /// the tree is walked and watched on the watcher thread. A directory
    /// is watched before it is read, so nothing that changes during the
    /// walk is missed. [`Dirs::add`] lets listings jump the queue.
    pub(crate) fn start(
        project: ProjectId,
        given_root: PathBuf,
        sink: EventSink,
        timing: Timing,
    ) -> Result<Self> {
        let root = crate::resolve::root(&given_root)?;
        let git = Arc::new(GitCache::new(Repo::discover(&root)));
        let stopped = Arc::new(AtomicBool::new(false));
        let git_tx = match git.repo() {
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
        let mut watcher = notify::recommended_watcher(move |e| {
            let _ = events.send(Msg::Fs(e));
        })
        .map_err(|e| internal(&e))?;
        if ONE_RECURSIVE_WATCH {
            watcher
                .watch(&root, RecursiveMode::Recursive)
                .map_err(|e| internal(&e))?;
        }
        let dirs = Arc::new(Dirs {
            watcher: Mutex::new(watcher),
            watched: Mutex::new(HashSet::new()),
            polled: Mutex::new(HashMap::new()),
            #[cfg(test)]
            watch_budget: AtomicUsize::new(timing.watch_limit),
        });
        dirs.add(&root);
        if let Some(repo) = git.repo() {
            dirs.watch_git_dir(&repo.git_dir);
        }
        let worker = Worker {
            project: project.clone(),
            root,
            sink: sink.clone(),
            git: Arc::clone(&git),
            git_tx,
            files: Arc::new(SharedFiles::default()),
            dirs: Arc::clone(&dirs),
            walked: HashSet::new(),
            stopped: Arc::clone(&stopped),
            timing,
            reported_unwatched: 0,
        };
        let files = Arc::clone(&worker.files);
        std::thread::Builder::new()
            .name("ovra-fs-watch".to_owned())
            .spawn(move || worker.run(&rx))
            .map_err(|e| thread_error(&e))?;
        Ok(Self {
            given_root,
            git,
            files,
            dirs,
            project,
            sink,
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
pub(crate) fn spawn_git_refresher(
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
    git: Arc<GitCache>,
    /// Asks for a git status refresh; `None` outside a repository.
    git_tx: Option<mpsc::Sender<()>>,
    files: Arc<SharedFiles>,
    dirs: Arc<Dirs>,
    /// Directories whose files are in the set (watching alone may come
    /// earlier, from a listing).
    walked: HashSet<PathBuf>,
    stopped: Arc<AtomicBool>,
    timing: Timing,
    /// The `unwatched` count clients last heard.
    reported_unwatched: usize,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// FSEvents (macOS) restarts its event stream on every `watch` call and
/// drops what happens meanwhile, so there one recursive watch covers the
/// root and [`Dirs::add`] only records directories. Elsewhere each directory
/// that is not ignored gets its own watch.
const ONE_RECURSIVE_WATCH: bool = cfg!(target_os = "macos");

/// The directories a project watches, shared by the watcher thread and
/// listings. Events count only for files directly in one of them.
#[derive(Debug)]
pub(crate) struct Dirs {
    watcher: Mutex<RecommendedWatcher>,
    watched: Mutex<HashSet<PathBuf>>,
    /// Watched directories the OS had no watch left for, with what they
    /// held at the last scan. Locked after `watched` when both are needed.
    polled: Mutex<HashMap<PathBuf, Snapshot>>,
    /// Tests only: watches left before the simulated limit is reached.
    #[cfg(test)]
    pub(crate) watch_budget: AtomicUsize,
}

/// What a directory held at a scan: per entry, whether it is a directory,
/// its size and modification time (`None` if it could not be read).
type Snapshot = HashMap<OsString, Option<(bool, u64, Option<SystemTime>)>>;

/// `None` if `dir` cannot be read (it is gone).
fn snapshot(dir: &Path) -> Option<Snapshot> {
    let entries = std::fs::read_dir(dir).ok()?;
    Some(
        entries
            .filter_map(std::result::Result::ok)
            .map(|e| {
                // Like `symlink_metadata`: links are not followed.
                let meta = e.metadata().ok();
                let meta = meta.map(|m| (m.is_dir(), m.len(), m.modified().ok()));
                (e.file_name(), meta)
            })
            .collect(),
    )
}

impl Dirs {
    /// Watches `dir` (not recursively) unless it already is. If the OS has
    /// no watch left, `dir` is polled instead.
    pub(crate) fn add(&self, dir: &Path) {
        let mut watched = lock(&self.watched);
        if watched.contains(dir) {
            return;
        }
        if !ONE_RECURSIVE_WATCH {
            match self.watch_os(dir) {
                Ok(()) => {}
                Err(e) if matches!(e.kind, notify::ErrorKind::MaxFilesWatch) => {
                    // Scanned before the caller reads it, as a watch would be
                    // in place before.
                    let Some(snapshot) = snapshot(dir) else {
                        return;
                    };
                    lock(&self.polled).insert(dir.to_path_buf(), snapshot);
                }
                Err(_) => return,
            }
        }
        watched.insert(dir.to_path_buf());
    }

    fn watch_os(&self, dir: &Path) -> notify::Result<()> {
        #[cfg(test)]
        if self
            .watch_budget
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_err()
        {
            return Err(notify::Error::new(notify::ErrorKind::MaxFilesWatch));
        }
        lock(&self.watcher).watch(dir, RecursiveMode::NonRecursive)
    }

    /// Watches the git directory for `index` and `HEAD`. Best effort (it
    /// also fails once the watch limit is reached): without it, commits and
    /// checkouts surface on the next worktree change.
    fn watch_git_dir(&self, git_dir: &Path) {
        let _ = lock(&self.watcher).watch(git_dir, RecursiveMode::NonRecursive);
    }

    fn contains(&self, dir: &Path) -> bool {
        lock(&self.watched).contains(dir)
    }

    /// How many directories are polled rather than watched.
    pub(crate) fn unwatched(&self) -> usize {
        lock(&self.polled).len()
    }

    /// Rescans the polled directories, first trying again to watch them (the
    /// limit may have been raised, or watches freed) until the OS refuses
    /// one. Returns the paths that changed since the last scan, including
    /// directories that are gone.
    fn rescan(&self) -> Vec<PathBuf> {
        let dirs: Vec<PathBuf> = lock(&self.polled).keys().cloned().collect();
        let mut changed = Vec::new();
        // Each attempt is a round trip to notify's thread: with thousands
        // of polled directories, stop at the first refusal.
        let mut full = false;
        for dir in dirs {
            // Watch, then read, so nothing that happens between is missed.
            let watching = !full
                && match self.watch_os(&dir) {
                    Ok(()) => true,
                    Err(e) => {
                        full = matches!(e.kind, notify::ErrorKind::MaxFilesWatch);
                        false
                    }
                };
            let fresh = snapshot(&dir);
            let mut polled = lock(&self.polled);
            let Some(old) = polled.get_mut(&dir) else {
                continue; // removed meanwhile
            };
            let Some(fresh) = fresh else {
                polled.remove(&dir);
                changed.push(dir);
                continue;
            };
            for (name, meta) in &fresh {
                if old.get(name) != Some(meta) {
                    changed.push(dir.join(name));
                }
            }
            for name in old.keys().filter(|n| !fresh.contains_key(*n)) {
                changed.push(dir.join(name));
            }
            if watching {
                polled.remove(&dir);
            } else {
                *old = fresh;
            }
        }
        changed
    }

    /// Forgets a removed directory and everything below it.
    fn remove(&self, dir: &Path) {
        let mut watched = lock(&self.watched);
        // inotify drops the watch of a deleted directory by itself (removing
        // it again only logs a warning); macOS has no per-directory watch.
        if watched.remove(dir) && cfg!(windows) {
            let _ = lock(&self.watcher).unwatch(dir);
        }
        watched.retain(|d| !d.starts_with(dir));
        lock(&self.polled).retain(|d, _| !d.starts_with(dir));
    }
}

impl Worker {
    /// Watches `dir` and every directory below it that is not ignored, and
    /// adds the files found to the set. Returns the files that were new.
    fn watch_tree(&mut self, dir: &Path) -> Vec<String> {
        let mut added = Vec::new();
        for entry in file_list::walk(dir).filter_map(std::result::Result::ok) {
            if entry.file_type().is_some_and(|t| t.is_dir()) {
                // The walk reads a directory only after yielding it, so the
                // watch is in place before its entries are read.
                let path = entry.into_path();
                self.dirs.add(&path);
                self.walked.insert(path);
            } else if let Some(rel) = file_list::relative(&self.root, entry.path())
                && self.files.lock().paths.insert(rel.clone())
            {
                added.push(rel);
            }
        }
        added
    }

    fn run(mut self, rx: &mpsc::Receiver<Msg>) {
        let root = self.root.clone();
        self.watch_tree(&root);
        let version = {
            let mut files = self.files.lock();
            files.version = 1;
            files.version
        };
        // Tells clients that listed files while indexing (version 0) to
        // list again; events queued during the walk follow.
        if !self.stopped.load(Ordering::SeqCst) {
            self.sink.emit(FsEvent::FilesChanged {
                project: self.project.clone(),
                added: Vec::new(),
                removed: Vec::new(),
                version,
            });
        }
        self.report_unwatched();
        let mut polled_at = Instant::now();
        loop {
            // Wakes up for the next rescan even when nothing is polled yet:
            // a listing may add a polled directory at any time.
            let wait = self.timing.poll.saturating_sub(polled_at.elapsed());
            match rx.recv_timeout(wait) {
                Ok(Msg::Fs(first)) => self.collect(rx, first),
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
            if self.stopped.load(Ordering::SeqCst) {
                return;
            }
            if polled_at.elapsed() >= self.timing.poll {
                polled_at = Instant::now();
                self.poll();
            }
        }
    }

    /// Collects a batch of events starting with `first`, then processes it.
    fn collect(&mut self, rx: &mpsc::Receiver<Msg>, first: notify::Result<notify::Event>) {
        let mut batch = vec![first];
        let started = Instant::now();
        loop {
            let left = self.timing.max_delay.saturating_sub(started.elapsed());
            match rx.recv_timeout(self.timing.quiet.min(left)) {
                Ok(Msg::Fs(e)) => batch.push(e),
                // The caller sees the stop flag; a disconnect means the
                // watch is gone too.
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => break,
            }
        }
        self.process(batch);
    }

    /// Rescans the directories without a watch, as if their changes had
    /// been reported, and tells clients if their number changed.
    fn poll(&mut self) {
        if self.dirs.unwatched() > 0 {
            let changed = self.dirs.rescan();
            if !changed.is_empty() {
                let event = changed
                    .into_iter()
                    .fold(notify::Event::new(EventKind::Any), notify::Event::add_path);
                self.process(vec![Ok(event)]);
            }
        }
        self.report_unwatched();
    }

    fn report_unwatched(&mut self) {
        let unwatched = self.dirs.unwatched();
        if unwatched == self.reported_unwatched || self.stopped.load(Ordering::SeqCst) {
            return;
        }
        self.reported_unwatched = unwatched;
        self.sink.emit(FsEvent::WatchLimited {
            project: self.project.clone(),
            unwatched: u32::try_from(unwatched).unwrap_or(u32::MAX),
        });
    }

    fn process(&mut self, batch: Vec<notify::Result<notify::Event>>) {
        let mut changed = BTreeSet::new();
        let mut set = SetChange::default();
        let mut visible = HashMap::new();
        let dot_git = self.root.join(".git");
        let mut git_appeared = false;
        for event in batch.into_iter().filter_map(std::result::Result::ok) {
            if matches!(event.kind, EventKind::Access(_)) {
                continue;
            }
            for path in event.paths {
                git_appeared |= self.git_tx.is_none()
                    && (path == dot_git || path.parent() == Some(dot_git.as_path()));
                // Ignored directories are not watched, so their files do not
                // count even where the OS reports them (macOS).
                let watched = path.parent().is_some_and(|p| self.dirs.contains(p));
                if watched
                    && let Some(rel) = self.relative(&path)
                    && changed.insert(rel.clone())
                {
                    self.update(path, rel, &mut set, &mut visible);
                }
            }
        }
        if git_appeared {
            self.find_repo(&dot_git);
        }
        // Changes inside `.git` (commits, checkouts) count too.
        if let Some(git) = &self.git_tx {
            let _ = git.send(());
        }
        let version = if set.added.is_empty() && set.removed.is_empty() {
            None
        } else {
            let mut files = self.files.lock();
            files.version += 1;
            Some(files.version)
        };
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
        if let Some(version) = version {
            self.sink.emit(FsEvent::FilesChanged {
                project: self.project.clone(),
                added: set.added.into_iter().collect(),
                removed: set.removed.into_iter().collect(),
                version,
            });
        }
    }

    /// Looks for the repository once `.git` appeared in a folder that had
    /// none ("Set up git"), and keeps its status fresh from then on. Until
    /// git has written enough of it, `.git` itself is watched, so its next
    /// change looks again.
    fn find_repo(&mut self, dot_git: &Path) {
        if dot_git.is_dir() {
            self.dirs.watch_git_dir(dot_git);
        }
        let Some(repo) = Repo::discover(&self.root) else {
            return;
        };
        self.dirs.watch_git_dir(&repo.git_dir);
        if self.git.repo.set(repo).is_err() {
            return;
        }
        let refresher = spawn_git_refresher(
            self.project.clone(),
            self.sink.clone(),
            Arc::clone(&self.git),
            Arc::clone(&self.stopped),
            self.timing,
        );
        self.git_tx = refresher.ok();
        // The tree had no status at all: tell clients to list again.
        if !self.stopped.load(Ordering::SeqCst) {
            self.sink.emit(FsEvent::GitChanged {
                project: self.project.clone(),
            });
        }
    }

    /// The project-relative path of a changed file, or `None` for paths that
    /// clients never see (outside the root, inside `.git`, our temp files).
    fn relative(&self, path: &Path) -> Option<String> {
        let rel = file_list::relative(&self.root, path)?;
        let hidden = rel.split('/').any(|p| p == ".git") || rel.ends_with(TMP_SUFFIX);
        (!hidden).then_some(rel)
    }

    /// Keeps the watched directories and the file set in step with one
    /// changed path.
    fn update(
        &mut self,
        path: PathBuf,
        rel: String,
        set: &mut SetChange,
        visible: &mut HashMap<PathBuf, HashSet<String>>,
    ) {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            // Gone: forget it and, if it was a directory, all below it.
            self.dirs.remove(&path);
            self.walked.retain(|d| !d.starts_with(&path));
            let mut files = self.files.lock();
            let below = format!("{rel}/");
            let mut gone: Vec<String> = files
                .paths
                .range(below.clone()..)
                .take_while(|p| p.starts_with(&below))
                .cloned()
                .collect();
            gone.extend(files.paths.get(&rel).cloned());
            for p in gone {
                files.paths.remove(&p);
                set.removed.insert(p.clone());
                set.added.remove(&p);
            }
            return;
        };
        let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
        else {
            return;
        };
        // One ignore check per directory and batch, however many files changed.
        let shown = self.dirs.contains(parent)
            && visible
                .entry(parent.to_path_buf())
                .or_insert_with(|| visible_names(parent))
                .contains(name);
        if !shown {
            return;
        }
        if meta.is_dir() {
            if !self.walked.contains(&path) {
                for added in self.watch_tree(&path) {
                    set.removed.remove(&added);
                    set.added.insert(added);
                }
            }
        } else if self.files.lock().paths.insert(rel.clone()) {
            set.removed.remove(&rel);
            set.added.insert(rel);
        }
    }
}

/// Additions to and removals from the file set during one batch.
#[derive(Debug, Default)]
struct SetChange {
    added: BTreeSet<String>,
    removed: BTreeSet<String>,
}

/// The non-ignored files of a watched project, kept by the watcher.
#[derive(Debug, Default)]
pub(crate) struct FileSet {
    pub(crate) paths: BTreeSet<String>,
    pub(crate) version: u64,
}

/// A [`FileSet`] behind a lock.
#[derive(Debug, Default)]
pub(crate) struct SharedFiles(Mutex<FileSet>);

impl SharedFiles {
    pub(crate) fn lock(&self) -> MutexGuard<'_, FileSet> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
