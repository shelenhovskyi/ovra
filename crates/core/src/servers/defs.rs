//! `.ovra/servers.json`: the servers a work dir defines, checked as the
//! spec says, and the command line each start runs.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::path::RelPath;
use crate::protocol::ErrorCode;
use crate::protocol::fs::FileBody;
use crate::protocol::servers::ServerDef;
use crate::services::{CommandLine, FsService};

/// Where a work dir defines its servers; `.ovra` is protected, so only the
/// person changes it.
pub(crate) const FILE: &str = ".ovra/servers.json";

/// The port the sandbox's helper bridges to the netproxy on, inside every
/// sandbox (`Sandbox::wrap` refuses it), so no server may listen there.
const PROXY_BRIDGE_PORT: u16 = 3128;

/// More servers than this in one file is a mistake, not a project.
const MAX_SERVERS: usize = 32;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    servers: Vec<ServerDef>,
}

/// The definitions of the work dir `dir`, or why they cannot be used. A
/// work dir without the file defines none.
pub(crate) async fn read(fs: &dyn FsService, dir: &Path) -> Result<Vec<ServerDef>, String> {
    let path = RelPath::parse(FILE).map_err(|e| e.message)?;
    match fs.read_file(dir.to_path_buf(), path).await {
        Ok(file) => match file.body {
            FileBody::Text { text } => parse(&text),
            _ => Err(format!("{FILE} is not a text file")),
        },
        Err(e) if e.code == ErrorCode::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{FILE} could not be read: {}", e.message)),
    }
}

/// Parses and checks the file's text.
pub(crate) fn parse(text: &str) -> Result<Vec<ServerDef>, String> {
    let file: File = serde_json::from_str(text).map_err(|e| format!("{FILE}: {e}"))?;
    if file.servers.len() > MAX_SERVERS {
        return Err(format!("{FILE}: more than {MAX_SERVERS} servers"));
    }
    let mut names = BTreeSet::new();
    let mut ports = BTreeSet::new();
    for (i, def) in file.servers.iter().enumerate() {
        let at = |why: String| format!("{FILE}: servers[{i}]: {why}");
        check(def).map_err(at)?;
        if !names.insert(def.name.as_str()) {
            return Err(at(format!("another server is named \"{}\"", def.name)));
        }
        if !ports.insert(def.port) {
            return Err(at(format!("another server uses port {}", def.port)));
        }
    }
    Ok(file.servers)
}

fn check(def: &ServerDef) -> Result<(), String> {
    if !valid_name(&def.name) {
        return Err("the name must match ^[a-z0-9][a-z0-9-]{0,31}$".to_owned());
    }
    let has_nul = |s: &str| s.contains('\0');
    match def.command.first() {
        None => return Err("the command is empty".to_owned()),
        Some(program) if program.is_empty() => return Err("the program is empty".to_owned()),
        Some(_) if def.command.iter().any(|a| has_nul(a)) => {
            return Err("the command contains a NUL byte".to_owned());
        }
        Some(_) => {}
    }
    if let Some(cwd) = &def.cwd {
        RelPath::parse(cwd).map_err(|e| format!("cwd: {}", e.message))?;
    }
    if def.port < 1024 {
        return Err("the port must be 1024 to 65535".to_owned());
    }
    if def.port == PROXY_BRIDGE_PORT {
        return Err(format!(
            "port {PROXY_BRIDGE_PORT} is where the sandbox reaches its netproxy"
        ));
    }
    for (name, value) in &def.env {
        if name.is_empty() || name.contains('=') || has_nul(name) || has_nul(value) {
            return Err(format!("env {name:?} is not a valid variable"));
        }
    }
    if let Some(ready) = &def.ready {
        if ready.log.as_deref().is_some_and(str::is_empty) {
            return Err("ready.log is empty".to_owned());
        }
        if ready.http.as_deref().is_some_and(|p| !p.starts_with('/')) {
            return Err("ready.http must be a path starting with /".to_owned());
        }
    }
    Ok(())
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let lower_or_digit = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    chars.next().is_some_and(lower_or_digit)
        && name.len() <= 32
        && chars.all(|c| lower_or_digit(c) || c == '-')
}

/// What one start of `def` in the work dir `dir` runs: the program
/// without a shell, `PORT` and `HOST` set, then the definition's own
/// variables.
pub(crate) fn command_line(def: &ServerDef, dir: &Path) -> CommandLine {
    let cwd = def
        .cwd
        .as_deref()
        .and_then(|cwd| RelPath::parse(cwd).ok())
        .map_or_else(|| dir.to_path_buf(), |cwd| cwd.under(dir));
    let var = |k: &str, v: &str| (OsString::from(k), Some(OsString::from(v)));
    let mut env = vec![var("PORT", &def.port.to_string()), var("HOST", "127.0.0.1")];
    env.extend(def.env.iter().map(|(k, v)| var(k, v)));
    CommandLine {
        program: PathBuf::from(&def.command[0]),
        args: def.command[1..].iter().map(OsString::from).collect(),
        env,
        cwd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_files_say_what_is_wrong() {
        let web = r#"{"name": "web", "command": ["pnpm", "dev"], "port": 5173}"#;
        let files = [
            "not json".to_owned(),
            r#"{"servers": [], "extra": 1}"#.to_owned(),
            r#"{"servers": [{"name": "Web", "command": ["x"], "port": 5173}]}"#.to_owned(),
            format!(r#"{{"servers": [{{"name": "{}", "command": ["x"], "port": 5173}}]}}"#, "a".repeat(33)),
            r#"{"servers": [{"name": "-web", "command": ["x"], "port": 5173}]}"#.to_owned(),
            r#"{"servers": [{"name": "web", "command": [], "port": 5173}]}"#.to_owned(),
            r#"{"servers": [{"name": "web", "command": [""], "port": 5173}]}"#.to_owned(),
            r#"{"servers": [{"name": "web", "command": ["x"], "port": 80}]}"#.to_owned(),
            r#"{"servers": [{"name": "web", "command": ["x"], "port": 70000}]}"#.to_owned(),
            r#"{"servers": [{"name": "web", "command": ["x"], "port": 3128}]}"#.to_owned(),
            r#"{"servers": [{"name": "web", "command": ["x"], "port": 5173, "cwd": "../up"}]}"#.to_owned(),
            r#"{"servers": [{"name": "web", "command": ["x"], "port": 5173, "env": {"A=B": "1"}}]}"#.to_owned(),
            r#"{"servers": [{"name": "web", "command": ["x"], "port": 5173, "ready": {"http": "health"}}]}"#.to_owned(),
            format!(r#"{{"servers": [{web}, {web}]}}"#),
            format!(
                r#"{{"servers": [{web}, {{"name": "api", "command": ["x"], "port": 5173}}]}}"#
            ),
        ];
        let errors: Vec<String> = files
            .iter()
            .map(|text| {
                parse(text)
                    .err()
                    .unwrap_or_else(|| format!("accepted: {text}"))
            })
            .collect();
        insta::assert_json_snapshot!(errors);
    }

    #[test]
    fn a_definition_becomes_a_command_line_in_its_work_dir() {
        let text = r#"{"servers": [
            {"name": "web", "command": ["pnpm", "dev", "--strict"], "cwd": "./ui/", "port": 5173,
             "env": {"VITE_API": "http://localhost:8080"}, "ready": {"log": "ready in"}},
            {"name": "api", "command": ["cargo", "run"], "port": 8080}
        ]}"#;
        let defs = parse(text).unwrap_or_else(|e| panic!("{e}"));
        let dir = Path::new(crate::test_path!("/work/a"));
        let web = command_line(&defs[0], dir);
        assert_eq!(web.program, PathBuf::from("pnpm"));
        assert_eq!(web.args, ["dev", "--strict"]);
        assert_eq!(web.cwd, dir.join("ui"));
        let env: Vec<String> = web
            .env
            .iter()
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    k.to_string_lossy(),
                    v.clone().unwrap_or_default().to_string_lossy()
                )
            })
            .collect();
        assert_eq!(
            env,
            [
                "PORT=5173",
                "HOST=127.0.0.1",
                "VITE_API=http://localhost:8080"
            ]
        );
        assert_eq!(command_line(&defs[1], dir).cwd, dir);
    }
}
