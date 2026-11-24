//! Project policies: rules that decide a tool call before the person is
//! asked (`docs/specs/security.md`). A `deny` rule wins; an `auto` rule only
//! counts in a sandboxed session whose network is limited; everything else
//! asks.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::Core;
use crate::protocol::agents::{SandboxNetwork, SandboxReport};
use crate::protocol::policy::{
    Policy, PolicyAction, PolicyEvent, PolicyRequest, PolicyResponse, PolicyRule,
};
use crate::protocol::{ErrorCode, ProtocolError, Response};

/// Tools whose main argument is a file, and the input field naming it.
const FILE_TOOLS: [(&str, &str); 6] = [
    ("Read", "file_path"),
    ("Edit", "file_path"),
    ("MultiEdit", "file_path"),
    ("Write", "file_path"),
    ("NotebookEdit", "notebook_path"),
    ("NotebookRead", "notebook_path"),
];

/// Shell syntax that runs more than the matched command: such a command
/// never matches an `auto` rule.
const SHELL_JOINS: [&str; 8] = [";", "&", "|", "`", "$(", ">", "<", "\n"];

pub(crate) fn handle(core: &Core, request: PolicyRequest) -> Result<Response, ProtocolError> {
    let store = core.project_store();
    match request {
        PolicyRequest::Get { project } => Ok(Response::Policy(PolicyResponse::Policy(
            store.policy(&project)?,
        ))),
        PolicyRequest::Set { project, policy } => {
            for rule in &policy.rules {
                check(rule)?;
            }
            store.set_policy(&project, &policy)?;
            core.emit(PolicyEvent::Changed { project });
            Ok(Response::Policy(PolicyResponse::Policy(policy)))
        }
    }
}

fn check(rule: &PolicyRule) -> Result<(), ProtocolError> {
    let bad = |why: &str| {
        Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!("{}: {why}", text(rule)),
        ))
    };
    if rule.tool.trim().is_empty() || rule.tool.trim() != rule.tool {
        return bad("the tool name is empty or has spaces around it");
    }
    let Some(pattern) = &rule.pattern else {
        return Ok(());
    };
    if pattern.trim().is_empty() {
        return bad("the match is empty; leave it out to match every call");
    }
    if file_field(&rule.tool).is_some()
        && (pattern.starts_with('/')
            || pattern.starts_with('\\')
            || pattern.contains(':')
            || pattern.split(['/', '\\']).any(|s| s == ".."))
    {
        return bad("a path glob is relative to the session's folder and stays inside it");
    }
    Ok(())
}

/// A rule as people read it: `Bash(npm test)`, or `WebFetch` for every call.
pub(crate) fn text(rule: &PolicyRule) -> String {
    match &rule.pattern {
        Some(pattern) => format!("{}({pattern})", rule.tool),
        None => rule.tool.clone(),
    }
}

/// What `policy` says about one call of `tool` in a session working in
/// `work_dir` and run as `sandbox` reports: the action and the rule that
/// decided it, or `None` when the person is asked as usual.
pub(crate) fn decide(
    policy: &Policy,
    tool: &str,
    input: &Value,
    work_dir: &Path,
    sandbox: Option<&SandboxReport>,
) -> Option<(PolicyAction, String)> {
    let matching = |action: PolicyAction| {
        policy
            .rules
            .iter()
            .filter(|r| r.action == action && r.tool == tool)
            .find(|r| matches(r, input, work_dir, action))
            .map(|r| (action, text(r)))
    };
    if let Some(denied) = matching(PolicyAction::Deny) {
        return Some(denied);
    }
    // Skipping the question is safe only where the sandbox contains a
    // mistake, and only if the runtime said it ran one.
    let contained = sandbox.is_some_and(|s| s.network != SandboxNetwork::Open);
    if contained {
        return matching(PolicyAction::Auto);
    }
    None
}

fn matches(rule: &PolicyRule, input: &Value, work_dir: &Path, action: PolicyAction) -> bool {
    let Some(pattern) = &rule.pattern else {
        return true;
    };
    if rule.tool == "Bash" {
        let Some(command) = input.get("command").and_then(Value::as_str) else {
            return false;
        };
        return if action == PolicyAction::Auto {
            !SHELL_JOINS.iter().any(|j| command.contains(j)) && starts_with_words(command, pattern)
        } else {
            // A deny looks at every part of a joined command.
            command
                .split(['\n', ';', '&', '|'])
                .any(|part| starts_with_words(part, pattern))
        };
    }
    let Some(field) = file_field(&rule.tool) else {
        return false;
    };
    let Some(path) = input.get(field).and_then(Value::as_str) else {
        return false;
    };
    match inside(work_dir, Path::new(path)) {
        Some(rel) => glob(pattern, &rel),
        None => false,
    }
}

fn file_field(tool: &str) -> Option<&'static str> {
    FILE_TOOLS
        .iter()
        .find(|(name, _)| *name == tool)
        .map(|(_, field)| *field)
}

/// Whether `command` starts with the words of `prefix`, on a word boundary.
fn starts_with_words(command: &str, prefix: &str) -> bool {
    let words = |s: &str| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    let (command, prefix) = (words(command), words(prefix));
    !prefix.is_empty() && command.starts_with(&prefix)
}

/// `path` relative to `work_dir`, with `/` separators, if it stays inside.
/// Lexical: the sandbox, not this, contains symbolic links.
fn inside(work_dir: &Path, path: &Path) -> Option<String> {
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        work_dir.join(path)
    };
    let mut clean = PathBuf::new();
    for part in full.components() {
        match part {
            Component::ParentDir => {
                if !clean.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            other => clean.push(other),
        }
    }
    let rel = clean.strip_prefix(work_dir).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// `*` within a segment, `?` one character, `**` any number of segments.
/// Case-insensitive where file names are (macOS, Windows).
fn glob(pattern: &str, path: &str) -> bool {
    let fold = |s: &str| {
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            s.to_lowercase()
        } else {
            s.to_owned()
        }
    };
    let (pattern, path) = (fold(pattern).replace('\\', "/"), fold(path));
    let pattern: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let path: Vec<&str> = path.split('/').collect();
    segments(&pattern, &path)
}

fn segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|skip| segments(rest, &path[skip..])),
        Some((first, rest)) => match path.split_first() {
            Some((name, more)) => segment(first, name) && segments(rest, more),
            None => false,
        },
    }
}

fn segment(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    fn go(p: &[char], n: &[char]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some(('*', rest)) => (0..=n.len()).any(|skip| go(rest, &n[skip..])),
            Some(('?', rest)) => !n.is_empty() && go(rest, &n[1..]),
            Some((c, rest)) => n.first() == Some(c) && go(rest, &n[1..]),
        }
    }
    go(&p, &n)
}

#[cfg(test)]
mod tests;
