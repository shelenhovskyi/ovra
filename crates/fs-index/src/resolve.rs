//! Maps project-relative paths to disk paths. Symlinks are followed only
//! while they stay inside the project root.

use std::io;
use std::path::{Path, PathBuf};

use ovra_core::path::RelPath;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;

/// Converts an I/O error about `what` into a protocol error.
pub(crate) fn io_error(what: impl std::fmt::Display, err: &io::Error) -> ProtocolError {
    let code = match err.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => ErrorCode::NotFound,
        _ => ErrorCode::Internal,
    };
    ProtocolError::new(code, format!("{what}: {err}"))
}

fn outside(path: &RelPath) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Forbidden,
        format!("{path} points outside the project"),
    )
}

/// The project root with symlinks resolved.
pub(crate) fn root(root: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(root).map_err(|e| io_error(root.display(), &e))
}

/// Resolves a path that must exist. Returns the canonical root and target.
pub(crate) fn existing(root: &Path, path: &RelPath) -> Result<(PathBuf, PathBuf)> {
    let root = self::root(root)?;
    let real = std::fs::canonicalize(path.under(&root)).map_err(|e| io_error(path, &e))?;
    if !real.starts_with(&root) {
        return Err(outside(path));
    }
    Ok((root, real))
}

/// Resolves a path that may not exist yet: its deepest existing ancestor must
/// resolve inside the root. Missing directories are not created here.
pub(crate) fn for_write(root: &Path, path: &RelPath) -> Result<PathBuf> {
    let root = self::root(root)?;
    let full = path.under(&root);
    let mut existing = full.as_path();
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                missing.push(existing.file_name().unwrap_or_default().to_owned());
                existing = existing.parent().ok_or_else(|| io_error(path, &e))?;
            }
            Err(e) => return Err(io_error(path, &e)),
        }
    }
    // A dangling symlink cannot be resolved, so it is refused.
    let mut real = std::fs::canonicalize(existing).map_err(|_| outside(path))?;
    if !real.starts_with(&root) {
        return Err(outside(path));
    }
    real.extend(missing.iter().rev());
    Ok(real)
}
