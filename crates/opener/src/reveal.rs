//! Shows a file or folder in the system file manager: `open -R` on macOS,
//! `explorer /select,` on Windows, and on Linux the freedesktop
//! `FileManager1.ShowItems` call over D-Bus, else `xdg-open` on the folder
//! that holds it. Nothing is ever opened or run: `explorer` only ever
//! selects, and `xdg-open` gets `<folder>/.`, which names no file even if
//! the folder was swapped for one.

use std::ffi::OsString;
use std::io;
use std::path::Path;

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;

use crate::editor::{Invocation, Platform};

/// What is revealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    File,
    Folder,
    /// The project root: on Linux its own folder opens, not its parent.
    Root,
}

/// The commands to try for the absolute `target`, in order: the next one
/// runs only when the one before cannot start or fails.
pub(crate) fn plan(platform: Platform, target: &Path, kind: Kind) -> Result<Vec<Invocation>> {
    let invocation = |program: &str, args: Vec<OsString>| Invocation {
        program: program.into(),
        args,
    };
    Ok(match platform {
        Platform::Macos => vec![invocation("open", vec!["-R".into(), target.into()])],
        // Passed as written (see `start`): explorer splits its command line
        // at commas outside quotes, so the path is always quoted. Folders
        // and the root are selected too: `explorer <path>` would run a file.
        Platform::Windows => {
            let path = target.as_os_str();
            if path.as_encoded_bytes().contains(&b'"') {
                return Err(ProtocolError::new(
                    ErrorCode::BadRequest,
                    "a path with a quote cannot be shown in Explorer",
                ));
            }
            let mut arg = OsString::from("/select,\"");
            arg.push(path);
            arg.push("\"");
            vec![invocation("explorer", vec![arg])]
        }
        Platform::Unix => {
            let folder = match kind {
                Kind::Root => target,
                Kind::File | Kind::Folder => target.parent().unwrap_or(target),
            };
            // `.` resolves only in a directory, so `xdg-open` never gets a
            // file to open.
            let folder = folder.join(".");
            let mut item = OsString::from("array:string:");
            item.push(file_uri(target));
            vec![
                invocation(
                    "dbus-send",
                    vec![
                        "--session".into(),
                        "--print-reply".into(),
                        "--reply-timeout=5000".into(),
                        "--dest=org.freedesktop.FileManager1".into(),
                        "--type=method_call".into(),
                        "/org/freedesktop/FileManager1".into(),
                        "org.freedesktop.FileManager1.ShowItems".into(),
                        item,
                        "string:".into(),
                    ],
                ),
                invocation("xdg-open", vec![folder.into_os_string()]),
            ]
        }
    })
}

/// A `file://` URI with every byte but unreserved ones and `/` escaped, so
/// no `,` reaches `dbus-send`, which splits arrays there.
fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_encoded_bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// How a started program ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exit {
    Succeeded,
    Failed,
    /// D-Bus got no reply in time: the file manager may still be starting
    /// and show the item, so nothing else is tried, to avoid two windows.
    NoReply,
}

/// Waits for a started program and tells how it ended.
pub(crate) type Wait = Box<dyn FnOnce() -> Exit + Send>;

/// Starts the first invocation that starts and returns. A background
/// thread waits for it and, if it fails, goes on with the rest.
pub(crate) fn run_in_order<S>(plan: Vec<Invocation>, start: S) -> Result<()>
where
    S: Fn(&Invocation) -> io::Result<Wait> + Send + 'static,
{
    let mut plan = plan.into_iter();
    let mut failures = Vec::new();
    while let Some(invocation) = plan.next() {
        match start(&invocation) {
            Ok(wait) => {
                let rest = plan.collect();
                std::thread::spawn(move || continue_after(wait, rest, start));
                return Ok(());
            }
            Err(e) => failures.push(format!("{}: {e}", invocation.program.to_string_lossy())),
        }
    }
    Err(ProtocolError::new(
        ErrorCode::Unavailable,
        format!("could not start a file manager ({})", failures.join("; ")),
    ))
}

fn continue_after<S>(wait: Wait, rest: Vec<Invocation>, start: S)
where
    S: Fn(&Invocation) -> io::Result<Wait> + Send + 'static,
{
    if wait() == Exit::Failed && !rest.is_empty() {
        // Nobody is left to tell; the person sees that nothing opened.
        let _ = run_in_order(rest, start);
    }
}

/// Starts the program with no shell. On Windows the arguments go to
/// explorer exactly as `plan` quoted them.
pub(crate) fn start(invocation: &Invocation) -> io::Result<Wait> {
    let mut command = std::process::Command::new(&invocation.program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        for arg in &invocation.args {
            command.raw_arg(arg);
        }
    }
    #[cfg(not(windows))]
    command.args(&invocation.args);
    let child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    Ok(Box::new(move || match child.wait_with_output() {
        Ok(out) => exit(out.status.success(), &out.stderr),
        Err(_) => Exit::Failed,
    }))
}

/// Tells a D-Bus timeout from other failures by what `dbus-send` prints.
fn exit(success: bool, stderr: &[u8]) -> Exit {
    let says = |name: &[u8]| stderr.windows(name.len()).any(|w| w == name);
    if success {
        Exit::Succeeded
    } else if says(b"org.freedesktop.DBus.Error.NoReply")
        || says(b"org.freedesktop.DBus.Error.Timeout")
    {
        Exit::NoReply
    } else {
        Exit::Failed
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    fn lines(platform: Platform, target: &str, kind: Kind) -> Vec<String> {
        plan(platform, Path::new(target), kind)
            .unwrap_or_else(|e| panic!("{e}"))
            .into_iter()
            .map(|i| {
                std::iter::once(i.program)
                    .chain(i.args)
                    .map(|s| s.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    #[test]
    fn macos_selects_the_item() {
        for kind in [Kind::File, Kind::Folder, Kind::Root] {
            let plan = lines(Platform::Macos, "/p/-rf a.txt", kind);
            assert_eq!(plan, ["open -R /p/-rf a.txt"]);
        }
    }

    #[test]
    fn windows_only_ever_selects_quoted() {
        let file = lines(Platform::Windows, r"C:\p\a,b c.txt", Kind::File);
        assert_eq!(file, [r#"explorer /select,"C:\p\a,b c.txt""#]);
        let folder = lines(Platform::Windows, r"C:\p\src", Kind::Folder);
        assert_eq!(folder, [r#"explorer /select,"C:\p\src""#]);
        let root = lines(Platform::Windows, r"C:\p", Kind::Root);
        assert_eq!(root, [r#"explorer /select,"C:\p""#]);
        let drive = lines(Platform::Windows, r"C:\", Kind::Root);
        assert_eq!(drive, [r#"explorer /select,"C:\""#]);
        let quoted = plan(Platform::Windows, Path::new("C:\\p\\a\" /x"), Kind::File);
        assert_eq!(quoted.err().map(|e| e.code), Some(ErrorCode::BadRequest));
    }

    #[test]
    fn linux_asks_the_file_manager_then_opens_the_folder() {
        let show = "dbus-send --session --print-reply --reply-timeout=5000 \
            --dest=org.freedesktop.FileManager1 --type=method_call \
            /org/freedesktop/FileManager1 org.freedesktop.FileManager1.ShowItems";
        let file = lines(Platform::Unix, "/p/a b,c%.txt", Kind::File);
        assert_eq!(
            file,
            [
                format!("{show} array:string:file:///p/a%20b%2Cc%25.txt string:"),
                "xdg-open /p/.".into(),
            ]
        );
        let folder = lines(Platform::Unix, "/p/src", Kind::Folder);
        assert_eq!(folder[1], "xdg-open /p/.");
        let root = lines(Platform::Unix, "/p", Kind::Root);
        assert_eq!(root[1], "xdg-open /p/.");
        assert_eq!(file_uri(Path::new("/p/\u{e9}")), "file:///p/%C3%A9");
    }

    fn fake(
        started: mpsc::Sender<String>,
        fails: &'static str,
    ) -> impl Fn(&Invocation) -> io::Result<Wait> + Send + 'static {
        move |invocation| {
            let program = invocation.program.to_string_lossy().into_owned();
            if program == "missing" {
                return Err(io::ErrorKind::NotFound.into());
            }
            let exit = if program == fails {
                Exit::Failed
            } else {
                Exit::Succeeded
            };
            started.send(program).unwrap_or_else(|e| panic!("{e}"));
            Ok(Box::new(move || exit))
        }
    }

    fn inv(program: &str) -> Invocation {
        Invocation {
            program: program.into(),
            args: Vec::new(),
        }
    }

    #[test]
    fn falls_back_when_the_first_fails_or_cannot_start() {
        let (tx, rx) = mpsc::channel();
        let plan = vec![inv("missing"), inv("dbus-send"), inv("xdg-open")];
        run_in_order(plan, fake(tx, "dbus-send")).unwrap_or_else(|e| panic!("{e}"));
        let next = || rx.recv_timeout(Duration::from_secs(1)).ok();
        assert_eq!(next().as_deref(), Some("dbus-send"));
        assert_eq!(next().as_deref(), Some("xdg-open"));
    }

    #[test]
    fn stops_after_one_that_succeeds() {
        let (tx, rx) = mpsc::channel();
        let wait: Wait = Box::new(|| Exit::Succeeded);
        continue_after(wait, vec![inv("xdg-open")], fake(tx.clone(), ""));
        assert!(rx.try_recv().is_err());
        let wait: Wait = Box::new(|| Exit::NoReply);
        continue_after(wait, vec![inv("xdg-open")], fake(tx, ""));
        assert!(rx.try_recv().is_err());
        let none = run_in_order(vec![inv("missing")], fake(mpsc::channel().0, ""));
        assert_eq!(none.err().map(|e| e.code), Some(ErrorCode::Unavailable));
    }

    #[test]
    fn a_dbus_timeout_is_told_from_other_failures() {
        let timeout = b"Error org.freedesktop.DBus.Error.NoReply: Did not receive a reply.";
        assert_eq!(exit(false, timeout), Exit::NoReply);
        let unknown = b"Error org.freedesktop.DBus.Error.ServiceUnknown: no such name";
        assert_eq!(exit(false, unknown), Exit::Failed);
        assert_eq!(exit(false, b""), Exit::Failed);
        assert_eq!(exit(true, timeout), Exit::Succeeded);
    }

    #[cfg(unix)]
    #[test]
    fn the_xdg_open_dot_names_no_file() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let file = tmp.path().join("a.txt");
        std::fs::write(&file, "x").unwrap_or_else(|e| panic!("{e}"));
        assert!(file.join(".").metadata().is_err());
        assert!(tmp.path().join(".").is_dir());
    }
}
