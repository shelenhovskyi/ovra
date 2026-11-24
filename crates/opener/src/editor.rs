//! Works out which commands could open a file, in order of preference:
//! `$VISUAL`, `$EDITOR`, then the platform's default handler.

use std::ffi::{OsStr, OsString};
use std::path::Path;

/// The platform whose default file handler to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    Macos,
    Windows,
    /// Linux and the BSDs (freedesktop `xdg-open`).
    Unix,
}

impl Platform {
    pub(crate) fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

/// One program to start, with its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub(crate) program: OsString,
    pub(crate) args: Vec<OsString>,
}

/// Editors that need a terminal. Ovra starts editors without one, so these
/// are skipped in favour of the next candidate.
const TERMINAL_EDITORS: &[&str] = &[
    "vi", "vim", "nvim", "nano", "pico", "micro", "hx", "helix", "kak", "ed", "joe", "ne", "mg",
];

/// How an editor takes a line number.
enum LineStyle {
    /// `--goto file:line` (VS Code family).
    Goto,
    /// `+line file` (Emacs, gVim, gedit).
    Plus,
    /// `file:line` (Sublime Text, Zed).
    Suffix,
    /// `--line line file` (JetBrains IDEs, Kate).
    LineFlag,
    /// `-l line file` (TextMate).
    DashL,
}

fn line_style(editor: &str) -> Option<LineStyle> {
    Some(match editor {
        "code" | "code-insiders" | "codium" | "vscodium" | "cursor" | "windsurf" => LineStyle::Goto,
        "emacs" | "emacsclient" | "gvim" | "mvim" | "gedit" => LineStyle::Plus,
        "subl" | "sublime_text" | "zed" | "zeditor" => LineStyle::Suffix,
        "idea" | "pycharm" | "webstorm" | "goland" | "clion" | "rustrover" | "phpstorm"
        | "rider" | "kate" => LineStyle::LineFlag,
        "mate" => LineStyle::DashL,
        _ => return None,
    })
}

/// Commands to try, best first. `env` reads an environment variable.
pub(crate) fn candidates(
    env: impl Fn(&str) -> Option<OsString>,
    platform: Platform,
    file: &Path,
    line: Option<u32>,
) -> Vec<Invocation> {
    let mut out: Vec<Invocation> = ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(env)
        .filter_map(|value| from_editor_var(&value, file, line))
        .collect();
    out.dedup();
    out.push(system_handler(platform, file));
    out
}

/// Builds an invocation from `$VISUAL`/`$EDITOR`, which may hold arguments
/// (`code --wait`). Returns `None` when unset, blank or a terminal editor.
fn from_editor_var(value: &OsStr, file: &Path, line: Option<u32>) -> Option<Invocation> {
    let (program, mut args): (OsString, Vec<OsString>) = match value.to_str() {
        Some(text) => {
            let mut words = text.split_whitespace().map(OsString::from);
            (words.next()?, words.collect())
        }
        // Not UTF-8: treat the whole value as the program path.
        None => (value.to_owned(), Vec::new()),
    };
    let name = editor_name(&program);
    if TERMINAL_EDITORS.contains(&name.as_str()) || runs_in_terminal(&name, &args) {
        return None;
    }
    let file = file.as_os_str().to_owned();
    match line.zip(line_style(&name)) {
        None => args.push(file),
        Some((l, style)) => match style {
            LineStyle::Goto => args.extend(["--goto".into(), with_suffix(file, l)]),
            LineStyle::Suffix => args.push(with_suffix(file, l)),
            LineStyle::Plus => args.extend([format!("+{l}").into(), file]),
            LineStyle::LineFlag => args.extend(["--line".into(), l.to_string().into(), file]),
            LineStyle::DashL => args.extend(["-l".into(), l.to_string().into(), file]),
        },
    }
    Some(Invocation { program, args })
}

/// `/usr/bin/code` or `C:\Tools\Code.exe` -> `code`, on any host.
fn editor_name(program: &OsStr) -> String {
    let program = program.to_string_lossy().to_lowercase();
    let base = program.rsplit(['/', '\\']).next().unwrap_or_default();
    base.strip_suffix(".exe").unwrap_or(base).to_owned()
}

/// Emacs is graphical unless told to use the terminal.
fn runs_in_terminal(name: &str, args: &[OsString]) -> bool {
    name.starts_with("emacs")
        && args.iter().any(|a| {
            matches!(
                a.to_str(),
                Some("-nw" | "-t" | "--tty" | "--no-window-system")
            )
        })
}

fn with_suffix(file: OsString, line: u32) -> OsString {
    let mut s = file;
    s.push(format!(":{line}"));
    s
}

fn system_handler(platform: Platform, file: &Path) -> Invocation {
    let program = match platform {
        Platform::Macos => "open",
        // `explorer <file>` uses the registered handler and, unlike
        // `cmd /c start`, does not interpret `&` or `^` in the path.
        Platform::Windows => "explorer",
        Platform::Unix => "xdg-open",
    };
    Invocation {
        program: program.into(),
        args: vec![file.as_os_str().to_owned()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(vars: &[(&str, &str)], platform: Platform, line: Option<u32>) -> Vec<String> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let env = |name: &str| {
            vars.iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| OsString::from(v))
        };
        candidates(env, platform, Path::new("/p/src/main.rs"), line)
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
    fn falls_back_to_the_platform_handler() {
        let file = "/p/src/main.rs";
        assert_eq!(
            plan(&[], Platform::Unix, None),
            [format!("xdg-open {file}")]
        );
        assert_eq!(
            plan(&[], Platform::Macos, Some(3)),
            [format!("open {file}")]
        );
        assert_eq!(
            plan(&[], Platform::Windows, None),
            [format!("explorer {file}")]
        );
    }

    #[test]
    fn prefers_visual_then_editor() {
        let vars = [("EDITOR", "subl -n"), ("VISUAL", "code --wait")];
        assert_eq!(
            plan(&vars, Platform::Unix, None),
            [
                "code --wait /p/src/main.rs",
                "subl -n /p/src/main.rs",
                "xdg-open /p/src/main.rs",
            ]
        );
    }

    #[test]
    fn passes_the_line_in_each_editor_style() {
        let cases = [
            ("code", "code --goto /p/src/main.rs:7"),
            (
                "/opt/Cursor/cursor",
                "/opt/Cursor/cursor --goto /p/src/main.rs:7",
            ),
            ("emacsclient -c", "emacsclient -c +7 /p/src/main.rs"),
            ("zed", "zed /p/src/main.rs:7"),
            ("idea", "idea --line 7 /p/src/main.rs"),
            ("mate", "mate -l 7 /p/src/main.rs"),
            (
                r"C:\Tools\Code.exe",
                r"C:\Tools\Code.exe --goto /p/src/main.rs:7",
            ),
            ("unknown-editor", "unknown-editor /p/src/main.rs"),
        ];
        for (editor, expected) in cases {
            let got = plan(&[("VISUAL", editor)], Platform::Unix, Some(7));
            assert_eq!(got[0], expected, "VISUAL={editor}");
        }
    }

    #[test]
    fn skips_terminal_editors_and_blank_values() {
        let vars = [("VISUAL", "  "), ("EDITOR", "nvim")];
        assert_eq!(
            plan(&vars, Platform::Macos, Some(2)),
            ["open /p/src/main.rs"]
        );
        let vars = [("EDITOR", "emacs -nw")];
        assert_eq!(
            plan(&vars, Platform::Unix, None),
            ["xdg-open /p/src/main.rs"]
        );
    }

    #[test]
    fn does_not_repeat_the_same_editor() {
        let vars = [("VISUAL", "zed"), ("EDITOR", "zed")];
        assert_eq!(plan(&vars, Platform::Unix, None).len(), 2);
    }
}
