//! `search.*`: searching the contents of a project's files. A search runs in
//! the background: `search.start` answers at once with an id, results stream
//! in as `search.results` events, and one `search.done` ends it.

use serde::{Deserialize, Serialize};

use super::projects::ProjectId;

/// Assigned by the core when a search starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SearchId(pub u64);

/// What to look for, and in which files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    /// Text to find on one line; a regular expression with `regex`. Must not
    /// be empty.
    pub pattern: String,
    /// Rust `regex` syntax (as in ripgrep) instead of literal text.
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub case_sensitive: bool,
    /// Only matches that start and end at a word boundary.
    #[serde(default)]
    pub whole_word: bool,
    /// Globs over project-relative paths; when not empty, only files that
    /// match one are searched. A glob without `/` matches a file or folder
    /// name at any depth (`*.rs`, `src`); otherwise it matches from the root
    /// (`crates/*/src`). A folder matches everything inside it.
    #[serde(default)]
    pub include: Vec<String>,
    /// Globs like `include`; files that match one are skipped.
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum SearchRequest {
    /// Searches the project's files that `fs.list_files` lists, skipping
    /// binary files and symlinks. Answers `search.started` at once, or
    /// `bad_request` for an empty pattern or an invalid regex or glob.
    #[serde(rename = "search.start")]
    Start {
        project: ProjectId,
        query: SearchQuery,
    },
    /// Stops a search; a `search.done` with `cancelled` follows unless it
    /// already ended. Answers `ok`, also for a search that ended.
    #[serde(rename = "search.cancel")]
    Cancel { id: SearchId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SearchResponse {
    #[serde(rename = "search.started")]
    Started { id: SearchId },
}

/// The most matching lines one search reports; the rest are counted as
/// `truncated`.
pub const MAX_SEARCH_LINES: u32 = 10_000;

/// Matching lines of one file, in line order. Each file appears in at most
/// one `search.results` event of a search; files arrive in no fixed order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMatches {
    pub path: String,
    pub lines: Vec<LineMatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineMatch {
    /// From 1.
    pub line: u32,
    /// The line without its line break. A long line is cut to a window
    /// around its first match; `column` says where the window starts.
    pub text: String,
    /// UTF-16 offset in the full line at which `text` starts (0 unless the
    /// line was cut).
    pub column: u32,
    /// Matches as `[start, end)` UTF-16 offsets into `text`, in order.
    pub ranges: Vec<[u32; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SearchEvent {
    /// More results of a running search.
    #[serde(rename = "search.results")]
    Results {
        id: SearchId,
        files: Vec<FileMatches>,
    },
    /// The search ended. Counts cover what was reported. `truncated`: it
    /// stopped at [`MAX_SEARCH_LINES`] lines and more files may match.
    #[serde(rename = "search.done")]
    Done {
        id: SearchId,
        files: u32,
        lines: u32,
        truncated: bool,
        cancelled: bool,
    },
}
