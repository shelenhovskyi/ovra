use std::path::Path;

use serde_json::{Value, json};

use crate::protocol::agents::{SandboxNetwork, SandboxReport, SessionMode};
use crate::protocol::policy::{Policy, PolicyAction, PolicyRule};

const DIR: &str = "/w";

fn linux() -> SandboxReport {
    SandboxReport {
        name: "bubblewrap, Landlock, seccomp".into(),
        network: SandboxNetwork::Proxy,
        gaps: vec!["submodules added during a session".into()],
    }
}

fn quiet(rules: Vec<PolicyRule>) -> Policy {
    Policy { rules, quiet: true }
}

fn allows_under(policy: &Policy, tool: &str, input: Value, mode: SessionMode) -> bool {
    let call = crate::fakes::tool(tool, input).call;
    super::allows(policy, tool, &call, Path::new(DIR), Some(&linux()), mode)
}

fn allows_in(tool: &str, input: Value, mode: SessionMode, sandbox: Option<&SandboxReport>) -> bool {
    let call = crate::fakes::tool(tool, input).call;
    super::allows(
        &quiet(Vec::new()),
        tool,
        &call,
        Path::new(DIR),
        sandbox,
        mode,
    )
}

fn allows(tool: &str, input: Value) -> bool {
    allows_in(tool, input, SessionMode::Isolated, Some(&linux()))
}

fn bash(command: &str) -> bool {
    allows("Bash", json!({ "command": command }))
}

#[test]
fn only_a_trusted_sandbox_with_a_limited_network_is_quiet() {
    let read = || json!({"file_path": "src/a.rs"});
    assert!(allows("Read", read()));
    let blocked = SandboxReport {
        network: SandboxNetwork::Blocked,
        ..linux()
    };
    assert!(allows_in(
        "Read",
        read(),
        SessionMode::Isolated,
        Some(&blocked)
    ));
    let open = SandboxReport {
        network: SandboxNetwork::Open,
        ..linux()
    };
    let seatbelt = SandboxReport {
        name: "Seatbelt".into(),
        ..linux()
    };
    let windows = SandboxReport {
        name: "Windows sandbox user".into(),
        ..linux()
    };
    for sandbox in [None, Some(&open), Some(&seatbelt), Some(&windows)] {
        assert!(
            !allows_in("Read", read(), SessionMode::Isolated, sandbox),
            "{sandbox:?}"
        );
    }
}

#[test]
fn reads_and_searches_stay_inside_the_folder_and_off_protected_paths() {
    assert!(allows("Read", json!({"file_path": "/w/src/a.rs"})));
    assert!(allows("Grep", json!({"pattern": "x"})));
    assert!(allows("Grep", json!({"pattern": "x", "path": "/w"})));
    assert!(allows("Glob", json!({"pattern": "*.rs", "path": "src"})));
    assert!(allows("WebSearch", json!({"query": "rust"})));
    for (tool, input) in [
        ("Read", json!({"file_path": "/etc/passwd"})),
        ("Read", json!({"file_path": "../other/a"})),
        ("Read", json!({"file_path": ".git/config"})),
        ("Read", json!({"file_path": ".claude/settings.json"})),
        ("Grep", json!({"pattern": "x", "path": "/home"})),
        ("LS", json!({"path": ".."})),
    ] {
        assert!(!allows(tool, input.clone()), "{tool} {input}");
    }
}

#[test]
fn edits_stay_inside_the_folder_off_protected_and_git_paths() {
    let edit = |path: &str| json!({"file_path": path, "old_string": "a", "new_string": "b"});
    assert!(allows("Edit", edit("src/a.rs")));
    assert!(allows(
        "Write",
        json!({"file_path": "/w/notes.md", "content": ""})
    ));
    // In the person's checkout their own tools may run an edit before any
    // review; a research session changes nothing.
    for mode in [SessionMode::Direct, SessionMode::Research] {
        assert!(
            !allows_in("Edit", edit("src/a.rs"), mode, Some(&linux())),
            "{mode:?}"
        );
    }
    for path in [
        "/etc/hosts",
        "../a",
        ".git/hooks/pre-commit",
        "sub/.GIT./config",
        "GIT~1/config",
        "PACKAG~1.JSO",
        "CLAUDE.md",
        ".claude/settings.json",
        ".github/workflows/ci.yml",
    ] {
        assert!(!allows("Edit", edit(path)), "{path}");
    }
    assert!(!allows("Write", json!({"content": "x"})));
}

#[test]
fn commands_run_quietly_only_in_a_worktree() {
    assert!(bash("cargo test -p ovra-core 2>&1 | tail -n 20"));
    assert!(bash("npm test && npm run lint"));
    assert!(bash("rm -rf target"));
    assert!(bash("ls /usr/include > /dev/null"));
    for mode in [SessionMode::Direct, SessionMode::Research] {
        let bash = json!({"command": "cargo test"});
        assert!(!allows_in("Bash", bash, mode, Some(&linux())), "{mode:?}");
        let rm = json!({"file_path": "a"});
        assert!(!allows_in("Delete", rm, mode, Some(&linux())), "{mode:?}");
    }
    // Reading stays quiet in the person's checkout.
    let read = json!({"file_path": "src/a.rs"});
    assert!(allows_in("Read", read, SessionMode::Direct, Some(&linux())));
}

#[test]
fn nothing_is_quiet_with_the_switch_off() {
    let off = Policy::default();
    assert!(!allows_under(
        &off,
        "Read",
        json!({"file_path": "a"}),
        SessionMode::Isolated
    ));
}

#[test]
fn a_read_without_a_path_asks() {
    assert!(!allows("Read", json!({})));
    assert!(!allows("Read", json!({"file_path": 3})));
}

#[test]
fn deny_and_ask_rules_on_files_hold_for_every_tool_that_names_them() {
    let rule = |tool: &str, pattern: Option<&str>, action| PolicyRule {
        tool: tool.into(),
        pattern: pattern.map(str::to_owned),
        action,
        exact: false,
        from_approval: false,
    };
    let policy = quiet(vec![
        rule("Read", Some(".env"), PolicyAction::Deny),
        rule("Edit", Some("keys/**"), PolicyAction::Ask),
        rule("Read", Some("src/**"), PolicyAction::Auto),
    ]);
    let quietly =
        |tool: &str, input: Value| allows_under(&policy, tool, input, SessionMode::Isolated);
    for (tool, input) in [
        ("Grep", json!({"pattern": ".", "path": ".env"})),
        ("Read", json!({"file_path": "/w/.env"})),
        ("Bash", json!({"command": "cat .env"})),
        ("Bash", json!({"command": "cp a --to='keys/b'"})),
        ("Write", json!({"file_path": "keys/a", "content": ""})),
    ] {
        assert!(!quietly(tool, input.clone()), "{tool} {input}");
    }
    assert!(quietly("Bash", json!({"command": "cat src/a.rs"})));
    // A file rule without a match covers every call that names a path.
    let all = quiet(vec![rule("Read", None, PolicyAction::Ask)]);
    assert!(!allows_under(
        &all,
        "Bash",
        json!({"command": "ls"}),
        SessionMode::Isolated
    ));
}

#[test]
fn the_home_folder_and_agent_logins_ask() {
    for (tool, input) in [
        (
            "Read",
            json!({"file_path": "~/.claude-work/.credentials.json"}),
        ),
        ("Read", json!({"file_path": "$HOME/a"})),
        (
            "Grep",
            json!({"pattern": "token", "path": "~/.claude-work"}),
        ),
        ("Glob", json!({"pattern": "/srv/other/**/.env"})),
        ("Glob", json!({"pattern": "../other/*"})),
        ("Grep", json!({"pattern": "x", "glob": "/etc/*"})),
        (
            "Bash",
            json!({"command": "cat ~/.claude-work/.credentials.json"}),
        ),
        ("Bash", json!({"command": "cat $HOME/x"})),
        ("Bash", json!({"command": "cat /root/.cl*/.cr*"})),
        ("Bash", json!({"command": "ls /home/me/.codex"})),
    ] {
        assert!(!allows(tool, input.clone()), "{tool} {input}");
    }
    assert!(allows("Glob", json!({"pattern": "src/**/*.rs"})));
}

#[test]
fn commands_that_hide_what_runs_ask() {
    for command in ["echo `id`", "echo $(id)", "diff <(ls) a", "tee >(cat)"] {
        assert!(!bash(command), "{command}");
    }
}

#[test]
fn git_runs_quietly_only_when_it_reads() {
    for command in [
        "git status",
        "git --no-pager log --oneline -5",
        "git -P diff HEAD~1",
        "git diff | head; git show HEAD:README.md",
        "/usr/bin/git blame src/a.rs",
    ] {
        assert!(bash(command), "{command}");
    }
    for command in [
        "git push",
        "git reset --hard",
        "git clean -fdx",
        "git checkout -- .",
        "git stash drop",
        "git commit --amend",
        "git -c alias.st=push st",
        "git -C .. status",
        "git",
        "ls; git push",
        "ls&&git push origin main",
        "\"git\" push",
        "g\\it push",
        "/usr/bin/git.exe push",
        "xargs git push",
        "sh -c 'git push'",
        "env git push",
    ] {
        assert!(!bash(command), "{command}");
    }
}

#[test]
fn commands_naming_protected_paths_ask() {
    for command in [
        "cat .git/config",
        "echo x >> CLAUDE.md",
        "echo x>CLAUDE.md",
        "cp a /w/.claude/settings.json",
        "sed -i s/a/b/ \".github/workflows/ci.yml\"",
        "cp a --target=.husky/pre-commit",
        "cat ./.GIT/HEAD",
    ] {
        assert!(!bash(command), "{command}");
    }
}

#[test]
fn fetches_go_only_to_named_hosts_nobody_else_receives_data_at() {
    let fetch = |url: &str| allows("WebFetch", json!({"url": url, "prompt": "x"}));
    assert!(fetch("https://docs.rs/serde"));
    for url in [
        "https://1.2.3.4/a",
        "http://localhost:3000",
        "https://gist.github.com/x",
        "https://raw.githubusercontent.com/a/b",
        "https://user@docs.rs/",
        "ftp://docs.rs/a",
        "docs.rs",
    ] {
        assert!(!fetch(url), "{url}");
    }
}

#[test]
fn other_tools_ask_and_helpers_do_not() {
    assert!(allows("TodoWrite", json!({"todos": []})));
    assert!(allows("Task", json!({"prompt": "x"})));
    assert!(!allows("mcp__github__create_issue", json!({})));
    assert!(!allows("SomethingNew", json!({})));
}
