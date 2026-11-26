//! The size of a worktree on disk (`docs/specs/worktree-setup.md`, "Disk
//! use").

use std::fs::{self, Metadata};
use std::path::Path;
use std::time::{Duration, Instant};

use ovra_core::protocol::orchestration::DiskUsage;
use ovra_core::services::Result;

/// Adds up what the files under `dir` take on disk, never following a
/// link, for at most `budget`. Cloned blocks count in full, so a complete
/// count is an upper bound; one cut short is a lower bound.
pub(crate) fn usage(dir: &Path, budget: Duration) -> DiskUsage {
    let started = Instant::now();
    let mut usage = DiskUsage {
        bytes: 0,
        complete: true,
    };
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        if started.elapsed() >= budget {
            usage.complete = false;
            break;
        }
        // A folder that went or cannot be read counts as empty.
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            usage.bytes += on_disk(&meta);
            if meta.is_dir() {
                folders.push(entry.path());
            }
        }
    }
    usage
}

/// Bytes an unprivileged process may still write on the volume that holds
/// `dir`: the blocks free for non-root users (`f_bavail`), not the ones
/// kept for root.
#[cfg(unix)]
pub(crate) fn free_space(dir: &Path) -> Result<u64> {
    let stat = rustix::fs::statvfs(dir).map_err(|e| {
        crate::git::internal(format!("cannot ask {} for free space: {e}", dir.display()))
    })?;
    Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
}

#[cfg(unix)]
fn on_disk(meta: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt as _;
    meta.blocks() * 512
}

#[cfg(not(unix))]
fn on_disk(meta: &Metadata) -> u64 {
    meta.len()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{free_space, usage};
    use crate::tests::put;

    #[test]
    fn the_size_counts_every_file_until_the_time_is_up() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let dir = tmp.path();
        put(dir, "a.bin", &"a".repeat(10_000));
        put(dir, "deep/er/b.bin", &"b".repeat(20_000));
        #[cfg(unix)]
        std::os::unix::fs::symlink("/usr", dir.join("link")).unwrap_or_else(|e| panic!("{e}"));

        let full = usage(dir, Duration::from_secs(2));
        assert!(full.complete);
        // At least the bytes written; a link to a big folder adds next to
        // nothing.
        assert!((30_000..1 << 20).contains(&full.bytes), "{full:?}");

        let cut = usage(dir, Duration::ZERO);
        assert!(!cut.complete);
        assert!(cut.bytes < full.bytes);
    }

    #[cfg(unix)]
    #[test]
    fn free_space_is_known_for_a_folder_and_an_error_for_none() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let free = free_space(tmp.path()).unwrap_or_else(|e| panic!("{e}"));
        assert!(free > 0);
        let missing = free_space(&tmp.path().join("gone"));
        assert!(missing.is_err());
    }
}
