use std::path::Path;

use serde_json::{Value, json};

use crate::protocol::agents::{SandboxNetwork, SandboxReport, SessionMode};

const DIR: &str = "/w";

fn linux() -> SandboxReport {
    SandboxReport {
        name: "bubblewrap, Landlock, seccomp".into(),
        network: SandboxNetwork::Proxy,
        gaps: vec!["submodules added during a session".into()],
    }
}

fn allows_in(tool: &str, input: Value, mode: SessionMode, sandbox: Option<&SandboxReport>) -> bool {
    let call = crate::fakes::tool(tool, input).call;
    super::allows(tool, &call, Path::new(DIR), sandbox, mode)
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
    // Discard restores a direct session's own edits.
    assert!(allows_in(
        "Edit",
        edit("src/a.rs"),
        SessionMode::Direct,
        Some(&linux())
    ));
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
    assert!(!allows_in(
        "Bash",
        json!({"command": "cargo test"}),
        SessionMode::Direct,
        Some(&linux())
    ));
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
