//! Opens files in other applications: the user's external editor from
//! `$VISUAL` or `$EDITOR`, else the platform's default handler (`open` on
//! macOS, `explorer` on Windows, `xdg-open` elsewhere). Web pages go to the
//! default handler, which is the system browser. A file the default handler
//! would run (see [`launcher`]) is only ever given to an editor. Files and
//! folders can also be shown in the system file manager ([`reveal`]).
//! Contract: `ovra_core::services::Opener`.

mod editor;
mod launcher;
mod reveal;

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ovra_core::path::RelPath;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{Opener, Result};

use editor::{Invocation, Platform, candidates, editor_name, system_handler};

/// The real opener.
#[derive(Debug, Default)]
pub struct SystemOpener {}

impl SystemOpener {
    pub fn new() -> Self {
        Self {}
    }
}

impl Opener for SystemOpener {
    fn open_in_editor(&self, root: &Path, path: &RelPath, line: Option<u32>) -> Result<()> {
        let file = inside(root, path)?;
        let plan = candidates(
            |var| std::env::var_os(var),
            Platform::current(),
            &file,
            line,
        );
        launch_first(&safe(plan, &file, path)?, spawn_detached)
    }

    fn open_url(&self, url: &str) -> Result<()> {
        let handler = system_handler(Platform::current(), url.as_ref());
        launch_first(&[handler], spawn_detached)
    }

    fn reveal(&self, root: &Path, path: &RelPath) -> Result<()> {
        let (shown, real) = resolved(root, path)?;
        let kind = if path.is_root() {
            reveal::Kind::Root
        } else if real.is_dir() {
            reveal::Kind::Folder
        } else {
            reveal::Kind::File
        };
        let plan = reveal::plan(Platform::current(), &shown, kind)?;
        reveal::run_in_order(plan, reveal::start)
    }
}

/// The regular file `path` under `root` with symlinks resolved, refused when
/// it resolves outside `root`. Checked at the time of the call: hard links,
/// and a file or folder swapped for a link between this check and the
/// handler opening it, are not detected.
fn inside(root: &Path, path: &RelPath) -> Result<PathBuf> {
    let (shown, real) = resolved(root, path)?;
    if !real.is_file() {
        return Err(not_found(path));
    }
    Ok(shown)
}

fn not_found(path: &RelPath) -> ProtocolError {
    ProtocolError::new(ErrorCode::NotFound, format!("no such file: {path}"))
}

/// `path` under `root` with symlinks resolved, refused when it resolves
/// outside `root`: the path to show and the resolved one. The path to show
/// is under `root` as given, so on Windows a program gets `C:\p\a.rs`, not
/// the verbatim `\\?\C:\p\a.rs` that `canonicalize` returns; a relative
/// `root` gives the resolved path, so no argument starts with `-`.
fn resolved(root: &Path, path: &RelPath) -> Result<(PathBuf, PathBuf)> {
    let real_root = std::fs::canonicalize(root).map_err(|_| not_found(path))?;
    let real = std::fs::canonicalize(path.under(root)).map_err(|_| not_found(path))?;
    let Ok(rest) = real.strip_prefix(&real_root) else {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            format!("{path} points outside the project"),
        ));
    };
    let shown = if !root.is_absolute() {
        real.clone()
    } else if rest.as_os_str().is_empty() {
        root.to_path_buf()
    } else {
        root.join(rest)
    };
    Ok((shown, real))
}

/// Programs that hand a file to its default handler, wherever they come
/// from (`EDITOR=xdg-open` too). `EDITOR="open -a TextEdit"` is dropped with
/// them for launchers: telling it apart is not worth the risk.
const HANDLERS: &[&str] = &[
    "open",
    "xdg-open",
    "gio",
    "gvfs-open",
    "kde-open",
    "kde-open5",
    "gnome-open",
    "exo-open",
    "mimeopen",
    "handlr",
    "wslview",
    "wsl-open",
    "explorer",
    "start",
    "cmd",
    "rundll32",
    "powershell",
    "pwsh",
];

/// `plan` without the default handler when that would run `file` as a
/// program; refused when no editor is left to open it.
fn safe(mut plan: Vec<Invocation>, file: &Path, path: &RelPath) -> Result<Vec<Invocation>> {
    if launcher::may_run(file) {
        plan.retain(|i| !HANDLERS.contains(&editor_name(&i.program).as_str()));
        if plan.is_empty() {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                format!(
                    "{path} could run as a program, so Ovra shows it here and does not hand it to another app"
                ),
            ));
        }
    }
    Ok(plan)
}

/// Starts the first invocation that spawns. A missing or broken editor in
/// `$VISUAL` falls through to the next candidate.
fn launch_first(
    plan: &[Invocation],
    mut spawn: impl FnMut(&Invocation) -> io::Result<()>,
) -> Result<()> {
    let mut failures = Vec::new();
    for invocation in plan {
        match spawn(invocation) {
            Ok(()) => return Ok(()),
            Err(e) => failures.push(format!("{}: {e}", invocation.program.to_string_lossy())),
        }
    }
    Err(ProtocolError::new(
        ErrorCode::Unavailable,
        format!(
            "could not start a program to open it ({}); set $VISUAL to a graphical editor",
            failures.join("; ")
        ),
    ))
}

/// Starts the program without waiting for it; a background thread reaps it.
fn spawn_detached(invocation: &Invocation) -> io::Result<()> {
    let mut command = Command::new(&invocation.program);
    if let Some(path) = ovra_core::child_path::login() {
        command.env("PATH", path);
    }
    let mut child = command
        .args(&invocation.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inv(program: &str) -> Invocation {
        Invocation {
            program: program.into(),
            args: vec!["/f".into()],
        }
    }

    fn rel(raw: &str) -> RelPath {
        RelPath::parse(raw).unwrap_or_else(|e| panic!("{e}"))
    }

    fn code(r: Result<PathBuf>) -> Option<ErrorCode> {
        r.err().map(|e| e.code)
    }

    #[test]
    fn missing_file_is_not_found() {
        let r = SystemOpener::new().open_in_editor(Path::new("/no/such"), &rel("ovra-file"), None);
        assert_eq!(r.err().map(|e| e.code), Some(ErrorCode::NotFound));
    }

    #[test]
    fn opens_only_regular_files_that_resolve_inside_the_root() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (root, outside) = (tmp.path().join("p"), tmp.path().join("secret.txt"));
        let write = |p: &Path| std::fs::write(p, "x").unwrap_or_else(|e| panic!("{e}"));
        std::fs::create_dir_all(root.join("src")).unwrap_or_else(|e| panic!("{e}"));
        write(&root.join("src/a.rs"));
        write(&outside);

        let given = Some(root.join("src").join("a.rs"));
        assert_eq!(inside(&root, &rel("src/a.rs")).ok(), given);
        assert_eq!(code(inside(&root, &rel("src"))), Some(ErrorCode::NotFound));
        assert_eq!(
            code(inside(&root, &rel("src/b.rs"))),
            Some(ErrorCode::NotFound)
        );

        #[cfg(unix)]
        {
            let link = |to: &Path, at: &str| {
                std::os::unix::fs::symlink(to, root.join(at)).unwrap_or_else(|e| panic!("{e}"))
            };
            link(&outside, "src/leak.txt");
            link(tmp.path(), "up");
            link(&root.join("src/a.rs"), "src/alias.rs");
            assert_eq!(
                code(inside(&root, &rel("src/leak.txt"))),
                Some(ErrorCode::Forbidden)
            );
            assert_eq!(
                code(inside(&root, &rel("up/secret.txt"))),
                Some(ErrorCode::Forbidden)
            );
            assert_eq!(inside(&root, &rel("src/alias.rs")).ok(), given);
            // A link to a launcher is judged by the launcher's name.
            write(&root.join("src/setup.exe"));
            link(&root.join("src/setup.exe"), "src/notes.md");
            let resolved = inside(&root, &rel("src/notes.md")).unwrap_or_else(|e| panic!("{e}"));
            assert!(launcher::may_run(&resolved));
            // A root that is itself a link: both sides are resolved.
            link(&root, "../alias-root");
            let alias = tmp.path().join("alias-root");
            let shown = Some(alias.join("src").join("a.rs"));
            assert_eq!(inside(&alias, &rel("src/a.rs")).ok(), shown);
        }
    }

    #[test]
    fn reveals_files_folders_and_the_root_inside_it_only() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let root = tmp.path().join("p");
        std::fs::create_dir_all(root.join("src")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(root.join("src/a.rs"), "x").unwrap_or_else(|e| panic!("{e}"));
        let shown = |raw: &str| resolved(&root, &rel(raw)).map(|(shown, _)| shown);

        assert_eq!(shown("").ok(), Some(root.clone()));
        assert_eq!(shown("src").ok(), Some(root.join("src")));
        assert_eq!(shown("src/a.rs").ok(), Some(root.join("src").join("a.rs")));
        assert_eq!(code(shown("src/b.rs")), Some(ErrorCode::NotFound));
        let missing = SystemOpener::new().reveal(&root, &rel("nope"));
        assert_eq!(missing.err().map(|e| e.code), Some(ErrorCode::NotFound));

        #[cfg(unix)]
        {
            let outside = tmp.path().join("out");
            std::fs::create_dir(&outside).unwrap_or_else(|e| panic!("{e}"));
            std::os::unix::fs::symlink(&outside, root.join("out"))
                .unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(code(shown("out")), Some(ErrorCode::Forbidden));
            let refused = SystemOpener::new().reveal(&root, &rel("out"));
            assert_eq!(refused.err().map(|e| e.code), Some(ErrorCode::Forbidden));
        }
    }

    #[test]
    fn a_launcher_goes_only_to_an_editor() {
        let path = rel("tools/setup.exe");
        let file = Path::new("/p/tools/setup.exe");
        let handler = system_handler(Platform::current(), file.as_os_str());
        let editor = inv("code");
        let plan = safe(vec![editor.clone(), handler.clone()], file, &path);
        assert_eq!(plan.ok(), Some(vec![editor]));
        let refused = safe(vec![handler.clone()], file, &path);
        assert_eq!(refused.err().map(|e| e.code), Some(ErrorCode::Forbidden));
        let text = Path::new("/p/notes.md");
        let handler = system_handler(Platform::current(), text.as_os_str());
        let kept = safe(vec![handler.clone()], text, &rel("notes.md"));
        assert_eq!(kept.ok(), Some(vec![handler]));
        // `EDITOR=xdg-open` is a handler too.
        let disguised = safe(vec![inv("/usr/bin/xdg-open"), inv("gio")], file, &path);
        assert_eq!(disguised.err().map(|e| e.code), Some(ErrorCode::Forbidden));
    }

    #[test]
    fn falls_through_to_the_next_candidate() {
        let mut tried = Vec::new();
        let r = launch_first(&[inv("broken"), inv("works"), inv("unused")], |i| {
            tried.push(i.program.clone());
            if i.program == "broken" {
                Err(io::ErrorKind::NotFound.into())
            } else {
                Ok(())
            }
        });
        assert!(r.is_ok());
        assert_eq!(tried, ["broken", "works"]);
    }

    #[test]
    fn reports_every_failure_when_nothing_starts() {
        let r = launch_first(&[inv("a"), inv("b")], |_| {
            Err(io::ErrorKind::NotFound.into())
        });
        let err = r.expect_err("no candidate started");
        assert_eq!(err.code, ErrorCode::Unavailable);
        assert!(
            err.message.contains("a: ") && err.message.contains("b: "),
            "{}",
            err.message
        );
    }

    #[test]
    fn a_missing_program_fails_to_spawn() {
        let missing = spawn_detached(&inv("ovra-definitely-missing-editor"));
        assert_eq!(
            missing.err().map(|e| e.kind()),
            Some(io::ErrorKind::NotFound)
        );
    }
}
