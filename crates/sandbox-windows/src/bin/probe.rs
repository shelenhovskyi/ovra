//! The helper alone plus a few probes, for this crate's tests:
//! `ovra-sandbox-probe --ovra-sandbox-helper ... -- <this> write <path> ...`.
//! `connect` takes `ip:port`; `rename` takes `from|to` and `acl` takes
//! `path|sid` (grants that SID modify with `icacls`). `delete` asks only for
//! `DELETE`, unlike `remove`. Each probe prints `<what> <path>: yes` if it
//! worked, else `no`.

fn main() {
    #[cfg(windows)]
    ovra_sandbox_windows::helper_if_asked();
    let args: Vec<String> = std::env::args().skip(1).collect();
    for pair in args.chunks(2) {
        let [what, path] = pair else { break };
        let worked = match what.as_str() {
            "write" => std::fs::write(path, "planted").is_ok(),
            "read" => std::fs::read(path).is_ok(),
            "remove" => std::fs::remove_dir_all(path).is_ok() || std::fs::remove_file(path).is_ok(),
            "env" => std::env::var_os(path).is_some(),
            "connect" => path.parse().is_ok_and(|address| {
                std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_secs(2))
                    .is_ok()
            }),
            "rename" => path
                .split_once('|')
                .is_some_and(|(from, to)| std::fs::rename(from, to).is_ok()),
            "acl" => path
                .split_once('|')
                .is_some_and(|(file, sid)| icacls(file, sid)),
            "delete" => delete(path),
            _ => false,
        };
        println!("{what} {path}: {}", if worked { "yes" } else { "no" });
    }
}

fn icacls(file: &str, sid: &str) -> bool {
    let system = std::env::var_os("SystemRoot").unwrap_or_default();
    let icacls = std::path::Path::new(&system)
        .join("System32")
        .join("icacls.exe");
    std::process::Command::new(icacls)
        .args([file, "/grant", &format!("*{sid}:(M)")])
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Opens `path` for `DELETE` alone and marks it deleted, the least a delete
/// can ask for.
#[cfg(windows)]
#[allow(unsafe_code)]
fn delete(path: &str) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, DELETE, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FileDispositionInfo, OPEN_EXISTING, SetFileInformationByHandle,
    };
    let wide: Vec<u16> = std::ffi::OsStr::new(path)
        .encode_wide()
        .chain([0])
        .collect();
    // SAFETY: `wide` is a NUL-terminated path that outlives the call; the
    // handle is checked and closed once.
    unsafe {
        let handle = CreateFileW(
            wide.as_ptr(),
            DELETE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return false;
        }
        let info = FILE_DISPOSITION_INFO { DeleteFile: true };
        let done = SetFileInformationByHandle(
            handle,
            FileDispositionInfo,
            (&raw const info).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        ) != 0;
        CloseHandle(handle);
        done
    }
}

#[cfg(not(windows))]
fn delete(_: &str) -> bool {
    false
}
