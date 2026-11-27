//! Quiet mode (ADR 0015, `docs/specs/security.md`): with it on, a call no
//! project rule decides runs without a question when the sandbox would
//! contain it.

use std::path::Path;

use crate::protocol::agents::{SandboxNetwork, SandboxReport, SessionMode, ToolCall, ToolKind};
use crate::protocol::policy::{Policy, PolicyAction};

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

/// File tools whose deny and ask rules quiet mode honours for every call
/// that reads or names a path, whatever its tool: a `Read(.env)` deny must
/// also keep a quiet `Grep` or `cat` off `.env`.
const PATH_TOOLS: [&str; 6] = [
    "Read",
    "NotebookRead",
    "Edit",
    "MultiEdit",
    "Write",
    "NotebookEdit",
];

/// Whether quiet mode allows `call` of `tool` in a session of `mode` in
/// `work_dir`, run as `sandbox` says, under `policy`.
pub(crate) fn allows(
    policy: &Policy,
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
    if !policy.quiet || !trusted || tool.starts_with("mcp__") {
        return false;
    }
    // Changes and commands only in a worktree: in the person's checkout
    // (direct) their own tools may run an edit before any review, and a
    // research session changes nothing.
    let own_copy = mode == SessionMode::Isolated;
    let held = |paths: &[String]| held_by_rule(policy, work_dir, paths);
    match call.kind {
        ToolKind::Read => {
            !call.paths.is_empty()
                && call.paths.iter().all(|p| kept_in(work_dir, p))
                && !held(&call.paths)
        }
        ToolKind::Search => call.paths.iter().all(|p| kept_in(work_dir, p)) && !held(&call.paths),
        ToolKind::Edit | ToolKind::Delete | ToolKind::Move => {
            own_copy && edits(work_dir, call) && !held(&call.paths)
        }
        ToolKind::Execute => {
            own_copy
                && call.command.as_deref().is_some_and(|c| {
                    command(work_dir, c)
                        && !held(&words(c).iter().map(|w| bare(w)).collect::<Vec<_>>())
                })
        }
        ToolKind::Fetch => call.url.as_deref().is_some_and(fetch),
        ToolKind::Think | ToolKind::Subagent => true,
        ToolKind::Other => false,
    }
}

/// Whether a deny or ask rule of a file tool covers one of `paths` (or
/// every call, without a match).
fn held_by_rule(policy: &Policy, work_dir: &Path, paths: &[String]) -> bool {
    policy
        .rules
        .iter()
        .filter(|r| r.action != PolicyAction::Auto && PATH_TOOLS.contains(&r.tool.as_str()))
        .any(|rule| match &rule.pattern {
            None => true,
            Some(pattern) => paths.iter().any(|path| {
                path.split('=').any(|part| {
                    relative(work_dir, part)
                        .is_some_and(|p| !p.is_empty() && super::glob(pattern, &p))
                })
            }),
        })
}

/// A command's words, with the shell's joins and redirections as
/// separators too, so `a;git push` and `x>CLAUDE.md` show their parts.
fn words(command: &str) -> Vec<&str> {
    command
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '<' | '>' | '(' | ')'))
        .filter(|w| !w.is_empty())
        .collect()
}

/// A word without quotes and backslashes.
fn bare(word: &str) -> String {
    word.chars()
        .filter(|c| !matches!(c, '"' | '\'' | '\\'))
        .collect()
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
    // The runtime or the shell may expand these to the home folder, where
    // the agent's own login lives.
    if path.starts_with('~') || path.contains(['$', '%']) {
        return None;
    }
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
/// git only reading, no protected path, home folder or agent login named.
fn command(work_dir: &Path, command: &str) -> bool {
    if HIDDEN.iter().any(|h| command.contains(h)) {
        return false;
    }
    let words = words(command);
    let names_kept_out = words.iter().any(|w| {
        w.split('=').any(|part| {
            let bare = bare(part);
            let bare = bare.strip_prefix("./").unwrap_or(&bare);
            let named = |path: &str| crate::protected::changes(path, None) || in_git(path);
            // `~`, variables and globs from `/` reach the home folder and
            // the agent's login under names this check never sees.
            let home = bare.starts_with('~')
                || bare.contains(['$', '%'])
                || (bare.starts_with('/') && bare.contains(['*', '?', '[', '{']));
            let login = bare.split('/').any(|seg| {
                let seg = seg.to_ascii_lowercase();
                crate::protected::AGENT_STATE_IN_HOME.contains(&seg.as_str())
            });
            home || login || named(bare) || relative(work_dir, bare).is_some_and(|p| named(&p))
        })
    });
    !names_kept_out && git_reads_only(&words)
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
