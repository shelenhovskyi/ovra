//! Files the platform's default handler would run instead of showing: a
//! program, a script or a shortcut to one. Ovra never hands these to
//! `open`, `explorer` or `xdg-open`: a file an agent wrote must not become a
//! program the person starts with one click. An editor from `$VISUAL` or
//! `$EDITOR` still opens them, as text.
//!
//! The list covers every platform on all of them: a project made on one is
//! often opened on another.

use std::path::Path;

/// Extensions (lowercase) that the default handler of some platform runs,
/// installs, or follows to another file or program.
const LAUNCHERS: &[&str] = &[
    // Windows programs, scripts, installers and control panel items.
    "exe",
    "com",
    "scr",
    "pif",
    "cpl",
    "msc",
    "msi",
    "msp",
    "mst",
    "msix",
    "msixbundle",
    "appx",
    "appxbundle",
    "appinstaller",
    "bat",
    "cmd",
    "ps1",
    "psm1",
    "psd1",
    "ps1xml",
    "vbs",
    "vbe",
    "vb",
    "js",
    "jse",
    "wsf",
    "wsh",
    "wsc",
    "ws",
    "sct",
    "hta",
    "inf",
    "reg",
    "chm",
    "hlp",
    "gadget",
    "diagcab",
    "application",
    "appref-ms",
    "xbap",
    "jnlp",
    "jar",
    // Windows shortcuts and shell links, which canonicalize does not follow.
    "lnk",
    "url",
    "website",
    "scf",
    "shb",
    "shs",
    "library-ms",
    "search-ms",
    "searchconnector-ms",
    "settingcontent-ms",
    "iqy",
    "slk",
    // Scripts that installed interpreters register for.
    "py",
    "pyw",
    "pyz",
    "pyzw",
    "pl",
    "rb",
    // macOS apps, scripts, installers and location files.
    "app",
    "command",
    "tool",
    "terminal",
    "workflow",
    "action",
    "scpt",
    "scptd",
    "applescript",
    "osascript",
    "pkg",
    "mpkg",
    "dmg",
    "prefpane",
    "fileloc",
    "webloc",
    "inetloc",
    "afploc",
    "ftploc",
    // Linux launchers, scripts and self-contained programs.
    "desktop",
    "sh",
    "bash",
    "zsh",
    "csh",
    "ksh",
    "fish",
    "run",
    "bin",
    "appimage",
    "flatpakref",
    "snap",
    "deb",
    "rpm",
];

/// Whether the default handler might run `file` (a resolved, regular file).
pub(crate) fn may_run(file: &Path) -> bool {
    let by_name = file
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| LAUNCHERS.contains(&e.to_ascii_lowercase().as_str()));
    by_name || is_executable(file)
}

#[cfg(unix)]
fn is_executable(file: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(file).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchers_are_known_by_extension_in_any_case() {
        for name in [
            "a.exe",
            "b.LNK",
            "c.Bat",
            "d.command",
            "e.desktop",
            "f.sh",
            "g.url",
            "h.ps1",
            "i.webloc",
            "j.tar.AppImage",
            "k.app",
        ] {
            assert!(may_run(Path::new(name)), "{name}");
        }
        for name in ["a.rs", "b.pdf", "c.png", "README", "d.exe.txt", ".sh"] {
            assert!(!may_run(Path::new(name)), "{name}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_executable_file_is_a_launcher_whatever_its_name() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let file = tmp.path().join("notes.txt");
        std::fs::write(&file, "x").unwrap_or_else(|e| panic!("{e}"));
        assert!(!may_run(&file));
        let exec = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&file, exec).unwrap_or_else(|e| panic!("{e}"));
        assert!(may_run(&file));
    }
}
