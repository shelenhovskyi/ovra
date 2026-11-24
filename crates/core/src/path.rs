//! Project-relative paths.
//!
//! Every path from a front end or an agent is checked here before it reaches a
//! service, so nothing can address a file outside its project by `..`,
//! absolute paths or drive prefixes. Services must additionally refuse to
//! follow symlinks that point outside the root.
//!
//! On Windows a drive is refused in every segment: `PathBuf::push("C:a")`
//! replaces the whole path with the drive-relative `C:a`, which resolves
//! against the process's current directory, so `src/C:a` would leave the
//! project once [`RelPath::under`] joins it. Such names cannot exist on
//! Windows anyway. Elsewhere `c:x` is an ordinary name, kept so that a review
//! with such a file can still be decided.
//!
//! On Windows, names Windows cannot store or reads as something else are
//! refused too: device names (`CON`, `nul.txt`, `COM1`), names ending in a
//! dot or a space (Windows drops those, so `a.` is `a` and `.git.` is
//! `.git`) and the characters `<>"|?*` and control characters. These are
//! the rules Git for Windows applies to paths in a repository.

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
    /// segments are dropped; `..`, absolute paths, backslashes, NUL bytes, a
    /// drive (`C:`) at the start of any segment on Windows, a name Windows
    /// cannot store as written (see the module docs) on Windows, and any `:`
    /// in the first segment (a scheme such as `file:`) are rejected. Other colons
    /// are kept: they are valid in names on Linux and macOS.
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
                s if cfg!(windows) && is_drive(s) => return forbidden("names a drive"),
                s if cfg!(windows) && !is_win32_name(s) => {
                    return forbidden("is not a name Windows can store as written");
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

/// Whether Windows reads `segment` as starting with a drive: an ASCII letter
/// and a colon (`C:`, `c:x`). Any other prefix needs a backslash, which
/// [`RelPath::parse`] refuses.
fn is_drive(segment: &str) -> bool {
    matches!(segment.as_bytes(), [letter, b':', ..] if letter.is_ascii_alphabetic())
}

/// Whether Windows stores `segment` under exactly that name: not a device
/// name, not ending in a dot or a space, and no `<>"|?*` or control
/// characters. `:` is handled by the callers (drives and streams).
fn is_win32_name(segment: &str) -> bool {
    if segment.ends_with(['.', ' '])
        || segment
            .chars()
            .any(|c| c.is_control() || "<>\"|?*".contains(c))
    {
        return false;
    }
    // A device name is reserved with any extension and with spaces before
    // it: `nul`, `NUL.txt`, `con .md`, `COM1`, `lpt\u{b9}`, `CONIN$`.
    let stem = segment
        .split(['.', ':'])
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    let numbered = |prefix: &str| {
        upper.strip_prefix(prefix).is_some_and(|n| {
            matches!(
                n,
                "1" | "2"
                    | "3"
                    | "4"
                    | "5"
                    | "6"
                    | "7"
                    | "8"
                    | "9"
                    | "\u{b9}"
                    | "\u{b2}"
                    | "\u{b3}"
            )
        })
    };
    let device = matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    );
    !(device || numbered("COM") || numbered("LPT"))
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
        for raw in ["notes/ab:c.md", "notes/10:30", "notes/:x", "a/b.rs:x"] {
            assert!(RelPath::parse(raw).is_ok(), "{raw:?}");
        }
    }

    #[test]
    fn a_drive_in_any_segment_is_refused_where_it_is_a_drive() {
        for raw in ["src/C:a", "src/c:", "a/b/Z:/x"] {
            let err = RelPath::parse(raw).err().map(|e| e.code);
            let expected = cfg!(windows).then_some(ErrorCode::Forbidden);
            assert_eq!(err, expected, "{raw:?}");
        }
        assert!(is_drive("C:a") && is_drive("z:") && !is_drive("ab:c") && !is_drive("1:x"));
        let joined = RelPath::parse("a").and_then(|p| p.join("C:x"));
        assert_eq!(joined.is_err(), cfg!(windows));
    }

    #[test]
    fn names_windows_cannot_store_are_refused_there() {
        let bad = [
            "CON",
            "con",
            "nul.txt",
            "Aux.tar.gz",
            "con .md",
            "COM1",
            "lpt9.log",
            "COM\u{b9}",
            "conin$",
            "CONOUT$.x",
            "a.",
            "a ",
            ".git.",
            ".. ",
            "a?b",
            "a*",
            "a<b",
            "a|b",
            "a\"b",
            "a\u{1}b",
        ];
        for name in bad {
            assert!(!is_win32_name(name), "{name:?}");
            let parsed = RelPath::parse(&format!("src/{name}")).err().map(|e| e.code);
            assert_eq!(
                parsed,
                cfg!(windows).then_some(ErrorCode::Forbidden),
                "{name:?}"
            );
        }
        for name in [
            "console", "COM10", "COM0", "nul-x", "a.b", ".git", "aux_", "LPT", "a b",
        ] {
            assert!(is_win32_name(name), "{name:?}");
            assert!(RelPath::parse(&format!("src/{name}")).is_ok(), "{name:?}");
        }
    }

    #[test]
    fn under_and_join() {
        let p = RelPath::parse("a").and_then(|p| p.join("b.txt"));
        assert_eq!(p.as_ref().map(RelPath::as_str), Ok("a/b.txt"));
        let abs = p.map(|p| p.under(Path::new("/r")));
        assert_eq!(abs, Ok(PathBuf::from("/r/a/b.txt")));
    }
}
