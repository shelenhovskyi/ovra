//! The language servers Ovra knows and finding them on `PATH`. Ovra never
//! downloads or installs one; it names the command the person can run.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use ovra_core::protocol::lsp::KnownServer;

/// One server Ovra can start for a language. Several may serve one
/// language; the first one found wins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerDef {
    /// The editor's language id (`rust`, `typescript`, ...).
    pub language: String,
    /// The server's name, such as `rust-analyzer`.
    pub name: String,
    /// A bare name searched on `PATH`, or an absolute path used as it is.
    pub program: PathBuf,
    pub args: Vec<String>,
    /// How to install it, for the person to run themselves.
    pub install: String,
    /// Variables set for this server besides the cache ones.
    pub env: Vec<(String, String)>,
}

fn def(language: &str, name: &str, program: &str, args: &[&str], install: &str) -> ServerDef {
    ServerDef {
        language: language.into(),
        name: name.into(),
        program: program.into(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        install: install.into(),
        env: Vec::new(),
    }
}

/// The servers Ovra knows, keyed by the editor's language ids. The UI asks
/// for `typescript` for TypeScript, TSX and JavaScript files. Every one is
/// under a permissive license.
pub(crate) fn table() -> Vec<ServerDef> {
    let npm = "npm install -g vscode-langservers-extracted";
    let clangd = "install clangd (LLVM) with your system's package manager";
    let mut gopls = def(
        "go",
        "gopls",
        "gopls",
        &[],
        "go install golang.org/x/tools/gopls@latest",
    );
    // No module downloads and no toolchain switches: the network is blocked.
    gopls.env = vec![
        ("GOPROXY".into(), "off".into()),
        ("GOTOOLCHAIN".into(), "local".into()),
    ];
    vec![
        def(
            "rust",
            "rust-analyzer",
            "rust-analyzer",
            &[],
            "rustup component add rust-analyzer",
        ),
        def(
            "typescript",
            "typescript-language-server",
            "typescript-language-server",
            &["--stdio"],
            "npm install -g typescript-language-server typescript",
        ),
        def(
            "python",
            "basedpyright",
            "basedpyright-langserver",
            &["--stdio"],
            "pipx install basedpyright",
        ),
        def(
            "python",
            "pyright",
            "pyright-langserver",
            &["--stdio"],
            "npm install -g pyright",
        ),
        def(
            "json",
            "vscode-json-language-server",
            "vscode-json-language-server",
            &["--stdio"],
            npm,
        ),
        def(
            "css",
            "vscode-css-language-server",
            "vscode-css-language-server",
            &["--stdio"],
            npm,
        ),
        gopls,
        def("c", "clangd", "clangd", &[], clangd),
        def("cpp", "clangd", "clangd", &[], clangd),
    ]
}

/// One entry per language in `defs`: the first server found, or the first
/// one named when none is installed.
pub(crate) fn known(defs: &[ServerDef], path: Option<&OsStr>) -> Vec<KnownServer> {
    let mut known: Vec<KnownServer> = Vec::new();
    for def in defs {
        let found = find(&def.program, path);
        let entry = KnownServer {
            language: def.language.clone(),
            name: def.name.clone(),
            path: found.map(|p| p.display().to_string()),
            install: def.install.clone(),
        };
        match known.iter_mut().find(|k| k.language == def.language) {
            Some(k) if k.path.is_none() && entry.path.is_some() => *k = entry,
            Some(_) => {}
            None => known.push(entry),
        }
    }
    known
}

/// The first server for `language` that is installed, and where.
pub(crate) fn installed<'a>(
    defs: &'a [ServerDef],
    language: &str,
    path: Option<&OsStr>,
) -> Option<(&'a ServerDef, PathBuf)> {
    defs.iter()
        .filter(|d| d.language == language)
        .find_map(|d| find(&d.program, path).map(|found| (d, found)))
}

/// Where `program` is: itself when absolute, else the first absolute
/// `PATH` entry that holds an executable file of that name. Empty,
/// relative and `.` entries are skipped, so the project cannot plant one.
/// The file is not run: a rustup proxy without its component counts.
pub(crate) fn find(program: &Path, path: Option<&OsStr>) -> Option<PathBuf> {
    if program.is_absolute() {
        return executable(program).then(|| program.to_path_buf());
    }
    if program.components().count() != 1 {
        return None;
    }
    std::env::split_paths(path?)
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| names(program).map(move |name| dir.join(name)))
        .find(|candidate| executable(candidate))
}

#[cfg(unix)]
fn names(program: &Path) -> impl Iterator<Item = PathBuf> {
    std::iter::once(program.to_path_buf())
}

/// npm installs `.cmd` shims on Windows.
#[cfg(not(unix))]
fn names(program: &Path) -> impl Iterator<Item = PathBuf> {
    ["exe", "cmd"]
        .into_iter()
        .map(|ext| program.with_extension(ext))
        .chain(std::iter::once(program.to_path_buf()))
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn file(dir: &Path, name: &str, mode: u32) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\n").expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");
        path
    }

    fn path_of(dirs: &[&Path]) -> std::ffi::OsString {
        std::env::join_paths(dirs).expect("join")
    }

    #[test]
    fn finds_the_first_executable_on_path() {
        let (a, b) = (
            tempfile::tempdir().expect("tmp"),
            tempfile::tempdir().expect("tmp"),
        );
        file(a.path(), "ra", 0o644);
        let found = file(b.path(), "ra", 0o755);
        let path = path_of(&[a.path(), b.path()]);
        assert_eq!(find(Path::new("ra"), Some(&path)), Some(found));
        assert_eq!(find(Path::new("other"), Some(&path)), None);
        assert_eq!(find(Path::new("ra"), None), None);
    }

    #[test]
    fn skips_empty_relative_and_dot_entries() {
        let dir = tempfile::tempdir().expect("tmp");
        file(dir.path(), "ra", 0o755);
        let path = std::ffi::OsString::from(":.:relative:");
        // A relative entry would resolve against the current folder.
        assert_eq!(find(Path::new("ra"), Some(&path)), None);
        let program = dir.path().join("ra");
        assert_eq!(find(&program, None), Some(program.clone()));
        assert_eq!(
            find(Path::new("sub/ra"), Some(&path_of(&[dir.path()]))),
            None
        );
    }

    #[test]
    fn reports_one_server_per_language_and_prefers_an_installed_one() {
        let dir = tempfile::tempdir().expect("tmp");
        let pyright = file(dir.path(), "pyright-langserver", 0o755);
        let path = path_of(&[dir.path()]);
        let defs = table();
        let known = known(&defs, Some(&path));
        let languages: Vec<&str> = known.iter().map(|k| k.language.as_str()).collect();
        assert_eq!(
            languages,
            [
                "rust",
                "typescript",
                "python",
                "json",
                "css",
                "go",
                "c",
                "cpp"
            ]
        );
        let python = known
            .iter()
            .find(|k| k.language == "python")
            .expect("python");
        assert_eq!(python.name, "pyright");
        assert_eq!(python.path, Some(pyright.display().to_string()));
        let rust = &known[0];
        assert_eq!((rust.name.as_str(), &rust.path), ("rust-analyzer", &None));
        assert_eq!(rust.install, "rustup component add rust-analyzer");
        let (def, found) = installed(&defs, "python", Some(&path)).expect("installed");
        assert_eq!((def.name.as_str(), found), ("pyright", pyright));
        assert!(installed(&defs, "rust", Some(&path)).is_none());
    }
}
