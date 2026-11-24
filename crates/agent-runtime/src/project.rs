//! What a project's own Claude configuration would load once the user trusts
//! the project: `--setting-sources project,local` reads `.claude/settings.json`
//! and `.claude/settings.local.json`, and project agents, commands and skills
//! come with them. `CLAUDE.md` loads either way, and `.mcp.json` never does
//! (Ovra passes `--strict-mcp-config`).

use std::path::Path;

use ovra_core::protocol::agents::{ConfigItem, ConfigKind, ProjectAgentConfig};
use serde_json::{Map, Value};

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
/// Entries listed per item; the rest are counted.
const MAX_ENTRIES: usize = 50;

pub(crate) fn inspect(root: &Path) -> ProjectAgentConfig {
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
    ProjectAgentConfig { items }
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
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path.is_dir() {
            markdown(&path, &format!("{prefix}{name}/"), names);
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
            "env" => push(items, path, ConfigKind::Env, keys(value)),
            "permissions" => {
                let Some(permissions) = value.as_object() else {
                    other.push(key.clone());
                    continue;
                };
                let allow = permissions.get("allow").map(strings).unwrap_or_default();
                push(items, path, ConfigKind::AllowRules, allow);
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
        assert_eq!(inspect(dir.path()), ProjectAgentConfig::default());
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
            "permissions": {"allow": ["Bash(npm test)"], "defaultMode": "acceptEdits"},
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
        insta::assert_json_snapshot!(inspect(dir.path()));
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
