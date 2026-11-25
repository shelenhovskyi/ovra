//! The environment of a `claude` session that runs without a sandbox (no
//! sandbox on this OS yet): the same allowlist the sandboxes apply, so
//! tokens such as `GITHUB_TOKEN`, `NPM_TOKEN`, `AWS_*` keys,
//! `CLAUDE_CODE_OAUTH_TOKEN` and `SSH_AUTH_SOCK` stay out there too.

use std::ffi::{OsStr, OsString};

use ovra_core::services::CommandLine;

/// The names an unsandboxed session keeps from Ovra's environment:
/// `ENV_KEPT` in `crates/sandbox-linux/src/plan.rs` (this crate cannot
/// depend on it), plus what that sandbox sets itself and the session needs
/// without it: `PATH`, `TMPDIR`, the person's proxy and CA bundle variables.
/// Whole names only: a prefix such as `CLAUDE_CODE_` would let
/// `CLAUDE_CODE_OAUTH_TOKEN` through.
const KEPT: [&str; 50] = [
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
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "ANTHROPIC_BEDROCK_BASE_URL",
    "ANTHROPIC_VERTEX_BASE_URL",
    "ANTHROPIC_VERTEX_PROJECT_ID",
    "ANTHROPIC_FOUNDRY_RESOURCE",
    "ANTHROPIC_FOUNDRY_BASE_URL",
    "AWS_REGION",
    "CLOUD_ML_REGION",
    "GOOGLE_CLOUD_PROJECT",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "NVM_DIR",
    "GOPATH",
    "OVRA_FAKE_CLAUDE_FIXTURE",
    // Set by the Linux sandbox itself.
    "PATH",
    "TMPDIR",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
    "GIT_SSL_CAINFO",
    // The person's proxy, which the sandbox replaces with its netproxy.
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
    "NODE_USE_ENV_PROXY",
];

/// What `claude` needs on Windows, as the Windows sandbox keeps it
/// (`ENV_KEPT` in `crates/sandbox-windows/src/plan.rs`), plus `TEMP` and
/// `TMP`, which that sandbox sets itself.
const WINDOWS_KEPT: [&str; 25] = [
    "USERNAME",
    "CLAUDE_CODE_GIT_BASH_PATH",
    "NVM_HOME",
    "NVM_SYMLINK",
    "SystemRoot",
    "SystemDrive",
    "windir",
    "ComSpec",
    "PATHEXT",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "CommonProgramFiles",
    "CommonProgramFiles(x86)",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
    "TEMP",
    "TMP",
];

/// The names of `outer` (Ovra's environment) a `claude` process must not
/// get.
pub(crate) fn removed(
    outer: impl IntoIterator<Item = OsString>,
    windows: bool,
) -> impl Iterator<Item = OsString> {
    let kept = move |name: &OsStr| {
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
    outer.into_iter().filter(move |name| !kept(name))
}

/// `line` for running without a sandbox: every name of `outer` (Ovra's
/// environment) that is not kept is removed first, then the line's own
/// variables apply.
pub(crate) fn unsandboxed(
    line: CommandLine,
    outer: impl IntoIterator<Item = OsString>,
    windows: bool,
) -> CommandLine {
    let mut env: Vec<(OsString, Option<OsString>)> =
        removed(outer, windows).map(|name| (name, None)).collect();
    env.extend(line.env);
    CommandLine { env, ..line }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names of the list `name` in `source`. Read as text, a test only:
    /// this crate cannot depend on the sandboxes, so the crate graph does
    /// not see this.
    fn list<'a>(source: &'a str, name: &str) -> Vec<&'a str> {
        let list = source
            .split_once(&format!("{name}: [&str; "))
            .and_then(|(_, rest)| rest.split_once("];"))
            .map(|(list, _)| list)
            .unwrap_or_else(|| panic!("{name} not found in the sandbox's plan.rs"));
        // Names only: comments there hold no quotes.
        list.split('"').skip(1).step_by(2).collect()
    }

    #[test]
    fn the_names_kept_are_the_sandboxes_plus_what_they_set_themselves() {
        let linux = include_str!("../../sandbox-linux/src/plan.rs");
        let mut expected = list(linux, "ENV_KEPT");
        expected.extend(list(linux, "CA_BUNDLE_VARS"));
        expected.extend(["PATH", "TMPDIR", "NODE_USE_ENV_PROXY"]);
        expected.extend(["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"]);
        expected.extend(["http_proxy", "https_proxy", "all_proxy", "no_proxy"]);
        expected.sort_unstable();
        expected.dedup();
        let mut kept = KEPT.to_vec();
        kept.sort_unstable();
        assert_eq!(kept, expected);

        let windows = include_str!("../../sandbox-windows/src/plan.rs");
        for name in list(windows, "ENV_KEPT") {
            let found = KEPT.iter().chain(&WINDOWS_KEPT).any(|k| *k == name);
            assert!(found, "{name} of the Windows sandbox is not kept");
        }
    }

    fn line() -> CommandLine {
        CommandLine {
            program: "/usr/bin/claude".into(),
            args: vec!["-p".into()],
            env: vec![
                ("CLAUDECODE".into(), None),
                (
                    "CLAUDE_CONFIG_DIR".into(),
                    Some("/home/u/.claude-work".into()),
                ),
            ],
            cwd: "/work/a".into(),
        }
    }

    const OUTER: [&str; 12] = [
        "PATH",
        "HOME",
        "TMPDIR",
        "HTTPS_PROXY",
        "ANTHROPIC_API_KEY",
        "GITHUB_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "AWS_SECRET_ACCESS_KEY",
        "SSH_AUTH_SOCK",
        "CLAUDE_CONFIG_DIR",
        "SystemRoot",
        "Path",
    ];

    fn removed(line: &CommandLine) -> Vec<String> {
        line.env
            .iter()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn an_unsandboxed_session_keeps_only_the_allowed_names() {
        let line = unsandboxed(line(), OUTER.map(OsString::from), false);
        assert_eq!(
            removed(&line),
            [
                "GITHUB_TOKEN",
                "CLAUDE_CODE_OAUTH_TOKEN",
                "AWS_SECRET_ACCESS_KEY",
                "SSH_AUTH_SOCK",
                "CLAUDE_CONFIG_DIR",
                "SystemRoot",
                "Path",
                "CLAUDECODE",
            ]
        );
        // The line's own come last, so an account's folder wins.
        assert_eq!(
            line.env.last(),
            Some(&(
                "CLAUDE_CONFIG_DIR".into(),
                Some("/home/u/.claude-work".into())
            ))
        );
        assert_eq!(line.program, std::path::Path::new("/usr/bin/claude"));
        assert_eq!(line.args, ["-p"]);
    }

    #[test]
    fn on_windows_it_keeps_what_processes_need_in_any_case() {
        let line = unsandboxed(line(), OUTER.map(OsString::from), true);
        assert_eq!(
            removed(&line),
            [
                "GITHUB_TOKEN",
                "CLAUDE_CODE_OAUTH_TOKEN",
                "AWS_SECRET_ACCESS_KEY",
                "SSH_AUTH_SOCK",
                "CLAUDE_CONFIG_DIR",
                "CLAUDECODE",
            ]
        );
    }
}
