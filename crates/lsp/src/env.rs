//! The environment of a server that runs unsandboxed: only the names it
//! needs, never Ovra's tokens and keys (`GITHUB_TOKEN`,
//! `ANTHROPIC_API_KEY`, ...).

use std::ffi::{OsStr, OsString};

use ovra_core::services::CommandLine;

/// The names an unsandboxed server keeps from Ovra's environment. The same
/// as `SERVER_ENV_KEPT` in `crates/sandbox-linux/src/plan/server.rs` (this
/// crate cannot depend on it), plus `PATH`, which the sandbox sets itself.
const KEPT: [&str; 20] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_CTYPE",
    "TZ",
    "TERM",
    "COLORTERM",
    "NO_COLOR",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "NVM_DIR",
    "GOPATH",
];

/// What processes need on Windows to start, find their folders and run
/// `cmd` shims (`npm`'s servers are `.cmd` files).
const WINDOWS_KEPT: [&str; 18] = [
    "SystemRoot",
    "SystemDrive",
    "windir",
    "ComSpec",
    "PATHEXT",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "USERNAME",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "ProgramData",
    "NUMBER_OF_PROCESSORS",
];

/// `line` for running without a sandbox: every name of `outer` (Ovra's
/// environment) that is not kept is removed first, then the line's own
/// variables apply.
pub(crate) fn unsandboxed(
    line: CommandLine,
    outer: impl IntoIterator<Item = OsString>,
    windows: bool,
) -> CommandLine {
    let kept = |name: &OsStr| {
        let Some(name) = name.to_str() else {
            return false;
        };
        if windows {
            KEPT.iter()
                .chain(&WINDOWS_KEPT)
                .any(|k| k.eq_ignore_ascii_case(name))
        } else {
            KEPT.contains(&name)
        }
    };
    let mut env: Vec<(OsString, Option<OsString>)> = outer
        .into_iter()
        .filter(|name| !kept(name))
        .map(|name| (name, None))
        .collect();
    env.extend(line.env);
    CommandLine { env, ..line }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_kept_are_the_linux_sandboxs_plus_path() {
        // Read as text, a test only: this crate cannot depend on the
        // sandbox's, so the crate graph does not see this. The list holds
        // names only; a quote in a comment there would break the parse.
        let source = include_str!("../../sandbox-linux/src/plan/server.rs");
        let list = source
            .split_once("SERVER_ENV_KEPT: [&str; ")
            .and_then(|(_, rest)| rest.split_once("];"))
            .map(|(list, _)| list)
            .expect("SERVER_ENV_KEPT in plan/server.rs");
        let mut sandbox: Vec<&str> = list.split('"').skip(1).step_by(2).chain(["PATH"]).collect();
        let mut kept = KEPT.to_vec();
        sandbox.sort_unstable();
        kept.sort_unstable();
        assert_eq!(kept, sandbox);
    }

    fn names(env: &[(OsString, Option<OsString>)]) -> Vec<(String, bool)> {
        env.iter()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.is_some()))
            .collect()
    }

    fn line() -> CommandLine {
        CommandLine {
            program: "/bin/ls".into(),
            args: vec!["--stdio".into()],
            env: vec![
                ("TMPDIR".into(), Some("/data/lsp/p1/rust/tmp".into())),
                ("GITHUB_TOKEN".into(), Some("set by the line".into())),
            ],
            cwd: "/work/a".into(),
        }
    }

    const OUTER: [&str; 9] = [
        "PATH",
        "HOME",
        "GITHUB_TOKEN",
        "ANTHROPIC_API_KEY",
        "AWS_SECRET_ACCESS_KEY",
        "SSH_AUTH_SOCK",
        "CARGO_HOME",
        "SystemRoot",
        "Path",
    ];

    #[test]
    fn an_unsandboxed_server_keeps_only_the_allowed_names() {
        let line = unsandboxed(line(), OUTER.map(OsString::from), false);
        assert_eq!(
            names(&line.env),
            [
                ("GITHUB_TOKEN".into(), false),
                ("ANTHROPIC_API_KEY".into(), false),
                ("AWS_SECRET_ACCESS_KEY".into(), false),
                ("SSH_AUTH_SOCK".into(), false),
                ("SystemRoot".into(), false),
                ("Path".into(), false),
                // The line's own come last, so they win.
                ("TMPDIR".into(), true),
                ("GITHUB_TOKEN".into(), true),
            ]
        );
        assert_eq!(line.program, std::path::Path::new("/bin/ls"));
        assert_eq!(line.args, ["--stdio"]);
    }

    #[test]
    fn on_windows_it_keeps_what_processes_need_in_any_case() {
        let line = unsandboxed(line(), OUTER.map(OsString::from), true);
        let removed: Vec<String> = names(&line.env)
            .into_iter()
            .filter(|(_, set)| !set)
            .map(|(k, _)| k)
            .collect();
        assert_eq!(
            removed,
            [
                "GITHUB_TOKEN",
                "ANTHROPIC_API_KEY",
                "AWS_SECRET_ACCESS_KEY",
                "SSH_AUTH_SOCK"
            ]
        );
    }
}
