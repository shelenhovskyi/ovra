//! Opens files in other applications: the user's external editor from
//! `$VISUAL` or `$EDITOR`, else the platform's default handler (`open` on
//! macOS, `explorer` on Windows, `xdg-open` elsewhere).
//! Contract: `ovra_core::services::Opener`.

mod editor;

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{Opener, Result};

use editor::{Invocation, Platform, candidates};

/// The real opener.
#[derive(Debug, Default)]
pub struct SystemOpener {}

impl SystemOpener {
    pub fn new() -> Self {
        Self {}
    }
}

impl Opener for SystemOpener {
    fn open_in_editor(&self, file: &Path, line: Option<u32>) -> Result<()> {
        if !file.is_file() {
            return Err(ProtocolError::new(
                ErrorCode::NotFound,
                format!("no such file: {}", file.display()),
            ));
        }
        let plan = candidates(|var| std::env::var_os(var), Platform::current(), file, line);
        launch_first(&plan, spawn_detached)
    }
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
            "could not start an editor ({}); set $VISUAL to a graphical editor",
            failures.join("; ")
        ),
    ))
}

/// Starts the program without waiting for it; a background thread reaps it.
fn spawn_detached(invocation: &Invocation) -> io::Result<()> {
    let mut child = Command::new(&invocation.program)
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

    #[test]
    fn missing_file_is_not_found() {
        let r = SystemOpener::new().open_in_editor(Path::new("/no/such/ovra-file"), None);
        assert_eq!(r.err().map(|e| e.code), Some(ErrorCode::NotFound));
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
    fn spawns_a_real_process_or_reports_a_missing_one() {
        let missing = spawn_detached(&inv("ovra-definitely-missing-editor"));
        assert_eq!(
            missing.err().map(|e| e.kind()),
            Some(io::ErrorKind::NotFound)
        );
        // `rustc` is on every machine that builds this crate.
        let rustc = Invocation {
            program: "rustc".into(),
            args: vec!["--version".into()],
        };
        assert!(spawn_detached(&rustc).is_ok());
    }
}
