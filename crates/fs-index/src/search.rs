//! Content search behind `search.start`: ripgrep's searcher over the
//! project's file list, a few files at a time on worker threads. Results go
//! out in batches, so the first ones show quickly and the event stream stays
//! small.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use ovra_core::protocol::search::{
    FileMatches, LineMatch, MAX_SEARCH_LINES, SearchEvent, SearchId, SearchQuery,
};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{EventSink, Result, SearchCancel};

/// Lines longer than this (in bytes) are cut to a window around the first
/// match.
const MAX_TEXT: usize = 400;
/// How much of a cut line comes before its first match, in bytes.
const BEFORE_MATCH: usize = 80;
/// A batch goes out once it holds this many lines...
const BATCH_LINES: usize = 200;
/// ...or once its first file waited this long.
const BATCH_WAIT: Duration = Duration::from_millis(50);
const MAX_WORKERS: usize = 8;
/// Larger files (logs, dumps, bundles) are skipped.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// A checked query: what to match and which files to look in.
#[derive(Debug)]
pub(crate) struct Plan {
    matcher: RegexMatcher,
    include: Option<GlobSet>,
    exclude: Option<GlobSet>,
}

fn bad_request(message: String) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, message)
}

impl Plan {
    pub(crate) fn new(query: &SearchQuery) -> Result<Self> {
        let matcher = RegexMatcherBuilder::new()
            .case_insensitive(!query.case_sensitive)
            .word(query.whole_word)
            .fixed_strings(!query.regex)
            .line_terminator(Some(b'\n'))
            .build(&query.pattern)
            .map_err(|e| bad_request(format!("invalid search pattern: {e}")))?;
        Ok(Self {
            matcher,
            include: globs(&query.include)?,
            exclude: globs(&query.exclude)?,
        })
    }

    fn wants(&self, path: &str) -> bool {
        self.include.as_ref().is_none_or(|set| set.is_match(path))
            && !self.exclude.as_ref().is_some_and(|set| set.is_match(path))
    }
}

/// `None` for no globs. A glob without `/` matches a name at any depth; a
/// folder matches what is inside it.
fn globs(patterns: &[String]) -> Result<Option<GlobSet>> {
    let mut set = GlobSetBuilder::new();
    let mut any = false;
    for pattern in patterns {
        let trimmed = pattern.trim();
        let glob = trimmed.trim_start_matches("./").trim_matches('/');
        if glob.is_empty() {
            continue;
        }
        let rooted = trimmed.starts_with('/') || glob.contains('/');
        let base = if rooted {
            glob.to_owned()
        } else {
            format!("**/{glob}")
        };
        for variant in [base.clone(), format!("{base}/**")] {
            let built = GlobBuilder::new(&variant)
                .literal_separator(true)
                .build()
                .map_err(|e| bad_request(format!("invalid glob {pattern:?}: {e}")))?;
            set.add(built);
        }
        any = true;
    }
    if !any {
        return Ok(None);
    }
    set.build()
        .map(Some)
        .map_err(|e| bad_request(format!("invalid globs: {e}")))
}

/// Everything one search needs on its thread.
#[derive(Debug)]
pub(crate) struct Run {
    pub(crate) id: SearchId,
    pub(crate) plan: Plan,
    /// Canonical project root.
    pub(crate) root: PathBuf,
    pub(crate) sink: EventSink,
    pub(crate) cancel: SearchCancel,
}

impl Run {
    /// Searches `files` (project-relative) and emits the results and the
    /// final `search.done`.
    pub(crate) fn run(self, files: &[String]) {
        let next = AtomicUsize::new(0);
        let full = AtomicBool::new(false);
        let (tx, rx) = mpsc::channel();
        let workers = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .clamp(1, MAX_WORKERS)
            .min(files.len().max(1));
        let totals = std::thread::scope(|scope| {
            for _ in 0..workers {
                let tx = tx.clone();
                let (run, next, full) = (&self, &next, &full);
                scope.spawn(move || run.work(files, next, full, &tx));
            }
            drop(tx);
            self.collect(&rx, &full)
        });
        // Stopped at the limit before every file was searched: more may match.
        let unsearched = next.load(Ordering::Relaxed) < files.len();
        let totals = Totals {
            truncated: totals.truncated || (full.load(Ordering::Relaxed) && unsearched),
            ..totals
        };
        self.sink.emit(SearchEvent::Done {
            id: self.id,
            files: totals.files,
            lines: totals.lines,
            truncated: totals.truncated,
            cancelled: self.cancel.is_cancelled(),
        });
    }

    fn stopped(&self, full: &AtomicBool) -> bool {
        self.cancel.is_cancelled() || full.load(Ordering::Relaxed)
    }

    /// One worker: takes the next file until none are left.
    fn work(
        &self,
        files: &[String],
        next: &AtomicUsize,
        full: &AtomicBool,
        tx: &mpsc::Sender<FileMatches>,
    ) {
        let mut searcher = SearcherBuilder::new()
            .line_number(true)
            .binary_detection(BinaryDetection::quit(0))
            .build();
        while !self.stopped(full) {
            let Some(path) = files.get(next.fetch_add(1, Ordering::Relaxed)) else {
                return;
            };
            if !self.plan.wants(path) {
                continue;
            }
            // Joined part by part, so the path compares equal to its canonical
            // form on Windows too.
            let file: PathBuf = std::iter::once(self.root.as_os_str())
                .chain(path.split('/').map(std::ffi::OsStr::new))
                .collect();
            let lines = self.search_file(&mut searcher, &file, full);
            if !lines.is_empty() {
                let found = FileMatches {
                    path: path.clone(),
                    lines,
                };
                if tx.send(found).is_err() {
                    return;
                }
            }
        }
    }

    /// Matching lines of one file; none for a binary file, a symlink or a
    /// file that cannot be read.
    fn search_file(
        &self,
        searcher: &mut Searcher,
        file: &Path,
        full: &AtomicBool,
    ) -> Vec<LineMatch> {
        // Every part of the path must be what it says: a symlink (also a
        // symlinked folder, possibly put there after the file list was
        // made) could lead out of the project.
        if !std::fs::canonicalize(file).is_ok_and(|real| real == file) {
            return Vec::new();
        }
        let Ok(handle) = std::fs::File::open(file) else {
            return Vec::new();
        };
        if !handle
            .metadata()
            .is_ok_and(|m| m.is_file() && m.len() <= MAX_FILE_BYTES)
        {
            return Vec::new();
        }
        let mut lines = Lines {
            matcher: &self.plan.matcher,
            found: Vec::new(),
            binary: false,
            stop: || self.stopped(full),
        };
        let searched = searcher.search_file(&self.plan.matcher, &handle, &mut lines);
        if searched.is_err() || lines.binary {
            return Vec::new();
        }
        lines.found
    }

    /// Batches what the workers find until they are all done; stops them at
    /// the line limit.
    fn collect(&self, rx: &mpsc::Receiver<FileMatches>, full: &AtomicBool) -> Totals {
        let mut totals = Totals::default();
        let mut batch: Vec<FileMatches> = Vec::new();
        let mut batch_lines = 0;
        let mut since: Option<Instant> = None;
        let limit = MAX_SEARCH_LINES as usize;
        loop {
            let received = match since {
                Some(start) => rx.recv_timeout(BATCH_WAIT.saturating_sub(start.elapsed())),
                None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(mut found) => {
                    let room = limit - totals.lines as usize;
                    totals.truncated |= found.lines.len() > room;
                    found.lines.truncate(room);
                    if found.lines.len() == room {
                        full.store(true, Ordering::Relaxed);
                    }
                    if found.lines.is_empty() {
                        continue;
                    }
                    totals.files += 1;
                    totals.lines += u32::try_from(found.lines.len()).unwrap_or(u32::MAX);
                    batch_lines += found.lines.len();
                    batch.push(found);
                    since.get_or_insert_with(Instant::now);
                    if batch_lines < BATCH_LINES {
                        continue;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            self.flush(&mut batch);
            batch_lines = 0;
            since = None;
        }
        self.flush(&mut batch);
        totals
    }

    fn flush(&self, batch: &mut Vec<FileMatches>) {
        if !batch.is_empty() && !self.cancel.is_cancelled() {
            self.sink.emit(SearchEvent::Results {
                id: self.id,
                files: std::mem::take(batch),
            });
        }
        batch.clear();
    }
}

#[derive(Debug, Default)]
struct Totals {
    files: u32,
    lines: u32,
    truncated: bool,
}

/// Collects one file's matching lines.
struct Lines<'a, F> {
    matcher: &'a RegexMatcher,
    found: Vec<LineMatch>,
    binary: bool,
    stop: F,
}

impl<F: Fn() -> bool> Sink for Lines<'_, F> {
    type Error = io::Error;

    fn matched(&mut self, _: &Searcher, found: &SinkMatch<'_>) -> io::Result<bool> {
        let bytes = found.bytes();
        let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
        let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
        let text = String::from_utf8_lossy(bytes);
        let mut ranges = Vec::new();
        self.matcher
            .find_iter(text.as_bytes(), |m| {
                if !m.is_empty() {
                    ranges.push((m.start(), m.end()));
                }
                true
            })
            .map_err(io::Error::other)?;
        if !ranges.is_empty() {
            let line = found
                .line_number()
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0);
            self.found.push(line_match(line, &text, &ranges));
        }
        // One line past the limit tells the collector that lines were left out.
        Ok(self.found.len() <= MAX_SEARCH_LINES as usize && !(self.stop)())
    }

    fn binary_data(&mut self, _: &Searcher, _: u64) -> io::Result<bool> {
        self.binary = true;
        Ok(false)
    }
}

fn utf16_len(text: &str) -> u32 {
    u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX)
}

/// The line, or a window of a long one around its first match, with the
/// matches as UTF-16 offsets into it. `ranges` are byte ranges in `text`.
fn line_match(line: u32, text: &str, ranges: &[(usize, usize)]) -> LineMatch {
    let (mut start, mut end) = (0, text.len());
    if text.len() > MAX_TEXT {
        start = ranges[0].0.saturating_sub(BEFORE_MATCH);
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        end = (start + MAX_TEXT).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
    }
    let window = &text[start..end];
    let offset = |byte: usize| {
        let mut at = byte.clamp(start, end) - start;
        while !window.is_char_boundary(at) {
            at -= 1;
        }
        utf16_len(&window[..at])
    };
    LineMatch {
        line,
        text: window.to_owned(),
        column: utf16_len(&text[..start]),
        ranges: ranges
            .iter()
            .filter(|(from, to)| *from < end && *to > start)
            .map(|&(from, to)| [offset(from), offset(to)])
            .collect(),
    }
}

#[cfg(test)]
mod tests;
