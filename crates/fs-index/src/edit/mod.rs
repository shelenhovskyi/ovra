//! Creating, renaming, copying and trashing files and folders.
//!
//! Every path is placed by its parent: the parent must resolve inside the
//! canonical root and outside any `.git` folder, and the last component is
//! used as it is, never followed, so renaming, copying or trashing a symlink
//! acts on the link and not on its target. A path that must be new is
//! refused with `conflict` if anything, even a dangling link, has its name.
//!
//! The checks run just before the change. A parent that another process
//! swaps for a symlink in between is not detected, as elsewhere in this
//! crate (`docs/specs/security.md`, "File operations"). The last component
//! is never followed or replaced: files are created with `create_new`,
//! renames refuse to replace where the OS can, and a copy opens each source
//! without following links and refuses one that changed since it was looked
//! at.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use ovra_core::path::RelPath;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;

use crate::resolve;

type TrashFn = dyn Fn(&Path) -> std::result::Result<(), String> + Send + Sync;

/// Moves one absolute path to the system trash. Tests swap it for a fake,
/// so they never touch the real trash.
#[derive(Clone)]
pub(crate) struct Trash {
    delete: Arc<TrashFn>,
    /// The home trash folder (freedesktop). An item on another drive is
    /// refused: `trash` would create `.Trash-$uid` at that drive's top,
    /// which may be the project itself.
    home: Option<PathBuf>,
}

impl Trash {
    #[cfg(test)]
    pub(crate) fn fake(
        f: impl Fn(&Path) -> std::result::Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            delete: Arc::new(f),
            home: None,
        }
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn with_home(self, home: PathBuf) -> Self {
        Self {
            home: Some(home),
            ..self
        }
    }
}

impl Default for Trash {
    fn default() -> Self {
        Self {
            delete: Arc::new(system_trash),
            home: home_trash(),
        }
    }
}

/// `trash` resolves only the parent, so a symlink itself is trashed. On
/// macOS it goes through `NSFileManager`, not a Finder script that may ask
/// for permission.
#[cfg(target_os = "macos")]
fn system_trash(path: &Path) -> std::result::Result<(), String> {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    let mut context = trash::TrashContext::default();
    context.set_delete_method(DeleteMethod::NsFileManager);
    context.delete(path).map_err(|e| e.to_string())
}

#[cfg(not(target_os = "macos"))]
fn system_trash(path: &Path) -> std::result::Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

/// `$XDG_DATA_HOME/Trash`, else `~/.local/share/Trash`.
#[cfg(all(unix, not(target_os = "macos")))]
fn home_trash() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/share")))?;
    Some(data.join("Trash"))
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn home_trash() -> Option<PathBuf> {
    None
}

/// The device `path` is on; with `follow`, a link's target's.
#[cfg(unix)]
fn device(path: &Path, follow: bool) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    let meta = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    };
    meta.ok().map(|m| m.dev())
}

#[cfg(not(unix))]
fn device(_: &Path, _: bool) -> Option<u64> {
    None
}

/// Refuses an item that is not on the home trash's drive (the trash
/// folder, or the nearest folder above it that exists).
fn on_home_drive(home: &Path, path: &RelPath, full: &Path) -> Result<()> {
    let home = home.ancestors().find_map(|p| device(p, true));
    match (home, device(full, false)) {
        (Some(home), Some(item)) if home != item => Err(ProtocolError::new(
            ErrorCode::Unavailable,
            format!("{path}: cannot move to Trash on this drive"),
        )),
        _ => Ok(()),
    }
}

impl fmt::Debug for Trash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Trash")
    }
}

fn io_error(path: &RelPath, err: &io::Error) -> ProtocolError {
    match err.kind() {
        io::ErrorKind::AlreadyExists => conflict(path),
        io::ErrorKind::Unsupported => {
            ProtocolError::new(ErrorCode::BadRequest, format!("{path}: {err}"))
        }
        _ => resolve::io_error(path, err),
    }
}

fn conflict(path: &RelPath) -> ProtocolError {
    ProtocolError::new(ErrorCode::Conflict, format!("{path} already exists"))
}

fn forbidden(path: &RelPath, why: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::Forbidden, format!("{path} {why}"))
}

/// The parent of `path` and its last component.
fn split(path: &RelPath) -> Result<(RelPath, &str)> {
    let (parent, name) = path
        .as_str()
        .rsplit_once('/')
        .unwrap_or(("", path.as_str()));
    if name.is_empty() {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "name a file or folder",
        ));
    }
    Ok((RelPath::parse(parent)?, name))
}

/// Refuses a resolved parent outside the root or inside a `.git` folder
/// (reached through a symlink: the core refuses `.git` in the path itself).
pub(crate) fn check_parent(root: &Path, real_parent: &Path, path: &RelPath) -> Result<()> {
    let root = resolve::root(root)?;
    let Ok(rest) = real_parent.strip_prefix(&root) else {
        return Err(forbidden(path, "points outside the project"));
    };
    let in_git = rest
        .components()
        .any(|c| matches!(c, Component::Normal(n) if n.eq_ignore_ascii_case(".git")));
    if in_git {
        return Err(forbidden(path, "is inside .git"));
    }
    Ok(())
}

/// An existing `path`, by its resolved parent; the path itself is not
/// followed.
fn existing(root: &Path, path: &RelPath) -> Result<PathBuf> {
    let (parent, name) = split(path)?;
    let (_, real_parent) = resolve::existing(root, &parent).map_err(|e| match e.code {
        ErrorCode::Forbidden => forbidden(path, "points outside the project"),
        _ => e,
    })?;
    check_parent(root, &real_parent, path)?;
    let full = real_parent.join(name);
    fs::symlink_metadata(&full).map_err(|e| io_error(path, &e))?;
    Ok(full)
}

/// Where a new `path` goes. Its parent's deepest existing ancestor resolves
/// inside the root, so the missing ones may be created there.
fn vacant(root: &Path, path: &RelPath) -> Result<PathBuf> {
    let (parent, name) = split(path)?;
    let dir = resolve::for_write(root, &parent).map_err(|e| match e.code {
        ErrorCode::Forbidden => forbidden(path, "points outside the project"),
        _ => e,
    })?;
    check_parent(root, &dir, path)?;
    let full = dir.join(name);
    match fs::symlink_metadata(&full) {
        Ok(_) => Err(conflict(path)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(full),
        Err(e) => Err(io_error(path, &e)),
    }
}

fn make_parents(target: &Path, path: &RelPath) -> Result<()> {
    match target.parent() {
        Some(dir) => fs::create_dir_all(dir).map_err(|e| io_error(path, &e)),
        None => Ok(()),
    }
}

/// Refuses `target` inside the folder `source`, also when a symlink in
/// `target`'s parents leads there.
pub(crate) fn not_inside(source: &Path, target: &Path) -> Result<()> {
    if target.starts_with(source) {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "cannot put a folder inside itself",
        ));
    }
    Ok(())
}

pub(crate) fn create(root: &Path, path: &RelPath, dir: bool) -> Result<()> {
    let target = vacant(root, path)?;
    make_parents(&target, path)?;
    let made = if dir {
        fs::create_dir(&target)
    } else {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map(drop)
    };
    made.map_err(|e| io_error(path, &e))
}

/// A new file holding `bytes`. `create_new` refuses any entry at
/// `target`, a link included, and `vacant` a parent that leaves `root`.
pub(crate) fn create_file(root: &Path, path: &RelPath, bytes: &[u8]) -> Result<()> {
    use std::io::Write as _;
    let target = vacant(root, path)?;
    make_parents(&target, path)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|e| match e.kind() {
            io::ErrorKind::AlreadyExists => conflict(path),
            _ => io_error(path, &e),
        })?;
    file.write_all(bytes).map_err(|e| io_error(path, &e))
}

pub(crate) fn rename(root: &Path, from: &RelPath, to: &RelPath) -> Result<()> {
    let source = existing(root, from)?;
    let target = vacant(root, to)?;
    not_inside(&source, &target)?;
    make_parents(&target, to)?;
    rename_new(&source, &target).map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => conflict(to),
        _ => io_error(from, &e),
    })
}

/// Renames without replacing: `renameat2(RENAME_NOREPLACE)` on Linux,
/// `renamex_np(RENAME_EXCL)` on macOS. Elsewhere, or on a file system
/// without it, a plain rename follows `vacant`'s check, so a file created
/// at `target` in between would be replaced.
fn rename_new(source: &Path, target: &Path) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
    {
        use rustix::fs::{CWD, RenameFlags, renameat_with};
        use rustix::io::Errno;
        match renameat_with(CWD, source, CWD, target, RenameFlags::NOREPLACE) {
            Err(Errno::INVAL | Errno::NOSYS | Errno::NOTSUP) => {}
            done => return done.map_err(io::Error::from),
        }
    }
    fs::rename(source, target)
}

pub(crate) fn copy(root: &Path, from: &RelPath, to: &RelPath) -> Result<()> {
    let source = existing(root, from)?;
    let target = vacant(root, to)?;
    not_inside(&source, &target)?;
    make_parents(&target, to)?;
    // A failed copy leaves what was copied so far; nothing is deleted.
    copy_entry(&source, &target).map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => conflict(to),
        _ => io_error(from, &e),
    })
}

/// Copies a file, a link (as a link) or a folder tree, never following links.
fn copy_entry(from: &Path, to: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    let kind = meta.file_type();
    if kind.is_symlink() {
        copy_link(from, to)
    } else if kind.is_dir() {
        check_dir(from, &meta)?;
        fs::create_dir(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_entry(&entry.path(), &to.join(entry.file_name()))?;
        }
        // Last, so a read-only folder still gets its children.
        fs::set_permissions(to, meta.permissions())
    } else if kind.is_file() {
        let mut source = open_file(from, &meta)?;
        let mut target = OpenOptions::new().write(true).create_new(true).open(to)?;
        io::copy(&mut source, &mut target)?;
        target.set_permissions(meta.permissions())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("{} is not a file, folder or link", from.display()),
        ))
    }
}

/// Opens `path` without following a link, non-blocking (a FIFO swapped in
/// does not hang), and refuses it unless it is still the entry `meta`
/// describes.
#[cfg(unix)]
fn open_unchanged(path: &Path, meta: &fs::Metadata, dir: bool) -> io::Result<File> {
    use std::os::unix::fs::MetadataExt;

    use rustix::fs::{Mode, OFlags};
    let kind = if dir {
        OFlags::DIRECTORY
    } else {
        OFlags::NONBLOCK
    };
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | kind;
    let file = File::from(rustix::fs::open(path, flags, Mode::empty())?);
    let now = file.metadata()?;
    if (now.dev(), now.ino()) != (meta.dev(), meta.ino()) {
        return Err(io::Error::other(format!(
            "{} changed while it was copied",
            path.display()
        )));
    }
    Ok(file)
}

/// The file `meta` describes, opened for reading.
#[cfg(unix)]
pub(crate) fn open_file(path: &Path, meta: &fs::Metadata) -> io::Result<File> {
    open_unchanged(path, meta, false)
}

/// Refuses a folder swapped (for a link, say) since `meta` was read, just
/// before it is listed.
#[cfg(unix)]
pub(crate) fn check_dir(path: &Path, meta: &fs::Metadata) -> io::Result<()> {
    open_unchanged(path, meta, true).map(drop)
}

#[cfg(not(unix))]
pub(crate) fn open_file(path: &Path, _: &fs::Metadata) -> io::Result<File> {
    File::open(path)
}

#[cfg(not(unix))]
pub(crate) fn check_dir(_: &Path, _: &fs::Metadata) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn copy_link(from: &Path, to: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(from)?, to)
}

/// Windows needs to know whether a link is to a file or a folder, and a
/// privilege to make one; copying links is left out there.
#[cfg(not(unix))]
fn copy_link(from: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "{} is a symlink, which Ovra cannot copy here",
            from.display()
        ),
    ))
}

/// Checks every path, then trashes them in order, stopping at the first
/// failure. A path inside (or equal to) another one is trashed with it.
pub(crate) fn trash(root: &Path, paths: &[RelPath], trash: &Trash) -> Result<()> {
    let mut found: Vec<(&RelPath, PathBuf)> = Vec::new();
    for path in paths {
        let full = existing(root, path)?;
        if let Some(home) = &trash.home {
            on_home_drive(home, path, &full)?;
        }
        if found.iter().any(|(_, kept)| full.starts_with(kept)) {
            continue;
        }
        found.retain(|(_, kept)| !kept.starts_with(&full));
        found.push((path, full));
    }
    for (path, full) in &found {
        (trash.delete)(full).map_err(|e| {
            ProtocolError::new(
                ErrorCode::Internal,
                format!("cannot move {path} to the trash: {e}"),
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
