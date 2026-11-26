//! The `PATH` the programs Ovra starts get. A desktop app started from the
//! Finder or a launcher inherits a minimal `PATH` without the tools the
//! user installed (Homebrew's `git-lfs`, `node`, ...). The app finds the
//! login shell's `PATH` once at start (`ovra_sandbox::use_login_path`) and
//! sets it here; every service that starts a program uses it. Only `PATH`
//! is taken from the shell, no other variable. The core only holds it.

use std::ffi::{OsStr, OsString};
use std::sync::OnceLock;

static LOGIN: OnceLock<OsString> = OnceLock::new();

/// Sets the login shell's `PATH` for the rest of the process. Only the
/// first call counts.
pub fn set_login(path: OsString) {
    let _ = LOGIN.set(path);
}

/// The login shell's `PATH`, if the app found one. A program started with
/// Ovra's own environment gets it as `PATH`.
pub fn login() -> Option<&'static OsStr> {
    LOGIN.get().map(OsString::as_os_str)
}

/// The `PATH` to search programs on: the login shell's, else `inherited`
/// (Ovra's own, read by the caller).
pub fn or_inherited(inherited: Option<OsString>) -> Option<OsString> {
    login().map(OsStr::to_owned).or(inherited)
}

/// The `PATH` a program Ovra starts outside the sandbox gets: only the
/// absolute directories of the login shell's `PATH`, else of `inherited`.
/// An empty entry or `.` would let it start a helper (`ssh`, `gpg`,
/// `git-lfs`) from its working directory, which an agent may have written.
pub fn absolute(inherited: Option<OsString>) -> OsString {
    let dirs: Vec<_> = or_inherited(inherited)
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|d| d.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    std::env::join_paths(dirs).unwrap_or_default()
}

/// The variables to set on a program started from a (possibly wrapped)
/// command line: `PATH` from the login shell first, then the line's own,
/// so a sandbox's `PATH` in the line wins.
pub fn env(line: &[(OsString, Option<OsString>)]) -> Vec<(OsString, Option<OsString>)> {
    login()
        .map(|path| ("PATH".into(), Some(path.to_owned())))
        .into_iter()
        .chain(line.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_get_only_the_absolute_directories_of_the_path() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let abs = std::env::temp_dir();
        let text = abs.to_string_lossy();
        let path = OsString::from(format!("{sep}.{sep}{text}{sep}bin{sep}{sep}{text}"));
        let want = std::env::join_paths([&abs, &abs]).unwrap_or_default();
        assert_eq!(absolute(Some(path)), want);
        assert_eq!(absolute(Some(".".into())), OsString::new());
        assert_eq!(absolute(None), OsString::new());
    }

    // Each test runs in its own process (nextest), so the global starts empty.
    #[test]
    fn the_login_path_wins_once_set_and_a_lines_own_path_comes_after_it() {
        let line = vec![
            ("PATH".into(), Some("/sandbox/bin".into())),
            ("X".into(), None),
        ];
        assert_eq!(login(), None);
        assert_eq!(or_inherited(Some("/bin".into())), Some("/bin".into()));
        assert_eq!(env(&line), line);
        set_login("/opt/homebrew/bin:/usr/bin".into());
        set_login("/elsewhere".into());
        assert_eq!(login(), Some(OsStr::new("/opt/homebrew/bin:/usr/bin")));
        assert_eq!(
            or_inherited(Some("/bin".into())),
            Some("/opt/homebrew/bin:/usr/bin".into())
        );
        let env = env(&line);
        assert_eq!(
            env[0],
            ("PATH".into(), Some("/opt/homebrew/bin:/usr/bin".into()))
        );
        assert_eq!(env[1..], line);
        // Applied in order, the last `PATH` is the sandbox's.
        let last = env.iter().rev().find(|(name, _)| name == "PATH");
        assert_eq!(last, Some(&("PATH".into(), Some("/sandbox/bin".into()))));
    }
}
