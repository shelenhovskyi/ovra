//! Project policies: rules that decide a tool call before the person is
//! asked (`docs/specs/security.md`). A `deny` rule wins; an `auto` rule only
//! counts in a sandboxed session whose network is limited; everything else
//! asks.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::Core;
use crate::protocol::agents::{SandboxNetwork, SandboxReport};
use crate::protocol::audit::AuditEvent;
use crate::protocol::policy::{
    Policy, PolicyAction, PolicyEvent, PolicyRequest, PolicyResponse, PolicyRule,
};
use crate::protocol::{ErrorCode, ProtocolError, Response};

/// Tools whose main argument is a file, and the input field naming it.
const FILE_TOOLS: [(&str, &str); 5] = [
    ("Read", "file_path"),
    ("Edit", "file_path"),
    ("MultiEdit", "file_path"),
    ("Write", "file_path"),
    ("NotebookEdit", "notebook_path"),
];

/// Shell syntax that runs more than the matched command: such a command
/// never matches an `auto` rule.
const SHELL_JOINS: [&str; 8] = [";", "&", "|", "`", "$(", ">", "<", "\n"];

/// Commands that run whatever their arguments name: a rule for one of
/// them would allow any command, so "Always allow" never offers one.
const RUNNERS: [&str; 22] = [
    "env",
    "sudo",
    "doas",
    "su",
    "xargs",
    "eval",
    "exec",
    "command",
    "nohup",
    "time",
    "timeout",
    "nice",
    "watch",
    "bash",
    "sh",
    "zsh",
    "dash",
    "ksh",
    "fish",
    "pwsh",
    "powershell",
    "cmd",
];

/// The longest command "Always allow" offers a rule for; longer ones are
/// rarely repeated and hard to read on a card.
const MAX_ALWAYS_COMMAND: usize = 200;

/// Held while a `policy.set` compares and stores. One lock for every
/// project and core: changes are rare and quick.
static SETTING: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn handle(core: &Core, request: PolicyRequest) -> Result<Response, ProtocolError> {
    let store = core.project_store();
    match request {
        PolicyRequest::Get { project } => Ok(Response::Policy(PolicyResponse::Policy(
            store.policy(&project)?,
        ))),
        PolicyRequest::Set {
            project,
            policy,
            expected,
        } => {
            for rule in &policy.rules {
                check(rule)?;
            }
            // Comparing and storing under one lock: two changes built from
            // the same policy cannot both be kept.
            let _setting = SETTING
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(expected) = expected
                && store.policy(&project)? != expected
            {
                return Err(ProtocolError::new(
                    ErrorCode::Conflict,
                    "The policy changed since this change was made; nothing was kept.",
                ));
            }
            store.set_policy(&project, &policy)?;
            let event = AuditEvent::PolicyChanged {
                policy: policy.clone(),
            };
            crate::audit::record(core, &project, None, event);
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
    if rule.tool.starts_with(crate::orchestration::IDE_TOOL_PREFIX) {
        return bad("Ovra's own tools follow their role's rules, not policies");
    }
    let Some(pattern) = &rule.pattern else {
        return Ok(());
    };
    if pattern.trim().is_empty() {
        return bad("the match is empty; leave it out to match every call");
    }
    if rule.tool == "Bash" {
        return Ok(());
    }
    if rule.tool == "WebFetch" {
        return match crate::netproxy::parse_host(pattern) {
            Ok(host) if host == *pattern => Ok(()),
            _ => bad("a WebFetch match is one host in lower case, such as docs.rs"),
        };
    }
    if file_field(&rule.tool).is_none() {
        return bad(
            "only Bash, WebFetch and file tools take a match; leave it out to match every call",
        );
    }
    let segments: Vec<&str> = pattern.split(['/', '\\']).collect();
    if pattern.starts_with('/')
        || pattern.starts_with('\\')
        || pattern.contains(':')
        || segments
            .iter()
            .any(|s| *s == ".." || *s == "." || s.is_empty())
    {
        return bad(
            "a path glob is relative to the session's folder, stays inside it and has no empty or `.` parts",
        );
    }
    Ok(())
}

/// The rule "Always allow" offers for a call no project rule decided: the
/// narrowest `auto` rule that matches it, or `None`. Only for a session
/// whose sandbox limits the network (elsewhere `auto` never counts), and
/// never for a path outside `work_dir`, a protected path, a command that
/// runs other commands, or a host where anyone can receive data.
pub(crate) fn always_rule(
    tool: &str,
    input: &Value,
    work_dir: &Path,
    sandbox: Option<&SandboxReport>,
) -> Option<PolicyRule> {
    if !sandbox.is_some_and(|s| s.network != SandboxNetwork::Open) {
        return None;
    }
    let text = |field: &str| input.get(field).and_then(Value::as_str);
    let pattern = match tool {
        "Bash" => command_scope(text("command")?)?,
        // The exact file: a glob could cover a protected path.
        "Edit" | "Write" => {
            let path = inside(work_dir, Path::new(text("file_path")?))?;
            let special = path.contains(['*', '?', '[', ':']);
            (!special && !crate::protected::changes(&path, None)).then_some(path)?
        }
        "WebFetch" => {
            let host = url_host(text("url")?)?;
            let named = host.parse::<std::net::IpAddr>().is_err() && host.contains('.');
            (named && !crate::netproxy::is_broad(&host)).then_some(host)?
        }
        _ => return None,
    };
    let rule = PolicyRule {
        tool: tool.to_owned(),
        pattern: Some(pattern),
        action: PolicyAction::Auto,
        from_approval: true,
    };
    let fits = check(&rule).is_ok() && matches(&rule, input, work_dir, PolicyAction::Auto);
    fits.then_some(rule)
}

/// The command itself, in single spaces, when a rule for it stays narrow:
/// no runner, variable or path outside the folder, no protected path.
fn command_scope(command: &str) -> Option<String> {
    let words: Vec<&str> = command.split_whitespace().collect();
    let first = words.first()?;
    // Each part of `--file=x` too.
    let outside = |w: &&str| {
        w.split('=').any(|part| {
            let bare = part.trim_matches(['"', '\'']);
            let bare = bare.strip_prefix("./").unwrap_or(bare);
            bare.starts_with(['/', '\\', '~', '$', '%'])
                || bare.contains("..")
                || bare.as_bytes().get(1) == Some(&b':')
                || crate::protected::changes(bare, None)
        })
    };
    if SHELL_JOINS.iter().any(|j| command.contains(j))
        || RUNNERS.contains(first)
        || first.contains('=')
        || words.iter().any(outside)
    {
        return None;
    }
    let scope = words.join(" ");
    (scope.len() <= MAX_ALWAYS_COMMAND).then_some(scope)
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
    // An ask rule wins over an auto rule: the stricter one decides.
    if let Some(asked) = matching(PolicyAction::Ask) {
        return Some(asked);
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
    if rule.tool == "WebFetch" {
        let url = input.get("url").and_then(Value::as_str);
        return url.and_then(url_host).is_some_and(|host| host == *pattern);
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

/// The host an `http` or `https` URL names, as the netproxy writes hosts;
/// `None` for anything else, or a URL with user info, which readers may
/// take for the host.
fn url_host(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    if !["http", "https"]
        .iter()
        .any(|s| scheme.eq_ignore_ascii_case(s))
    {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.contains(['@', '\\']) {
        return None;
    }
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') => {
            port.bytes().all(|b| b.is_ascii_digit()).then_some(host)?
        }
        _ => authority,
    };
    crate::netproxy::parse_host(host).ok()
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
    // File names are case-insensitive on macOS and Windows: `/W/A` there is
    // inside `/w`.
    let name = |c: Component<'_>| fold(&c.as_os_str().to_string_lossy());
    let mut parts = clean.components();
    for base in work_dir.components() {
        if parts.next().map(name) != Some(name(base)) {
            return None;
        }
    }
    let parts: Vec<String> = parts.map(name).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// `*` within a segment, `?` one character, `**` any number of segments.
/// Case-insensitive where file names are (macOS, Windows).
fn glob(pattern: &str, path: &str) -> bool {
    let (pattern, path) = (fold(pattern).replace('\\', "/"), fold(path));
    let pattern: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let path: Vec<&str> = path.split('/').collect();
    segments(&pattern, &path)
}

/// A file name as the file system compares it.
fn fold(name: &str) -> String {
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        name.to_lowercase()
    } else {
        name.to_owned()
    }
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
