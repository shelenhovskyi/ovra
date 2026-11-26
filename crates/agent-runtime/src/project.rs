//! What a project's own Claude configuration would load once the user trusts
//! the project: `--setting-sources project,local` reads `.claude/settings.json`
//! and `.claude/settings.local.json`, and project agents, commands and skills
//! come with them. `CLAUDE.md` loads either way, and `.mcp.json` never does
//! (Ovra passes `--strict-mcp-config`).

use std::path::Path;

use ovra_core::protocol::agents::{ConfigItem, ConfigKind, ProjectAgentConfig};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const SETTINGS: [&str; 2] = [".claude/settings.json", ".claude/settings.local.json"];
/// Settings that name a command Claude runs.
const HELPERS: [&str; 7] = [
    "apiKeyHelper",
    "awsAuthRefresh",
    "awsCredentialExport",
    "gcpAuthRefresh",
    "otelHeadersHelper",
    "statusLine",
    "fileSuggestion",
];
const PLUGINS: [&str; 2] = ["enabledPlugins", "extraKnownMarketplaces"];
/// Bigger settings files are not shown, only reported.
const MAX_SETTINGS_BYTES: u64 = 1 << 20;
/// Files bigger than this are fingerprinted by size and time only.
const MAX_HASHED_BYTES: u64 = 8 << 20;
const MAX_HASHED_FILES: usize = 10_000;
/// Folder depth walked under `.claude/skills` and the others.
const MAX_DEPTH: usize = 8;
/// Entries listed per item; the rest are counted.
const MAX_ENTRIES: usize = 50;

pub(crate) fn inspect(root: &Path) -> ProjectAgentConfig {
    ProjectAgentConfig {
        fingerprint: fingerprint(root),
        items: items(root),
    }
}

fn items(root: &Path) -> Vec<ConfigItem> {
    let mut items = Vec::new();
    for path in SETTINGS {
        let file = root.join(path);
        if file.symlink_metadata().is_ok() {
            settings(path, &file, &mut items);
        }
    }
    let claude = root.join(".claude");
    for (dir, kind) in [
        ("agents", ConfigKind::Agents),
        ("commands", ConfigKind::Commands),
    ] {
        let mut names = Vec::new();
        markdown(&claude.join(dir), "", &mut names);
        push(&mut items, &format!(".claude/{dir}"), kind, names);
    }
    let skills = std::fs::read_dir(claude.join("skills"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("SKILL.md").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    push(&mut items, ".claude/skills", ConfigKind::Skills, skills);
    items
}

/// SHA-256 over the path and bytes of every file that would load: the
/// settings files and everything under the agent, command and skill
/// folders (skills can carry scripts). Symlinked folders are not followed.
fn fingerprint(root: &Path) -> String {
    let mut files = Vec::new();
    for path in SETTINGS {
        files.push(path.to_owned());
    }
    for dir in ["agents", "commands", "skills"] {
        let prefix = format!(".claude/{dir}");
        walk(&root.join(&prefix), &prefix, 0, &mut files);
    }
    files.sort();
    files.truncate(MAX_HASHED_FILES);
    let mut hash = Sha256::new();
    hash.update(b"ovra-project-config-v1\0");
    for path in files {
        let file = root.join(&path);
        let Ok(meta) = file.metadata() else { continue };
        hash.update(path.as_bytes());
        hash.update(b"\0");
        match (meta.len() <= MAX_HASHED_BYTES)
            .then(|| std::fs::read(&file).ok())
            .flatten()
        {
            Some(bytes) => hash.update(&bytes),
            // Too big or unreadable: its size and time still change with it.
            None => hash.update(format!("{}:{:?}", meta.len(), meta.modified().ok()).as_bytes()),
        }
        hash.update(b"\0");
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Every file under `dir` as `prefix/...`, without following symlinked folders.
fn walk(dir: &Path, prefix: &str, depth: usize, files: &mut Vec<String>) {
    if depth > MAX_DEPTH || files.len() > MAX_HASHED_FILES {
        return;
    }
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk(&entry.path(), &name, depth + 1, files),
            Ok(_) => files.push(name),
            Err(_) => {}
        }
    }
}

fn push(items: &mut Vec<ConfigItem>, path: &str, kind: ConfigKind, mut entries: Vec<String>) {
    if entries.is_empty() {
        return;
    }
    entries.sort();
    if entries.len() > MAX_ENTRIES {
        let more = entries.len() - MAX_ENTRIES;
        entries.truncate(MAX_ENTRIES);
        entries.push(format!("… and {more} more"));
    }
    items.push(ConfigItem {
        path: path.to_owned(),
        kind,
        entries,
    });
}

/// `.md` files under `dir`, as `sub/name` without the extension.
fn markdown(dir: &Path, prefix: &str, names: &mut Vec<String>) {
    if prefix.matches('/').count() > MAX_DEPTH {
        return;
    }
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            markdown(&entry.path(), &format!("{prefix}{name}/"), names);
        } else if let Some(stem) = name.strip_suffix(".md") {
            names.push(format!("{prefix}{stem}"));
        }
    }
}

fn settings(path: &str, file: &Path, items: &mut Vec<ConfigItem>) {
    let unreadable = |items: &mut Vec<ConfigItem>, why: String| {
        push(items, path, ConfigKind::Unreadable, vec![why]);
    };
    match file.metadata() {
        Ok(meta) if meta.len() > MAX_SETTINGS_BYTES => {
            return unreadable(items, format!("larger than {MAX_SETTINGS_BYTES} bytes"));
        }
        Ok(_) => {}
        Err(e) => return unreadable(items, e.to_string()),
    }
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) => return unreadable(items, e.to_string()),
    };
    let object = match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(object)) => object,
        Ok(_) => return unreadable(items, "not a JSON object".to_owned()),
        Err(e) => return unreadable(items, e.to_string()),
    };
    let mut other = Vec::new();
    for (key, value) in &object {
        match key.as_str() {
            "$schema" => {}
            "hooks" => push(items, path, ConfigKind::Hooks, hooks(value)),
            "env" => push(items, path, ConfigKind::Env, env(value)),
            "permissions" => {
                let Some(permissions) = value.as_object() else {
                    other.push(key.clone());
                    continue;
                };
                let allow = permissions.get("allow").map(strings).unwrap_or_default();
                let (broad, narrow) = allow.into_iter().partition(|rule| broad(rule));
                push(items, path, ConfigKind::BroadAllowRules, broad);
                push(items, path, ConfigKind::AllowRules, narrow);
                other.extend(
                    permissions
                        .iter()
                        .filter(|(k, _)| *k != "allow")
                        .map(|(k, v)| format!("permissions.{k}: {}", short(v))),
                );
            }
            key if HELPERS.contains(&key) => {}
            key if PLUGINS.contains(&key) => {}
            _ => other.push(format!("{key}: {}", short(value))),
        }
    }
    let helpers = HELPERS
        .iter()
        .filter_map(|k| object.get(*k).map(|v| format!("{k}: {}", command(v))))
        .collect();
    push(items, path, ConfigKind::Helpers, helpers);
    let plugins = PLUGINS
        .iter()
        .filter_map(|k| object.get(*k))
        .flat_map(keys)
        .collect();
    push(items, path, ConfigKind::Plugins, plugins);
    push(items, path, ConfigKind::Settings, other);
}

/// Programs that run whatever they are given.
const INTERPRETERS: [&str; 22] = [
    "sh",
    "bash",
    "zsh",
    "fish",
    "pwsh",
    "powershell",
    "cmd",
    "python",
    "python3",
    "node",
    "npx",
    "bun",
    "deno",
    "eval",
    "env",
    "sudo",
    "perl",
    "ruby",
    "php",
    "xargs",
    "osascript",
    "doas",
];

/// Programs that reach the network or the rest of the machine whatever the
/// project is: they send data out, publish, or delete anywhere.
const REACH: [&str; 17] = [
    "curl", "wget", "ssh", "scp", "sftp", "rsync", "nc", "ncat", "netcat", "telnet", "ftp", "gh",
    "rm", "dd", "chmod", "chown", "open",
];

/// `program subcommand` pairs that publish or push.
const PUBLISH: [(&str, &str); 8] = [
    ("git", "push"),
    ("npm", "publish"),
    ("pnpm", "publish"),
    ("yarn", "publish"),
    ("cargo", "publish"),
    ("twine", "upload"),
    ("gem", "push"),
    ("docker", "push"),
];

/// Tools whose rule spec is a path.
const PATH_TOOLS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// Whether an allow rule lets a whole tool through, any command, or reaches
/// past the project: a bare tool name (`Bash`, `Edit`, `mcp__server`), a
/// spec of only wildcards (`Bash(*)`, `Write(**)`, `Edit(//**)`), a shell
/// command that starts an interpreter (`Bash(python:*)`, `Bash(sh -c *)`),
/// reaches the network, publishes or deletes (`Bash(curl:*)`,
/// `Bash(git push:*)`, `Bash(rm:*)`), or an edit outside the project
/// (`Edit(~/.bashrc)`, `Write(//etc/hosts)`, `Edit(../x)`). Read-only tools
/// are left out: allowing them asks nothing that changes the machine.
fn broad(rule: &str) -> bool {
    let (tool, spec) = match rule.split_once('(') {
        Some((tool, rest)) => (tool.trim(), rest.strip_suffix(')').map(str::trim)),
        None => (rule.trim(), None),
    };
    if matches!(tool, "Read" | "Glob" | "Grep" | "LS" | "TodoWrite") {
        return false;
    }
    let Some(spec) = spec else { return true };
    if spec
        .chars()
        .all(|c| matches!(c, '*' | '/' | '.' | '~' | ':' | ' '))
    {
        return true;
    }
    if PATH_TOOLS.contains(&tool) {
        return outside(spec);
    }
    if tool != "Bash" && tool != "PowerShell" {
        return false;
    }
    let mut words = spec
        .split(|c: char| c == ':' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_end_matches('*'));
    let program = words.next().unwrap_or_default();
    // `/usr/bin/curl` is curl.
    let program = program.rsplit(['/', '\\']).next().unwrap_or_default();
    let next = words.next().unwrap_or_default();
    INTERPRETERS.contains(&program)
        || REACH.contains(&program)
        || PUBLISH.contains(&(program, next))
}

/// Whether a path rule names something outside the project: absolute
/// (`//etc`, `C:`), in the home folder (`~`), or up from it (`..`). In
/// Claude's rules `/x` is relative to the settings file, so it stays inside.
fn outside(spec: &str) -> bool {
    let spec = spec.trim();
    spec.starts_with("//")
        || spec.starts_with('~')
        || spec.starts_with('\\')
        || spec.as_bytes().get(1) == Some(&b':')
        || spec.split(['/', '\\']).any(|part| part == "..")
}

/// Names that look like they hold a secret: their values are not shown.
const SECRET: [&str; 6] = ["KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "AUTH"];

/// `NAME = value` for every variable, so a changed value shows as a change.
/// Secret-looking values are masked down to a short digest that still
/// changes with them.
fn env(value: &Value) -> Vec<String> {
    let Some(vars) = value.as_object() else {
        return vec![short(value)];
    };
    vars.iter()
        .map(|(name, value)| {
            let text = value.as_str().map_or_else(|| short(value), str::to_owned);
            let upper = name.to_uppercase();
            if SECRET.iter().any(|s| upper.contains(s)) {
                let digest: String = Sha256::digest(text.as_bytes())
                    .iter()
                    .take(3)
                    .map(|b| format!("{b:02x}"))
                    .collect();
                format!("{name} = (hidden, #{digest})")
            } else {
                format!("{name} = {}", clip(&text))
            }
        })
        .collect()
}

/// Text on one line, cut to 120 characters.
fn clip(text: &str) -> String {
    let text = text.replace(['\n', '\r'], " ");
    match text.char_indices().nth(120) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text,
    }
}

/// `Event (matcher): command` for every hook.
fn hooks(value: &Value) -> Vec<String> {
    let Some(events) = value.as_object() else {
        return vec![short(value)];
    };
    let mut entries = Vec::new();
    for (event, matchers) in events {
        for matcher in matchers.as_array().into_iter().flatten() {
            let on = match matcher.get("matcher").and_then(Value::as_str) {
                Some(m) if !m.is_empty() => format!("{event} ({m})"),
                _ => event.clone(),
            };
            let hooks = matcher.get("hooks").and_then(Value::as_array);
            for hook in hooks.into_iter().flatten() {
                entries.push(format!("{on}: {}", command(hook)));
            }
        }
    }
    entries
}

/// What a hook or helper runs: its command, URL or prompt.
fn command(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_owned();
    }
    ["command", "url", "prompt"]
        .iter()
        .find_map(|k| value.get(*k).and_then(Value::as_str))
        .map_or_else(|| short(value), str::to_owned)
}

fn keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(Map::keys)
        .into_iter()
        .flatten()
        .cloned()
        .collect()
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| v.as_str().map_or_else(|| short(v), str::to_owned))
        .collect()
}

/// A value on one line, cut to 120 characters.
fn short(value: &Value) -> String {
    let text = value.to_string();
    match text.char_indices().nth(120) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        for (path, text) in files {
            let file = dir.path().join(path);
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent).unwrap_or_else(|e| panic!("{e}"));
            }
            std::fs::write(file, text).unwrap_or_else(|e| panic!("{e}"));
        }
        dir
    }

    #[test]
    fn a_project_without_claude_configuration_loads_nothing_extra() {
        let dir = project(&[("CLAUDE.md", "rules"), (".mcp.json", "{}")]);
        assert_eq!(inspect(dir.path()).items, []);
    }

    #[test]
    fn the_fingerprint_follows_contents_the_items_do_not_show() {
        let dir = project(&[
            (".claude/settings.json", r#"{"env": {"A": "1"}}"#),
            (".claude/skills/release/SKILL.md", ""),
            (".claude/skills/release/run.sh", "echo one"),
        ]);
        let first = inspect(dir.path());
        assert_eq!(first.fingerprint.len(), 64);
        assert_eq!(inspect(dir.path()), first);
        std::fs::write(dir.path().join(".claude/skills/release/run.sh"), "echo two")
            .unwrap_or_else(|e| panic!("{e}"));
        let script = inspect(dir.path());
        assert_eq!(script.items, first.items);
        assert_ne!(script.fingerprint, first.fingerprint);
        std::fs::write(
            dir.path().join(".claude/settings.json"),
            r#"{"env": {"A": "2"}}"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_ne!(inspect(dir.path()).fingerprint, script.fingerprint);
        // A changed value is a changed entry, so the review shows it.
        assert_eq!(inspect(dir.path()).items[0].entries, ["A = 2"]);
        // CLAUDE.md loads either way, so it is not part of what is trusted.
        std::fs::write(dir.path().join("CLAUDE.md"), "new rules").unwrap_or_else(|e| panic!("{e}"));
        let unchanged = inspect(dir.path()).fingerprint;
        std::fs::remove_file(dir.path().join("CLAUDE.md")).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(inspect(dir.path()).fingerprint, unchanged);
    }

    #[test]
    fn everything_that_would_load_is_listed() {
        let settings = r#"{
            "$schema": "https://json.schemastore.org/claude-code-settings.json",
            "hooks": {
                "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "./check.sh"}]}],
                "Stop": [{"hooks": [{"type": "http", "url": "https://example.com/done"}]}]
            },
            "env": {"ANTHROPIC_BASE_URL": "https://proxy.example"},
            "permissions": {"allow": ["Bash(npm test)", "Bash(*)"], "defaultMode": "acceptEdits"},
            "apiKeyHelper": "./key.sh",
            "statusLine": {"type": "command", "command": "./status.sh"},
            "enabledPlugins": {"tools@market": true},
            "model": "opus"
        }"#;
        let dir = project(&[
            (".claude/settings.json", settings),
            (".claude/settings.local.json", "{not json"),
            (".claude/agents/reviewer.md", ""),
            (".claude/commands/git/ship.md", ""),
            (".claude/commands/notes.txt", ""),
            (".claude/skills/release/SKILL.md", ""),
            (".claude/skills/empty/README.md", ""),
        ]);
        insta::assert_json_snapshot!(inspect(dir.path()).items);
    }

    #[test]
    fn broad_allow_rules_are_told_apart() {
        for rule in [
            "Bash",
            "Bash(*)",
            "Bash(:*)",
            "Edit",
            "Write(**)",
            "Edit(//**)",
            "Bash(python:*)",
            "Bash(sh -c *)",
            "Bash(sudo *)",
            "mcp__github",
            "WebFetch",
            "Bash(curl:*)",
            "Bash(/usr/bin/curl https://x)",
            "Bash(git push:*)",
            "Bash(npm publish)",
            "Bash(rm -rf *)",
            "Bash(perl -e *)",
            "Edit(~/.bashrc)",
            "Write(//etc/hosts)",
            "Edit(../other/**)",
            "Write(C:\\Windows\\x)",
        ] {
            assert!(broad(rule), "{rule}");
        }
        for rule in [
            "Bash(npm test)",
            "Bash(git status:*)",
            "Bash(git pushd)",
            "Bash(npm run publish-docs)",
            "Edit(src/**)",
            "Edit(/src/**)",
            "Write(./docs/a..b.md)",
            "WebFetch(domain:example.com)",
            "Read",
            "Read(**)",
            "Grep",
        ] {
            assert!(!broad(rule), "{rule}");
        }
    }

    #[test]
    fn env_values_are_shown_and_secrets_masked_by_a_digest() {
        let entries = |text: &str| {
            let dir = project(&[(".claude/settings.json", text)]);
            inspect(dir.path()).items[0].entries.clone()
        };
        let first = entries(r#"{"env": {"ANTHROPIC_BASE_URL": "https://a", "API_TOKEN": "one"}}"#);
        assert_eq!(first[0], "ANTHROPIC_BASE_URL = https://a");
        assert!(
            first[1].starts_with("API_TOKEN = (hidden, #"),
            "{}",
            first[1]
        );
        assert!(!first[1].contains("one"));
        let second = entries(r#"{"env": {"ANTHROPIC_BASE_URL": "https://a", "API_TOKEN": "two"}}"#);
        assert_ne!(first[1], second[1]);
    }

    #[test]
    fn long_lists_are_cut() {
        let rules: Vec<_> = (0..60).map(|n| format!("\"Read(f{n:02})\"")).collect();
        let settings = format!(r#"{{"permissions": {{"allow": [{}]}}}}"#, rules.join(","));
        let dir = project(&[(".claude/settings.json", &settings)]);
        let config = inspect(dir.path());
        let entries = &config.items[0].entries;
        assert_eq!(entries.len(), MAX_ENTRIES + 1);
        assert_eq!(entries.last().map(String::as_str), Some("… and 10 more"));
    }
}
