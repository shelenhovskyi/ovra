//! Reading and writing single files.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use ovra_core::path::RelPath;
use ovra_core::protocol::fs::{FileBody, FileContent};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;

use crate::resolve::{self, io_error};

/// Files larger than this are answered with [`FileBody::TooLarge`].
pub(crate) const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// Suffix of the temporary files used for atomic writes; the watcher skips them.
pub(crate) const TMP_SUFFIX: &str = ".ovra-tmp";

pub(crate) fn mtime_ms(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn not_a_file(path: &RelPath) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, format!("{path} is a directory"))
}

pub(crate) fn read(root: &Path, path: &RelPath) -> Result<FileContent> {
    let (_, real) = resolve::existing(root, path)?;
    let meta = fs::metadata(&real).map_err(|e| io_error(path, &e))?;
    if meta.is_dir() {
        return Err(not_a_file(path));
    }
    let body = if meta.len() > MAX_FILE_BYTES {
        FileBody::TooLarge
    } else {
        let bytes = fs::read(&real).map_err(|e| io_error(path, &e))?;
        match String::from_utf8(bytes) {
            Ok(text) if !text.contains('\0') => FileBody::Text { text },
            Ok(text) => binary(path, text.as_bytes()),
            Err(e) => binary(path, e.as_bytes()),
        }
    };
    Ok(FileContent {
        path: path.to_string(),
        size: meta.len(),
        mtime_ms: mtime_ms(&meta),
        body,
    })
}

fn binary(path: &RelPath, bytes: &[u8]) -> FileBody {
    FileBody::Binary {
        mime: mime(path.as_str()).to_owned(),
        base64: base64(bytes),
    }
}

fn mime(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        Some("ico") => "image/x-icon",
        Some("pdf") => "application/pdf",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let byte = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let n = (byte(0) << 16) | (byte(1) << 8) | byte(2);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Writes `text` atomically (temporary file + rename), keeping the old
/// file's permissions. Missing parent directories are created. With
/// `expected_mtime_ms`, the file must still exist and have that mtime.
pub(crate) fn write(
    root: &Path,
    path: &RelPath,
    text: &str,
    expected_mtime_ms: Option<u64>,
) -> Result<u64> {
    let target = resolve::for_write(root, path)?;
    let permissions = match fs::metadata(&target) {
        Ok(meta) if meta.is_dir() => return Err(not_a_file(path)),
        Ok(meta) => {
            if expected_mtime_ms.is_some_and(|expected| expected != mtime_ms(&meta)) {
                return Err(ProtocolError::new(
                    ErrorCode::Conflict,
                    format!("{path} changed on disk"),
                ));
            }
            Some(meta.permissions())
        }
        // The client saw this file, so it was deleted or renamed since:
        // writing would silently bring it back.
        Err(e) if e.kind() == io::ErrorKind::NotFound && expected_mtime_ms.is_some() => {
            return Err(ProtocolError::new(
                ErrorCode::Conflict,
                format!("{path} was deleted on disk"),
            ));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(io_error(path, &e)),
    };
    let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
        return Err(not_a_file(path));
    };
    fs::create_dir_all(dir).map_err(|e| io_error(path, &e))?;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let tmp = dir.join(format!(
        ".{}.{}-{}{TMP_SUFFIX}",
        name.to_string_lossy(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let written = fs::write(&tmp, text)
        .and_then(|()| permissions.map_or(Ok(()), |p| fs::set_permissions(&tmp, p)))
        .and_then(|()| fs::rename(&tmp, &target));
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Err(io_error(path, &e));
    }
    let meta = fs::metadata(&target).map_err(|e| io_error(path, &e))?;
    Ok(mtime_ms(&meta))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_4648_vectors() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (input, expected) in cases {
            assert_eq!(base64(input.as_bytes()), expected, "{input:?}");
        }
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn mime_by_extension() {
        assert_eq!(mime("a/b.PNG"), "image/png");
        assert_eq!(mime("photo.jpeg"), "image/jpeg");
        assert_eq!(mime("noext"), "application/octet-stream");
    }
}
