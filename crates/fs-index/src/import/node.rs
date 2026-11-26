//! The file system calls an import makes, by handle where the OS allows.
//!
//! On Unix every entry is reached from its parent folder's open handle
//! (`openat` with `O_NOFOLLOW`), never by a path that is resolved again, so
//! a folder swapped for a link while the import runs is not followed: not
//! in the dropped tree, not in the target folder, and not in a folder the
//! import just made. Elsewhere the calls take paths.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io;
use std::path::Path;

use ovra_core::path::RelPath;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    File,
    Dir,
    Link,
    Other,
}

/// An entry as it was looked at, links not followed.
pub(super) struct Meta {
    pub(super) kind: Kind,
    pub(super) len: u64,
    /// What a copy gets: the source's bits up to 0o755 on Unix.
    pub(super) perms: fs::Permissions,
    #[cfg(unix)]
    id: (u64, u64),
}

/// The parent folder of a dropped `source` and the source's name in it.
pub(super) fn parent_of(source: &Path) -> io::Result<(Dir, OsString)> {
    let parent = source.parent().unwrap_or(source);
    let name = source.file_name().unwrap_or_default().to_owned();
    Ok((Dir::open(parent)?, name))
}

#[cfg(unix)]
pub(super) use self::unix::*;

#[cfg(unix)]
mod unix {
    use std::os::fd::{AsFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;

    use rustix::fs::{AtFlags, FileType, Mode, OFlags, Stat};

    use super::*;

    /// An open folder.
    pub(in crate::import) struct Dir(OwnedFd);

    const DIR: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);

    fn meta(stat: &Stat) -> Meta {
        let kind = match FileType::from_raw_mode(stat.st_mode) {
            FileType::RegularFile => Kind::File,
            FileType::Directory => Kind::Dir,
            FileType::Symlink => Kind::Link,
            _ => Kind::Other,
        };
        // `as`: the field types differ between platforms (`st_mode` is a
        // `u16` on macOS).
        #[allow(clippy::unnecessary_cast)]
        Meta {
            kind,
            len: u64::try_from(stat.st_size).unwrap_or(0),
            perms: fs::Permissions::from_mode(stat.st_mode as u32 & 0o755),
            id: (stat.st_dev as u64, stat.st_ino as u64),
        }
    }

    impl Dir {
        /// A folder by path; the path's links are followed.
        pub(in crate::import) fn open(path: &Path) -> io::Result<Self> {
            let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
            Ok(Self(rustix::fs::open(path, flags, Mode::empty())?))
        }
    }

    /// The folder `into` under the (resolved) `root`, refused with
    /// `None` when any part of `into` is a link.
    pub(in crate::import) fn target(root: &Path, into: &RelPath) -> io::Result<Option<Dir>> {
        let mut dir = Dir::open(root)?;
        for name in into.as_str().split('/').filter(|n| !n.is_empty()) {
            if stat(&dir, OsStr::new(name))?.kind == Kind::Link {
                return Ok(None);
            }
            dir = Dir(rustix::fs::openat(&dir.0, name, DIR, Mode::empty())?);
        }
        Ok(Some(dir))
    }

    pub(in crate::import) fn stat(dir: &Dir, name: &OsStr) -> io::Result<Meta> {
        Ok(meta(&rustix::fs::statat(
            &dir.0,
            name,
            AtFlags::SYMLINK_NOFOLLOW,
        )?))
    }

    /// Opens `name` without following a link and refuses it unless it is
    /// still the entry `was` describes.
    fn open_unchanged(dir: &Dir, name: &OsStr, was: &Meta, flags: OFlags) -> io::Result<OwnedFd> {
        let fd = rustix::fs::openat(&dir.0, name, flags, Mode::empty())?;
        if meta(&rustix::fs::fstat(&fd)?).id != was.id {
            return Err(io::Error::other("changed while it was copied"));
        }
        Ok(fd)
    }

    pub(in crate::import) fn open_dir(dir: &Dir, name: &OsStr, was: &Meta) -> io::Result<Dir> {
        open_unchanged(dir, name, was, DIR).map(Dir)
    }

    /// Non-blocking, so a FIFO swapped in does not hang.
    pub(in crate::import) fn open_file(dir: &Dir, name: &OsStr, was: &Meta) -> io::Result<File> {
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
        open_unchanged(dir, name, was, flags).map(File::from)
    }

    /// The names in `dir`, read from its handle.
    pub(in crate::import) fn names(dir: &Dir) -> io::Result<Vec<OsString>> {
        let mut names = Vec::new();
        for entry in rustix::fs::Dir::read_from(dir.0.as_fd())? {
            let entry = entry?;
            let name = entry.file_name().to_bytes();
            if name != b"." && name != b".." {
                names.push(<OsStr as OsStrExt>::from_bytes(name).to_owned());
            }
        }
        Ok(names)
    }

    /// A new folder, only the owner's until `set_perms`.
    pub(in crate::import) fn make_dir(dir: &Dir, name: &OsStr) -> io::Result<Dir> {
        rustix::fs::mkdirat(&dir.0, name, Mode::RWXU)?;
        Ok(Dir(rustix::fs::openat(&dir.0, name, DIR, Mode::empty())?))
    }

    pub(in crate::import) fn set_perms(dir: &Dir, perms: &fs::Permissions) -> io::Result<()> {
        let mode = Mode::from_raw_mode(perms.mode() as rustix::fs::RawMode);
        Ok(rustix::fs::fchmod(&dir.0, mode)?)
    }

    /// A new file, never over an existing entry or through a link.
    pub(in crate::import) fn make_file(dir: &Dir, name: &OsStr) -> io::Result<File> {
        let flags =
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let fd = rustix::fs::openat(&dir.0, name, flags, Mode::RUSR | Mode::WUSR)?;
        Ok(File::from(fd))
    }

    pub(in crate::import) fn remove_file(dir: &Dir, name: &OsStr) -> io::Result<()> {
        Ok(rustix::fs::unlinkat(&dir.0, name, AtFlags::empty())?)
    }
}

#[cfg(not(unix))]
pub(super) use self::paths::*;

/// Windows: the same calls by path, as the import made them before.
#[cfg(not(unix))]
mod paths {
    use std::fs::OpenOptions;
    use std::path::PathBuf;

    use super::*;

    pub(in crate::import) struct Dir(PathBuf);

    impl Dir {
        pub(in crate::import) fn open(path: &Path) -> io::Result<Self> {
            Ok(Self(path.to_path_buf()))
        }
    }

    pub(in crate::import) fn target(root: &Path, into: &RelPath) -> io::Result<Option<Dir>> {
        Ok(Some(Dir(into.under(root))))
    }

    pub(in crate::import) fn stat(dir: &Dir, name: &OsStr) -> io::Result<Meta> {
        let meta = fs::symlink_metadata(dir.0.join(name))?;
        let kind = meta.file_type();
        let kind = if kind.is_symlink() {
            Kind::Link
        } else if kind.is_dir() {
            Kind::Dir
        } else if kind.is_file() {
            Kind::File
        } else {
            Kind::Other
        };
        Ok(Meta {
            kind,
            len: meta.len(),
            perms: meta.permissions(),
        })
    }

    pub(in crate::import) fn open_dir(dir: &Dir, name: &OsStr, _: &Meta) -> io::Result<Dir> {
        Ok(Dir(dir.0.join(name)))
    }

    pub(in crate::import) fn open_file(dir: &Dir, name: &OsStr, _: &Meta) -> io::Result<File> {
        File::open(dir.0.join(name))
    }

    pub(in crate::import) fn names(dir: &Dir) -> io::Result<Vec<OsString>> {
        fs::read_dir(&dir.0)?
            .map(|entry| entry.map(|e| e.file_name()))
            .collect()
    }

    pub(in crate::import) fn make_dir(dir: &Dir, name: &OsStr) -> io::Result<Dir> {
        let path = dir.0.join(name);
        fs::create_dir(&path)?;
        Ok(Dir(path))
    }

    pub(in crate::import) fn set_perms(dir: &Dir, perms: &fs::Permissions) -> io::Result<()> {
        fs::set_permissions(&dir.0, perms.clone())
    }

    pub(in crate::import) fn make_file(dir: &Dir, name: &OsStr) -> io::Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.0.join(name))
    }

    pub(in crate::import) fn remove_file(dir: &Dir, name: &OsStr) -> io::Result<()> {
        fs::remove_file(dir.0.join(name))
    }
}
