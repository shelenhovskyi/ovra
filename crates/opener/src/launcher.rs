//! Files the platform's default handler would run instead of showing: a
//! program, a script or a shortcut to one. Ovra never hands these to
//! `open`, `explorer` or `xdg-open`: a file an agent wrote must not become a
//! program the person starts with one click. An editor from `$VISUAL` or
//! `$EDITOR` still opens them, as text.
//!
//! The list covers every platform on all of them: a project made on one is
//! often opened on another. For Windows it follows the attachments Outlook
//! blocks, plus files that connect, mount or install something.

use std::path::Path;

/// Extensions (lowercase) that the default handler of some platform runs,
/// installs, mounts, or follows to another file, program or server.
#[rustfmt::skip]
const LAUNCHERS: &[&str] = &[
    // Windows programs, scripts, installers, add-ins and control panel items.
    "exe", "com", "scr", "pif", "cpl", "msc", "msi", "msp", "mst", "msu", "msix", "msixbundle",
    "appx", "appxbundle", "appinstaller", "bat", "cmd", "ps1", "psm1", "psd1", "ps1xml", "ps2",
    "ps2xml", "psc1", "psc2", "msh", "msh1", "msh2", "mshxml", "msh1xml", "msh2xml", "vbs",
    "vbe", "vb", "js", "jse", "wsf", "wsh", "wsc", "ws", "sct", "hta", "inf", "reg", "chm",
    "hlp", "gadget", "diagcab", "diagcfg", "diagpkg", "application", "appref-ms", "xbap", "jnlp",
    "jar", "vsto", "ppkg", "xll", "mde", "accde", "ade", "adp", "mcf", "job", "cer", "crt", "der",
    "mdb", "mda", "mdt", "mdw", "mdz", "accdb", "cab", "grp", "ins", "isp", "its", "udl", "xnk",
    "printerexport", "vsmacros", "vbp", "bas", "asx", "htc", "prf", "ops", "pcd", "fxp", "prg",
    "cnt", "hpj", "vsw", "webpnp", "plg", "pst",
    // Windows shortcuts, shell links and files that reach out to a server.
    "lnk", "url", "website", "scf", "shb", "shs", "library-ms", "search-ms", "searchconnector-ms",
    "settingcontent-ms", "iqy", "slk", "rdp", "wsb", "theme", "themepack", "deskthemepack",
    // Disk images, whose files the system does not mark as downloaded.
    "iso", "img", "vhd", "vhdx", "dmg",
    // Scripts and bytecode that installed interpreters register for.
    "py", "pyw", "pyz", "pyzw", "pyc", "pyo", "pl", "rb",
    // macOS apps, scripts, installers, profiles and location files.
    "app", "command", "tool", "terminal", "workflow", "action", "scpt", "scptd", "applescript",
    "osascript", "pkg", "mpkg", "prefpane", "mobileconfig", "shortcut", "saver", "qlgenerator",
    "fileloc", "webloc", "inetloc", "afploc", "ftploc",
    // Linux launchers, scripts, packages and self-contained programs.
    "desktop", "sh", "bash", "zsh", "csh", "ksh", "fish", "run", "bin", "appimage", "flatpak",
    "flatpakref", "flatpakrepo", "snap", "deb", "rpm",
];

/// Extensions (lowercase) the default handler opens by type even when the
/// file has an exec bit, as every file does on FAT, exFAT, SMB or `/mnt/c`.
#[rustfmt::skip]
const DOCUMENTS: &[&str] = &[
    "md", "markdown", "txt", "rst", "adoc", "pdf", "png", "jpg", "jpeg", "gif", "webp", "svg",
    "ico", "bmp", "tif", "tiff", "mp3", "wav", "ogg", "flac", "mp4", "mov", "webm", "mkv", "csv",
    "tsv", "json", "yaml", "yml", "toml", "xml", "html", "htm", "css", "rs", "ts", "tsx", "jsx",
    "svelte", "c", "h", "cpp", "hpp", "go", "java", "kt", "swift", "lock", "log", "docx", "xlsx",
    "pptx", "odt", "ods", "odp", "zip", "gz", "tar",
];

/// Whether the default handler might run `file` (a resolved, regular file):
/// a known launcher extension, or an exec bit on a file whose type is not a
/// known document.
pub(crate) fn may_run(file: &Path) -> bool {
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match ext.as_deref() {
        Some(ext) if LAUNCHERS.contains(&ext) => true,
        Some(ext) if DOCUMENTS.contains(&ext) => false,
        _ => is_executable(file),
    }
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
            "l.rdp",
            "m.wsb",
            "n.pyc",
            "o.iso",
        ] {
            assert!(may_run(Path::new(name)), "{name}");
        }
        for name in ["a.rs", "b.pdf", "c.png", "README", "d.exe.txt", ".sh"] {
            assert!(!may_run(Path::new(name)), "{name}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_exec_bit_makes_a_launcher_unless_the_type_is_a_document() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let exec = std::fs::Permissions::from_mode(0o755);
        for (name, runs) in [("tool", true), ("data.weird", true), ("notes.md", false)] {
            let file = tmp.path().join(name);
            std::fs::write(&file, "x").unwrap_or_else(|e| panic!("{e}"));
            assert!(!may_run(&file), "{name}");
            std::fs::set_permissions(&file, exec.clone()).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(may_run(&file), runs, "{name}");
        }
    }
}
