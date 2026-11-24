//! Project-relative paths.
//!
//! Every path from a front end or an agent is checked here before it reaches a
//! service, so nothing can address a file outside its project by `..`,
//! absolute paths or drive prefixes. Services must additionally refuse to
//! follow symlinks that point outside the root.

use std::path::{Path, PathBuf};

use crate::protocol::{ErrorCode, ProtocolError};

/// A normalized path relative to a project root, with `/` separators.
/// The empty path is the root itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RelPath(String);

impl RelPath {
    /// The project root.
    pub fn root() -> Self {
        Self(String::new())
    }

    /// Parses and normalizes a client-supplied path. `.` segments and empty
    /// segments are dropped; `..`, absolute paths, backslashes, drive
    /// prefixes and NUL bytes are rejected.
    pub fn parse(raw: &str) -> Result<Self, ProtocolError> {
        let forbidden = |why: &str| {
            Err(ProtocolError::new(
                ErrorCode::Forbidden,
                format!("path {raw:?} {why}"),
            ))
        };
        if raw.starts_with('/') {
            return forbidden("is absolute");
        }
        if raw.contains('\\') || raw.contains('\0') {
            return forbidden("contains a backslash or NUL");
        }
        let mut parts = Vec::new();
        for segment in raw.split('/') {
            match segment {
                "" | "." => {}
                ".." => return forbidden("leaves the project"),
                s if s.contains(':') && parts.is_empty() => {
                    return forbidden("has a drive or scheme prefix");
                }
                s => parts.push(s),
            }
        }
        Ok(Self(parts.join("/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// The absolute path under `root`.
    pub fn under(&self, root: &Path) -> PathBuf {
        let mut path = root.to_path_buf();
        for part in self.0.split('/').filter(|p| !p.is_empty()) {
            path.push(part);
        }
        path
    }

    /// The child `name` of this directory.
    pub fn join(&self, name: &str) -> Result<Self, ProtocolError> {
        if self.is_root() {
            Self::parse(name)
        } else {
            Self::parse(&format!("{}/{name}", self.0))
        }
    }
}

impl std::fmt::Display for RelPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(RelPath::parse("").map(|p| p.0), Ok(String::new()));
        assert_eq!(RelPath::parse("./a//b/.").map(|p| p.0), Ok("a/b".into()));
        assert_eq!(
            RelPath::parse("src/main.rs").map(|p| p.0),
            Ok("src/main.rs".into())
        );
    }

    #[test]
    fn rejects_escapes() {
        for raw in [
            "..",
            "a/../../b",
            "/etc/passwd",
            "C:/x",
            "a\\b",
            "a\0b",
            "file:x",
        ] {
            let err = RelPath::parse(raw).err().map(|e| e.code);
            assert_eq!(err, Some(ErrorCode::Forbidden), "{raw:?}");
        }
    }

    #[test]
    fn colon_is_fine_after_first_segment() {
        assert!(RelPath::parse("notes/a:b.md").is_ok());
    }

    #[test]
    fn under_and_join() {
        let p = RelPath::parse("a").and_then(|p| p.join("b.txt"));
        assert_eq!(p.as_ref().map(RelPath::as_str), Ok("a/b.txt"));
        let abs = p.map(|p| p.under(Path::new("/r")));
        assert_eq!(abs, Ok(PathBuf::from("/r/a/b.txt")));
    }
}
