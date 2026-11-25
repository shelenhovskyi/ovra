//! Onboarding: finding the user's `claude`, checking its version against the
//! tested range and asking it whether the user is signed in. Ovra only runs
//! the unmodified binary and reads its answers; it never looks at
//! credentials (ADR 0006).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ovra_core::protocol::agents::{AgentSetup, Billing, SetupState};

/// Oldest version the protocol was checked against (the 2026-11-24 spike).
const MIN_VERSION: Version = Version(2, 1, 331);
/// First version that is not supported: a new major may change the
/// undocumented control messages.
const MAX_VERSION: Version = Version(3, 0, 0);
/// `claude --version` and `claude auth status` normally answer in well
/// under a second.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version(u32, u32, u32);

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// Reads `2.1.331` from output like `2.1.331 (Claude Code)`.
fn parse_version(output: &str) -> Option<Version> {
    let word = output.split_whitespace().next()?;
    let mut parts = word.split('.').map(|p| p.parse::<u32>().ok());
    let version = Version(parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// Why an executable cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Problem {
    /// The chosen path does not exist, or nothing was found.
    NotFound(Option<PathBuf>),
    Outdated(Version),
    TooNew(Version),
    /// It could not be run, or its answer could not be read.
    Broken(String),
}

impl Problem {
    pub(crate) fn state(&self) -> SetupState {
        match self {
            Self::NotFound(_) => SetupState::NotFound,
            Self::Outdated(_) => SetupState::Outdated {
                minimum: MIN_VERSION.to_string(),
            },
            Self::TooNew(_) => SetupState::TooNew {
                below: MAX_VERSION.to_string(),
            },
            Self::Broken(message) => SetupState::Broken {
                message: message.clone(),
            },
        }
    }
}

/// The message a session that cannot start fails with.
impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(Some(path)) => write!(
                f,
                "Claude could not start: there is no program at {}. In the Agents panel, \
                 choose \"Find automatically\" or another path.",
                path.display()
            ),
            Self::NotFound(None) => f.write_str(
                "claude was not found. Install Claude Code and sign in once by running \
                 `claude` in a terminal, or choose its path in the agent setup.",
            ),
            Self::Outdated(version) => write!(
                f,
                "Claude Code {version} is too old; Ovra needs {MIN_VERSION} or newer. \
                 Update it with `claude update`."
            ),
            Self::TooNew(version) => write!(
                f,
                "Claude Code {version} is newer than this version of Ovra supports \
                 (below {MAX_VERSION}). Update Ovra."
            ),
            Self::Broken(message) => f.write_str(message),
        }
    }
}

/// Whether `output` of `claude --version` is in the tested range.
fn check(output: &str) -> Result<Version, Problem> {
    let Some(version) = parse_version(output) else {
        return Err(Problem::Broken(format!(
            "could not read the version of claude from {:?}",
            output.trim()
        )));
    };
    if version < MIN_VERSION {
        return Err(Problem::Outdated(version));
    }
    if version >= MAX_VERSION {
        return Err(Problem::TooNew(version));
    }
    Ok(version)
}

/// The environment the search reads, so tests can pass their own.
#[derive(Debug, Default)]
pub(crate) struct Env {
    pub(crate) path: Option<OsString>,
    pub(crate) home: Option<PathBuf>,
    /// `%APPDATA%`, where npm puts `claude.cmd` on Windows.
    pub(crate) app_data: Option<PathBuf>,
    /// `Some(PATHEXT)` on Windows, where `claude` is `claude.exe` or `claude.cmd`.
    pub(crate) windows_ext: Option<OsString>,
    /// `ANTHROPIC_API_KEY` is set (its value is never read).
    pub(crate) api_key: bool,
}

impl Env {
    pub(crate) fn current() -> Self {
        let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
        let windows = cfg!(windows);
        Self {
            path: var("PATH"),
            home: var(if windows { "USERPROFILE" } else { "HOME" }).map(PathBuf::from),
            app_data: var("APPDATA").map(PathBuf::from),
            windows_ext: windows.then(|| var("PATHEXT").unwrap_or_else(|| ".EXE;.CMD".into())),
            api_key: var("ANTHROPIC_API_KEY").is_some(),
        }
    }

    /// File names `claude` may have here: on Windows the executable and
    /// script extensions of `PATHEXT`, in its order.
    fn names(&self) -> Vec<OsString> {
        let Some(ext) = &self.windows_ext else {
            return vec!["claude".into()];
        };
        ext.to_string_lossy()
            .split(';')
            .map(|e| e.trim().to_ascii_lowercase())
            .filter(|e| matches!(e.as_str(), ".exe" | ".cmd" | ".bat"))
            .map(|e| format!("claude{e}").into())
            .collect()
    }

    /// Every place to look, best first: `PATH`, then where the installers
    /// put it (a desktop app often starts without the shell's `PATH`).
    fn candidates(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = self
            .path
            .as_deref()
            .map(|p| std::env::split_paths(p).collect())
            .unwrap_or_default();
        if let Some(home) = &self.home {
            dirs.push(home.join(".local").join("bin"));
            dirs.push(home.join(".claude").join("local"));
        }
        if self.windows_ext.is_some() {
            dirs.extend(self.app_data.iter().map(|a| a.join("npm")));
        } else {
            dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
        }
        let names = self.names();
        dirs.iter()
            .filter(|dir| dir.is_absolute())
            .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
            .collect()
    }
}

/// The executable to run: the chosen one, or the first one found.
pub(crate) fn find(chosen: Option<&Path>, env: &Env) -> Result<PathBuf, Problem> {
    if let Some(path) = chosen {
        return if path.is_file() {
            Ok(path.to_owned())
        } else {
            Err(Problem::NotFound(Some(path.to_owned())))
        };
    }
    env.candidates()
        .into_iter()
        .find(|p| p.is_file())
        .ok_or(Problem::NotFound(None))
}

/// Runs `executable` with `args` (in the account folder `config_dir`, if
/// given) and returns its stdout and whether it exited successfully.
async fn ask(
    executable: &Path,
    args: &[&str],
    config_dir: Option<&Path>,
) -> Result<(String, bool), Problem> {
    let run = crate::launch::command(executable, config_dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let what = || format!("{} {}", executable.display(), args.join(" "));
    match tokio::time::timeout(ANSWER_TIMEOUT, run).await {
        Err(_) => Err(Problem::Broken(format!("{} did not answer", what()))),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(Problem::NotFound(Some(executable.to_owned())))
        }
        Ok(Err(e)) => Err(Problem::Broken(format!("could not run {}: {e}", what()))),
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            if output.status.success() || !stdout.trim().is_empty() {
                return Ok((stdout, output.status.success()));
            }
            Err(Problem::Broken(format!(
                "{} failed: {}",
                what(),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }
}

/// Runs `claude --version` and checks the result against the tested range.
pub(crate) async fn check_version(executable: &Path) -> Result<Version, Problem> {
    match ask(executable, &["--version"], None).await? {
        (stdout, true) => check(&stdout),
        (stdout, false) => Err(Problem::Broken(format!(
            "{} --version failed: {}",
            executable.display(),
            stdout.trim()
        ))),
    }
}

/// Reads `loggedIn` from `claude auth status --json`; `None` if the answer
/// does not say.
fn signed_in(output: &str) -> Option<bool> {
    let status: serde_json::Value = serde_json::from_str(output.trim()).ok()?;
    status.get("loggedIn")?.as_bool()
}

/// Reads who pays from `authMethod` and `apiProvider` of `claude auth status
/// --json` (values of Claude Code 2.1.331); `None` for anything else.
fn billing(output: &str) -> Option<Billing> {
    let status: serde_json::Value = serde_json::from_str(output.trim()).ok()?;
    match status.get("authMethod")?.as_str()? {
        "claude.ai" | "oauth_token" => Some(Billing::Subscription),
        "api_key" | "api_key_helper" => Some(Billing::ApiKey),
        "third_party" => Some(Billing::Cloud {
            provider: status.get("apiProvider")?.as_str()?.to_owned(),
        }),
        _ => None,
    }
}

/// The whole onboarding check, for the account whose folder is
/// `config_dir` (`None`: the default account).
pub(crate) async fn setup(
    chosen: Option<PathBuf>,
    config_dir: Option<&Path>,
    env: &Env,
) -> AgentSetup {
    let mut setup = AgentSetup {
        path: chosen.as_ref().map(|p| p.to_string_lossy().into_owned()),
        executable: None,
        version: None,
        state: SetupState::Ready,
        billing: None,
        api_key_in_env: env.api_key,
    };
    let executable = match find(chosen.as_deref(), env) {
        Ok(executable) => executable,
        Err(problem) => {
            setup.state = problem.state();
            return setup;
        }
    };
    setup.executable = Some(executable.to_string_lossy().into_owned());
    match check_version(&executable).await {
        Ok(version) => setup.version = Some(version.to_string()),
        Err(problem) => {
            if let Problem::Outdated(v) | Problem::TooNew(v) = &problem {
                setup.version = Some(v.to_string());
            }
            setup.state = problem.state();
            return setup;
        }
    }
    // A folder that does not exist yet has no sign-in; asking `claude`
    // could make it.
    if config_dir.is_some_and(|dir| !dir.is_dir()) {
        setup.state = SetupState::SignedOut;
        return setup;
    }
    // An answer that cannot be read does not block: a session that is
    // really signed out fails with Claude's own message.
    if let Ok((stdout, _)) = ask(&executable, &["auth", "status", "--json"], config_dir).await {
        if signed_in(&stdout) == Some(false) {
            setup.state = SetupState::SignedOut;
        } else {
            setup.billing = billing(&stdout);
        }
    }
    setup
}

/// Folders in the home folder that look like another account's Claude
/// Code configuration (`~/.claude-work`, `~/.claude.personal`): a name that
/// starts with `.claude` and holds what `claude` writes there. `~/.claude`
/// itself is the default account's. Names only; nothing inside is read.
pub(crate) fn config_dirs(env: &Env) -> Vec<PathBuf> {
    let Some(entries) = env.home.as_deref().and_then(|h| std::fs::read_dir(h).ok()) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(".claude")
                && name != ".claude"
                && entry.file_type().is_ok_and(|t| t.is_dir())
        })
        .map(|entry| entry.path())
        .filter(|dir| {
            [
                ".claude.json",
                "settings.json",
                "projects",
                ".credentials.json",
            ]
            .iter()
            .any(|marker| dir.join(marker).exists())
        })
        .collect();
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_accounts_folders_are_found_by_name_and_contents() {
        let home = tempfile::TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        let make = |dir: &str, marker: Option<&str>| {
            let dir = home.path().join(dir);
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
            if let Some(marker) = marker {
                std::fs::write(dir.join(marker), "{}").unwrap_or_else(|e| panic!("{e}"));
            }
        };
        make(".claude", Some("settings.json"));
        make(".claude-work", Some(".claude.json"));
        make(".claude-team", None);
        make(".claude-team/projects", None);
        make(".claude-empty", None);
        make(".claudette", Some("settings.json"));
        make("claude-work", Some(".claude.json"));
        std::fs::write(home.path().join(".claude.json"), "{}").unwrap_or_else(|e| panic!("{e}"));
        let env = Env {
            home: Some(home.path().to_owned()),
            ..Env::default()
        };
        let names: Vec<_> = config_dirs(&env)
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        assert_eq!(names, [".claude-team", ".claude-work", ".claudette"]);
        assert!(config_dirs(&Env::default()).is_empty());
    }

    #[test]
    fn versions_in_the_tested_range_pass() {
        assert_eq!(check("2.1.331 (Claude Code)\n"), Ok(Version(2, 1, 331)));
        assert_eq!(check("2.7.0"), Ok(Version(2, 7, 0)));
        assert_eq!(
            check("2.1.330 (Claude Code)"),
            Err(Problem::Outdated(Version(2, 1, 330)))
        );
        assert_eq!(check("3.0.0"), Err(Problem::TooNew(Version(3, 0, 0))));
        assert!(
            matches!(check("claude 2.1"), Err(Problem::Broken(m)) if m.contains("could not read"))
        );
        assert!(check("2.1.331.4").is_err());
        assert!(check("").is_err());
    }

    #[test]
    fn problems_say_what_to_do() {
        let text = |p: Problem| p.to_string();
        assert!(text(Problem::NotFound(None)).contains("Install Claude Code"));
        assert!(text(Problem::Outdated(Version(2, 0, 0))).contains("claude update"));
        assert!(text(Problem::TooNew(Version(3, 1, 0))).contains("Update Ovra"));
    }

    #[test]
    fn the_sign_in_state_is_read_from_its_own_field_only() {
        assert_eq!(
            signed_in(r#"{"loggedIn": true, "authMethod": "x"}"#),
            Some(true)
        );
        assert_eq!(signed_in("{\"loggedIn\": false}\n"), Some(false));
        assert_eq!(signed_in("Login method: Claude account"), None);
        assert_eq!(signed_in(r#"{"loggedIn": "yes"}"#), None);
    }

    #[test]
    fn the_sign_in_says_who_pays() {
        let paid = |method: &str| {
            billing(&format!(
                r#"{{"loggedIn": true, "authMethod": "{method}", "apiProvider": "bedrock"}}"#
            ))
        };
        assert_eq!(paid("claude.ai"), Some(Billing::Subscription));
        assert_eq!(paid("oauth_token"), Some(Billing::Subscription));
        assert_eq!(paid("api_key"), Some(Billing::ApiKey));
        assert_eq!(paid("api_key_helper"), Some(Billing::ApiKey));
        assert_eq!(
            paid("third_party"),
            Some(Billing::Cloud {
                provider: "bedrock".into()
            })
        );
        assert_eq!(paid("none"), None);
        assert_eq!(billing("not json"), None);
    }

    fn env(path: &[PathBuf], home: PathBuf, windows_ext: Option<&str>) -> Env {
        Env {
            path: std::env::join_paths(path).ok(),
            home: Some(home),
            app_data: Some(std::env::temp_dir().join("roaming")),
            windows_ext: windows_ext.map(OsString::from),
            api_key: false,
        }
    }

    #[test]
    fn the_search_tries_path_then_the_installers_places() {
        let base = std::env::temp_dir();
        let path = [base.join("bin"), PathBuf::from("relative")];
        let unix = env(&path, base.join("home"), None).candidates();
        assert_eq!(
            unix[..3],
            [
                base.join("bin/claude"),
                base.join("home/.local/bin/claude"),
                base.join("home/.claude/local/claude"),
            ]
        );
        assert!(unix.iter().all(|p| p.is_absolute()));
        let windows = env(&[], base.join("home"), Some(".COM;.EXE;.BAT;.CMD;.VBS")).candidates();
        let names: Vec<_> = windows
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        assert_eq!(names[..3], ["claude.exe", "claude.bat", "claude.cmd"]);
        assert_eq!(
            windows.last(),
            Some(&base.join("roaming").join("npm").join("claude.cmd"))
        );
    }

    #[test]
    fn a_chosen_path_is_used_as_is_and_must_exist() {
        let dir = tempfile::TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        let empty = Env::default();
        let missing = dir.path().join("claude");
        assert_eq!(
            find(Some(&missing), &empty),
            Err(Problem::NotFound(Some(missing.clone())))
        );
        std::fs::write(&missing, "").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(find(Some(&missing), &empty), Ok(missing.clone()));
        assert_eq!(find(None, &empty), Err(Problem::NotFound(None)));
        let on_path = env(&[dir.path().to_owned()], dir.path().join("home"), None);
        assert_eq!(find(None, &on_path), Ok(missing));
    }
}
