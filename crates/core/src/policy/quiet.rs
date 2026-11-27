//! Quiet mode (ADR 0015, `docs/specs/security.md`): with it on, a call no
//! project rule decides runs without a question when the sandbox would
//! contain it.

use std::path::Path;

use crate::protocol::agents::{SandboxNetwork, SandboxReport, SessionMode, ToolCall, ToolKind};

/// What the audit log names as the rule that decided a quiet call.
pub(crate) const QUIET_RULE: &str = "quiet mode";

/// The start of the names of sandboxes Ovra trusts to contain a mistake.
/// The macOS one joins once it is verified on a Mac, the Windows one once
/// its network gaps close.
const QUIET_SANDBOXES: [&str; 1] = ["bubblewrap"];

/// Git's subcommands that only read. Any other may change git state.
const GIT_READS: [&str; 25] = [
    "status",
    "diff",
    "log",
    "show",
    "blame",
    "grep",
    "ls-files",
    "ls-tree",
    "rev-parse",
    "rev-list",
    "describe",
    "shortlog",
    "cat-file",
    "show-ref",
    "merge-base",
    "name-rev",
    "for-each-ref",
    "diff-tree",
    "diff-files",
    "diff-index",
    "check-ignore",
    "check-attr",
    "count-objects",
    "help",
    "version",
];

/// Shell syntax that hides what runs: the check below cannot see inside.
const HIDDEN: [&str; 4] = ["`", "$(", "<(", ">("];

/// Whether quiet mode allows `call` of `tool` in a session of `mode` in
/// `work_dir`, run as `sandbox` says.
pub(crate) fn allows(
    tool: &str,
    call: &ToolCall,
    work_dir: &Path,
    sandbox: Option<&SandboxReport>,
    mode: SessionMode,
) -> bool {
    let trusted = sandbox.is_some_and(|s| {
        s.network != SandboxNetwork::Open
            && QUIET_SANDBOXES.iter().any(|name| s.name.starts_with(name))
    });
    if !trusted || tool.starts_with("mcp__") {
        return false;
    }
    let direct = mode == SessionMode::Direct;
    match call.kind {
        ToolKind::Read | ToolKind::Search => call.paths.iter().all(|p| kept_in(work_dir, p)),
        ToolKind::Edit => edits(work_dir, call),
        ToolKind::Delete | ToolKind::Move => !direct && edits(work_dir, call),
        ToolKind::Execute => {
            !direct
                && call
                    .command
                    .as_deref()
                    .is_some_and(|c| command(work_dir, c))
        }
        ToolKind::Fetch => call.url.as_deref().is_some_and(fetch),
        ToolKind::Think | ToolKind::Subagent => true,
        ToolKind::Other => false,
    }
}

/// Whether `path` is inside `work_dir`, or the folder itself, not
/// protected, marked in reviews or in git's folder.
fn kept_in(work_dir: &Path, path: &str) -> bool {
    relative(work_dir, path)
        .is_some_and(|p| p.is_empty() || !(crate::protected::changes(&p, None) || in_git(&p)))
}

/// `path` relative to `work_dir` (empty for the folder itself), if it
/// stays inside.
fn relative(work_dir: &Path, path: &str) -> Option<String> {
    let below = super::inside(work_dir, &Path::new(path).join("x"))?;
    let path = below.strip_suffix('x')?;
    Some(path.trim_end_matches('/').to_owned())
}

fn edits(work_dir: &Path, call: &ToolCall) -> bool {
    !call.paths.is_empty()
        && call.paths.iter().all(|path| {
            super::inside(work_dir, Path::new(path)).is_some_and(|p| {
                !crate::protected::changes(&p, None) && !super::short_name(&p) && !in_git(&p)
            })
        })
}

/// Whether a part of `path` is git's own folder, as Windows also names it
/// (trailing dots and spaces dropped, `git~1`).
fn in_git(path: &str) -> bool {
    path.split(['/', '\\']).any(|part| {
        let part = part.trim_end_matches(['.', ' ']).to_ascii_lowercase();
        part == ".git" || part.starts_with("git~")
    })
}

/// Whether a command only does what the sandbox contains: nothing hidden,
/// git only reading, no protected path named.
fn command(work_dir: &Path, command: &str) -> bool {
    if HIDDEN.iter().any(|h| command.contains(h)) {
        return false;
    }
    // Words, with the shell's joins and redirections as separators too,
    // so `a;git push` and `x>CLAUDE.md` show their parts.
    let words: Vec<&str> = command
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '<' | '>' | '(' | ')'))
        .filter(|w| !w.is_empty())
        .collect();
    let names_protected = words.iter().any(|w| {
        w.split('=').any(|part| {
            let bare: String = part
                .chars()
                .filter(|c| !matches!(c, '"' | '\'' | '\\'))
                .collect();
            let bare = bare.strip_prefix("./").unwrap_or(&bare);
            let named = |path: &str| crate::protected::changes(path, None) || in_git(path);
            named(bare) || relative(work_dir, bare).is_some_and(|p| named(&p))
        })
    });
    !names_protected && git_reads_only(&words)
}

/// Whether every `git` among `words` runs a reading subcommand, with only
/// `--no-pager` or `-P` before it.
fn git_reads_only(words: &[&str]) -> bool {
    let is_git = |w: &str| super::program(w).iter().any(|name| name == "git");
    words.iter().enumerate().all(|(i, word)| {
        if !is_git(word) {
            return true;
        }
        let bare = |w: &str| -> String {
            w.chars()
                .filter(|c| !matches!(c, '"' | '\'' | '\\'))
                .collect::<String>()
                .to_ascii_lowercase()
        };
        let next = words[i + 1..]
            .iter()
            .map(|w| bare(w))
            .find(|w| w != "--no-pager" && w != "-p");
        next.is_some_and(|sub| GIT_READS.contains(&sub.as_str()))
    })
}

/// Whether a fetch names one plain host, by name, that not anyone can
/// receive data at.
fn fetch(url: &str) -> bool {
    super::url_host(url).is_some_and(|host| {
        host.parse::<std::net::IpAddr>().is_err()
            && host.contains('.')
            && !crate::netproxy::is_broad(&host)
    })
}

#[cfg(test)]
mod tests;
