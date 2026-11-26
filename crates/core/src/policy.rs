//! Project policies: rules that decide a tool call before the person is
//! asked (`docs/specs/security.md`). A `deny` rule wins; an `auto` rule only
//! counts in a sandboxed session whose network is limited; everything else
//! asks.

use std::path::{Component, Path, PathBuf};

use crate::Core;
use crate::protocol::agents::{SandboxNetwork, SandboxReport, ToolCall, ToolKind};
use crate::protocol::audit::{AuditEvent, DecidedBy};
use crate::protocol::permissions::{
    Decision, PermissionAnswer, PermissionId, PermissionRequest, PermissionsEvent,
};
use crate::protocol::policy::{
    Policy, PolicyAction, PolicyEvent, PolicyRequest, PolicyResponse, PolicyRule,
};
use crate::protocol::{ErrorCode, ProtocolError, Response};

/// Rules are written in Claude's syntax (`Bash(git *)`, `Edit(src/**)`,
/// `WebFetch(docs.rs)`): the tools whose rules take a `match`, and what it
/// is tested against, by the call's kind: the command of an `execute`
/// call, the paths of a `read` or `edit` call, the URL of a `fetch` call.
/// The last column says whether "Always allow" offers a rule for the tool.
/// A rule still names the tool: it matches only calls of that name.
const MATCHED_TOOLS: [(&str, ToolKind, bool); 7] = [
    ("Bash", ToolKind::Execute, true),
    ("WebFetch", ToolKind::Fetch, true),
    ("Read", ToolKind::Read, false),
    ("Edit", ToolKind::Edit, true),
    ("MultiEdit", ToolKind::Edit, false),
    ("Write", ToolKind::Edit, true),
    ("NotebookEdit", ToolKind::Edit, false),
];

/// What a rule's `match` is tested against for calls of `tool`; `None`
/// when a rule for it takes no match.
fn matched_kind(tool: &str) -> Option<ToolKind> {
    MATCHED_TOOLS
        .iter()
        .find(|(name, ..)| *name == tool)
        .map(|(_, kind, _)| *kind)
}

/// Shell syntax that runs more than the matched command: such a command
/// never matches an `auto` rule.
const SHELL_JOINS: [&str; 8] = [";", "&", "|", "`", "$(", ">", "<", "\n"];

/// Commands that run whatever their arguments name: a rule for one of
/// them would allow any command, so "Always allow" never offers one.
/// Compared with [`program`]'s names.
const RUNNERS: [&str; 57] = [
    "env",
    "sudo",
    "doas",
    "su",
    "runuser",
    "pkexec",
    "sg",
    "xargs",
    "eval",
    "exec",
    "command",
    "builtin",
    "coproc",
    "trap",
    "source",
    ".",
    "!",
    "nohup",
    "time",
    "timeout",
    "nice",
    "ionice",
    "chrt",
    "taskset",
    "stdbuf",
    "setsid",
    "flock",
    "unshare",
    "nsenter",
    "chroot",
    "fakeroot",
    "systemd-run",
    "script",
    "busybox",
    "strace",
    "parallel",
    "watch",
    "npx",
    "bunx",
    "pnpx",
    "uvx",
    "wsl",
    "start",
    "open",
    "bash",
    "sh",
    "ash",
    "zsh",
    "dash",
    "ksh",
    "mksh",
    "csh",
    "tcsh",
    "fish",
    "pwsh",
    "powershell",
    "cmd",
];

/// Runtimes and tools that take code. Alone, they read it from the
/// terminal or start a prompt, so "Always allow" never offers one
/// without arguments (`git` with no subcommand is in the same place).
const CODE_TAKERS: [&str; 28] = [
    "node",
    "nodejs",
    "python",
    "py",
    "ruby",
    "irb",
    "perl",
    "php",
    "deno",
    "bun",
    "lua",
    "luajit",
    "julia",
    "r",
    "rscript",
    "tclsh",
    "osascript",
    "java",
    "jshell",
    "ghci",
    "groovy",
    "scala",
    "erl",
    "iex",
    "elixir",
    "awk",
    "gawk",
    "git",
];

/// The longest command "Always allow" offers a rule for; longer ones are
/// rarely repeated and hard to read on a card.
const MAX_ALWAYS_COMMAND: usize = 200;

/// Files that say what commands run: an `Edit` rule for one would let the
/// agent change what an allowed command does, so "Always allow" never
/// offers one. Compared by file name, in any case.
const COMMAND_FILES: [&str; 13] = [
    "package.json",
    "makefile",
    "gnumakefile",
    "build.rs",
    "cargo.toml",
    "pyproject.toml",
    "setup.py",
    ".npmrc",
    "justfile",
    ".envrc",
    "lefthook.yml",
    ".pre-commit-config.yaml",
    "rakefile",
];

/// Folders whose files say what commands run, as [`COMMAND_FILES`]: by
/// their path inside the folder, in any case.
const COMMAND_FOLDERS: [&str; 3] = [".github/workflows", ".cargo", ".husky"];

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
    let kind = matched_kind(&rule.tool);
    if rule.exact && (kind != Some(ToolKind::Execute) || rule.pattern.is_none()) {
        return bad("only a Bash rule with a match can match the exact command");
    }
    // An exact deny or ask would let `npm test -- -u` through a rule
    // meant to stop `npm test`.
    if rule.exact && rule.action != PolicyAction::Auto {
        return bad(
            "only an auto rule can match the exact command; a deny or ask matches the prefix",
        );
    }
    let Some(pattern) = &rule.pattern else {
        return Ok(());
    };
    if pattern.trim().is_empty() {
        return bad("the match is empty; leave it out to match every call");
    }
    match kind {
        Some(ToolKind::Execute) => return Ok(()),
        Some(ToolKind::Fetch) => {
            return match crate::netproxy::parse_host(pattern) {
                Ok(host) if host == *pattern => Ok(()),
                _ => bad("a WebFetch match is one host in lower case, such as docs.rs"),
            };
        }
        Some(ToolKind::Read | ToolKind::Edit) => {}
        _ => {
            return bad(
                "only Bash, WebFetch and file tools take a match; leave it out to match every call",
            );
        }
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
    call: &ToolCall,
    work_dir: &Path,
    sandbox: Option<&SandboxReport>,
) -> Option<PolicyRule> {
    if !sandbox.is_some_and(|s| s.network != SandboxNetwork::Open) {
        return None;
    }
    let (_, kind, _) = MATCHED_TOOLS
        .iter()
        .find(|(name, _, always)| *name == tool && *always)?;
    let pattern = match kind {
        ToolKind::Execute => command_scope(call.command.as_deref()?)?,
        // The exact file: a glob could cover a protected path.
        ToolKind::Edit => {
            let [path] = call.paths.as_slice() else {
                return None;
            };
            let path = inside(work_dir, Path::new(path))?;
            let special = path.contains(['*', '?', '[', ':']);
            let kept = special
                || short_name(&path)
                || crate::protected::changes(&path, None)
                || runs_commands(&path);
            (!kept).then_some(path)?
        }
        ToolKind::Fetch => {
            let host = url_host(call.url.as_deref()?)?;
            let named = host.parse::<std::net::IpAddr>().is_err() && host.contains('.');
            (named && !crate::netproxy::is_broad(&host)).then_some(host)?
        }
        _ => return None,
    };
    let rule = PolicyRule {
        tool: tool.to_owned(),
        pattern: Some(pattern),
        action: PolicyAction::Auto,
        // The command offered, not every command it starts.
        exact: *kind == ToolKind::Execute,
        from_approval: true,
    };
    let fits = check(&rule).is_ok() && matches(&rule, call, work_dir, PolicyAction::Auto);
    fits.then_some(rule)
}

/// The command itself, in single spaces, when a rule for it stays narrow:
/// no runner, variable or path outside the folder, no protected path, no
/// word the shell expands.
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
    // Globs, braces and variables expand past that check, into names it
    // never saw; a quote or backslash inside a word can hide what it
    // starts (`.cl\aude` is `.claude`).
    let expands = |w: &&str| {
        w.contains(['*', '?', '[', '{', '$', '\\'])
            || w.char_indices()
                .any(|(i, c)| matches!(c, '"' | '\'') && i != 0 && i + 1 != w.len())
    };
    let program = program(first);
    let runs = |list: &[&str]| program.iter().any(|name| list.contains(&name.as_str()));
    if SHELL_JOINS.iter().any(|j| command.contains(j))
        || runs(&RUNNERS)
        || (words.len() == 1 && runs(&CODE_TAKERS))
        || first.contains('=')
        || words.iter().any(outside)
        || words.iter().any(expands)
    {
        return None;
    }
    let scope = words.join(" ");
    (scope.len() <= MAX_ALWAYS_COMMAND).then_some(scope)
}

/// Whether `path` (relative, `/` separators) is in git's own folder or a
/// file that says what commands run. Windows drops trailing dots and
/// spaces from names and knows `.git` as `git~1` too.
fn runs_commands(path: &str) -> bool {
    let parts: Vec<String> = path
        .split('/')
        .map(|p| p.trim_end_matches(['.', ' ']).to_ascii_lowercase())
        .collect();
    let in_git = parts.iter().any(|p| p == ".git" || p.starts_with("git~"));
    let named = parts
        .last()
        .is_some_and(|name| COMMAND_FILES.contains(&name.as_str()));
    let folder = COMMAND_FOLDERS.iter().any(|folder| {
        let folder: Vec<&str> = folder.split('/').collect();
        parts.len() > folder.len() && parts.iter().zip(&folder).all(|(p, f)| p == f)
    });
    in_git || named || folder
}

/// Whether a part of `path` may be a Windows 8.3 short name, such as
/// `PACKAG~1.JSO` for `package.json`: `~` and a digit. Such a name may
/// stand for any file, those the checks above keep out too.
fn short_name(path: &str) -> bool {
    path.split(['/', '\\']).any(|part| {
        part.as_bytes()
            .windows(2)
            .any(|w| w[0] == b'~' && w[1].is_ascii_digit())
    })
}

/// The names the shell may find for a command's first word: without
/// quotes, backslashes, folder, `.exe` or version (`python3.12`), in lower
/// case. Two when a backslash may be an escape or a Windows folder.
fn program(word: &str) -> Vec<String> {
    let name = |word: &str| {
        let bare: String = word.chars().filter(|c| !matches!(c, '"' | '\'')).collect();
        let bare = bare.rsplit('/').next().unwrap_or_default().to_lowercase();
        let bare = bare.strip_suffix(".exe").unwrap_or(&bare);
        // `.` is a runner of its own; a name is never only a version.
        match bare.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.') {
            "" => bare.to_owned(),
            stem => stem.to_owned(),
        }
    };
    let escaped = name(&word.replace('\\', ""));
    let folder = name(word.rsplit('\\').next().unwrap_or_default());
    if escaped == folder {
        vec![escaped]
    } else {
        vec![escaped, folder]
    }
}

/// "Always allow" on the pending request `id`: takes the request, so no
/// other answer can come between, adds the rule it offers to its project's
/// policy, then answers it. Refuses, adding nothing, when the request is
/// no longer pending, offers no rule, or a project rule now asks about or
/// refuses the call.
pub(crate) fn answer_always(
    core: &Core,
    id: &PermissionId,
    answer: PermissionAnswer,
) -> Result<(), ProtocolError> {
    let bad = |why: &str| Err(ProtocolError::new(ErrorCode::BadRequest, why));
    if answer.decision != Decision::Allow {
        return bad("only an allow can add a rule");
    }
    let gone = || {
        Err(ProtocolError::new(
            ErrorCode::NotFound,
            format!(
                "permission request {} is not pending; no rule was added",
                id.0
            ),
        ))
    };
    let permissions = &core.sessions().permissions;
    // Held until the rule is stored: no policy change comes between the
    // check below and the store.
    let setting = SETTING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(taken) = permissions.take(id) else {
        return gone();
    };
    let request = &taken.request;
    let refused = match (&request.always, &request.cwd) {
        (Some(rule), Some(cwd)) => added(core, request, rule, Path::new(cwd)).err(),
        _ => Some(ProtocolError::new(
            ErrorCode::BadRequest,
            "this request offers no rule to always allow",
        )),
    };
    drop(setting);
    if let Some(refused) = refused {
        // Still pending, for an Allow once or a Deny.
        core.sessions().put_back(taken, &core.sink());
        return Err(refused);
    }
    let decision = answer.decision;
    taken.answer(answer);
    core.emit(PermissionsEvent::Resolved {
        id: id.clone(),
        decision,
    });
    Ok(())
}

/// Adds `rule`, offered by `request`, to its project's policy, unless a
/// project rule now asks about or refuses the call. The caller holds
/// [`SETTING`].
fn added(
    core: &Core,
    request: &PermissionRequest,
    rule: &PolicyRule,
    cwd: &Path,
) -> Result<(), ProtocolError> {
    let store = core.project_store();
    let mut policy = store.policy(&request.project)?;
    // Without a sandbox only deny and ask rules decide.
    if decide(&policy, &request.tool, &request.call, cwd, None).is_some() {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "a project rule now asks about this call; nothing was added",
        ));
    }
    // The same rule, wherever it came from, is there already.
    let same = |r: &PolicyRule| {
        r.tool == rule.tool
            && r.pattern == rule.pattern
            && r.action == rule.action
            && r.exact == rule.exact
    };
    if policy.rules.iter().any(same) {
        return Ok(());
    }
    policy.rules.push(rule.clone());
    store.set_policy(&request.project, &policy)?;
    let event = AuditEvent::RuleAdded {
        rule: rule.clone(),
        by: DecidedBy::Person,
    };
    crate::audit::record(core, &request.project, Some(&request.session), event);
    core.emit(PolicyEvent::Changed {
        project: request.project.clone(),
    });
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
    call: &ToolCall,
    work_dir: &Path,
    sandbox: Option<&SandboxReport>,
) -> Option<(PolicyAction, String)> {
    let matching = |action: PolicyAction| {
        policy
            .rules
            .iter()
            .filter(|r| r.action == action && r.tool == tool)
            .find(|r| matches(r, call, work_dir, action))
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

/// Whether `rule`, which names the call's tool, matches `call`: its
/// `match` is tested against the field the rule's tool takes it for
/// ([`MATCHED_TOOLS`]).
fn matches(rule: &PolicyRule, call: &ToolCall, work_dir: &Path, action: PolicyAction) -> bool {
    let Some(pattern) = &rule.pattern else {
        return true;
    };
    let kind = matched_kind(&rule.tool);
    if kind == Some(ToolKind::Execute) {
        let Some(command) = call.command.as_deref() else {
            return false;
        };
        let fits = |part: &str| {
            // `check` refuses an exact deny or ask; one stored anyway
            // still matches the prefix.
            if rule.exact && action == PolicyAction::Auto {
                words(part) == words(pattern)
            } else {
                starts_with_words(part, pattern)
            }
        };
        return if action == PolicyAction::Auto {
            !SHELL_JOINS.iter().any(|j| command.contains(j)) && fits(command)
        } else {
            // A deny looks at every part of a joined command.
            command.split(['\n', ';', '&', '|']).any(fits)
        };
    }
    if kind == Some(ToolKind::Fetch) {
        return match call.url.as_deref().and_then(url_host) {
            // `auto` allows the host it names and nothing else.
            Some(host) if action == PolicyAction::Auto => host == *pattern,
            // A deny or ask covers the host's subdomains too.
            Some(host) => host == *pattern || host.ends_with(&format!(".{pattern}")),
            // A URL whose host Ovra cannot read may be the host a deny or
            // ask names, written another way: the rule holds.
            None => action != PolicyAction::Auto,
        };
    }
    if !matches!(kind, Some(ToolKind::Read | ToolKind::Edit)) || call.paths.is_empty() {
        return false;
    }
    let fits = |path: &String| match inside(work_dir, Path::new(path)) {
        Some(rel) => glob(pattern, &rel),
        None => false,
    };
    // `auto` allows a call only when it allows every file the call names;
    // a deny or ask holds when it covers any of them.
    if action == PolicyAction::Auto {
        call.paths.iter().all(fits)
    } else {
        call.paths.iter().any(fits)
    }
}

/// The host an `http` or `https` URL names, as the netproxy writes hosts;
/// `None` for anything else: a URL with user info, which readers may take
/// for the host, or a host with escapes, non-ASCII letters or dots other
/// than `.`, which a URL parser may turn into another host.
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
    let (command, prefix) = (words(command), words(prefix));
    !prefix.is_empty() && command.starts_with(&prefix)
}

fn words(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
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
