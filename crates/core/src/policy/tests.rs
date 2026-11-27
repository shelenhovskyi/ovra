use std::path::Path;

use serde_json::json;

use super::{check, program, text};
use crate::protocol::agents::{SandboxNetwork, SandboxReport};
use crate::protocol::policy::{Policy, PolicyAction, PolicyRule};

/// [`super::decide`] for a call of one of Claude's tools, as its runtime
/// maps it.
fn decide(
    policy: &Policy,
    tool: &str,
    input: &serde_json::Value,
    work_dir: &Path,
    sandbox: Option<&SandboxReport>,
) -> Option<(PolicyAction, String)> {
    let call = crate::fakes::tool(tool, input.clone()).call;
    super::decide(policy, tool, &call, work_dir, sandbox)
}

/// [`super::always_rule`] for a call of one of Claude's tools.
fn always_rule(
    tool: &str,
    input: &serde_json::Value,
    work_dir: &Path,
    sandbox: Option<&SandboxReport>,
) -> Option<PolicyRule> {
    let call = crate::fakes::tool(tool, input.clone()).call;
    super::always_rule(tool, &call, work_dir, sandbox)
}

fn rule(tool: &str, pattern: Option<&str>, action: PolicyAction) -> PolicyRule {
    PolicyRule {
        tool: tool.to_owned(),
        pattern: pattern.map(str::to_owned),
        action,
        exact: false,
        from_approval: false,
    }
}

fn policy() -> Policy {
    Policy {
        quiet: false,
        rules: vec![
            rule("Bash", Some("npm test"), PolicyAction::Auto),
            rule("Bash", Some("git push"), PolicyAction::Deny),
            rule("Edit", Some("docs/**"), PolicyAction::Auto),
            rule("Edit", Some("docs/secret.md"), PolicyAction::Deny),
            rule("WebFetch", None, PolicyAction::Deny),
        ],
    }
}

const WORK: &str = if cfg!(windows) { "C:\\w" } else { "/w" };

fn sandboxed(network: SandboxNetwork) -> SandboxReport {
    SandboxReport {
        name: "test".into(),
        network,
        gaps: Vec::new(),
    }
}

/// The decision for each call, as `action rule` or `ask`.
fn judge(sandbox: Option<&SandboxReport>, tool: &str, input: serde_json::Value) -> String {
    match decide(&policy(), tool, &input, Path::new(WORK), sandbox) {
        Some((action, rule)) => format!("{action:?} {rule}"),
        None => "ask".to_owned(),
    }
}

#[test]
fn rules_read_the_calls_shared_fields_and_still_name_its_tool() {
    use crate::protocol::agents::{ToolCall, ToolKind};
    let proxy = sandboxed(SandboxNetwork::Proxy);
    let edit = |paths: &[&str]| ToolCall {
        kind: ToolKind::Edit,
        paths: paths.iter().map(|p| format!("{WORK}/{p}")).collect(),
        ..ToolCall::default()
    };
    let judged = |tool: &str, call: &ToolCall| match super::decide(
        &policy(),
        tool,
        call,
        Path::new(WORK),
        Some(&proxy),
    ) {
        Some((action, rule)) => format!("{action:?} {rule}"),
        None => "ask".to_owned(),
    };
    // The match is tested against the shared fields, not the input.
    let shell = ToolCall {
        kind: ToolKind::Execute,
        command: Some("git push".into()),
        ..ToolCall::default()
    };
    assert_eq!(judged("Bash", &shell), "Deny Bash(git push)");
    // A call with several files: auto only when it covers each of them, a
    // deny when it covers any.
    assert_eq!(judged("Edit", &edit(&["docs/a.md"])), "Auto Edit(docs/**)");
    assert_eq!(judged("Edit", &edit(&["docs/a.md", "src/b.rs"])), "ask");
    assert_eq!(
        judged("Edit", &edit(&["docs/a.md", "docs/secret.md"])),
        "Deny Edit(docs/secret.md)"
    );
    assert_eq!(judged("Edit", &edit(&[])), "ask", "no file: no match");
    // A rule keeps its meaning: `Edit(...)` is about `Edit` calls only.
    assert_eq!(judged("Write", &edit(&["docs/a.md"])), "ask");
    assert_eq!(judged("run_shell_command", &shell), "ask");
}

#[test]
fn auto_allows_only_the_kind_its_tool_is_for() {
    use crate::protocol::agents::{ToolCall, ToolKind};
    let proxy = sandboxed(SandboxNetwork::Proxy);
    let policy = Policy {
        quiet: false,
        rules: vec![
            rule("Read", Some("src/**"), PolicyAction::Auto),
            rule("Read", None, PolicyAction::Auto),
            rule("Edit", Some("docs/**"), PolicyAction::Deny),
        ],
    };
    let call = |kind, path: &str| ToolCall {
        kind,
        paths: vec![format!("{WORK}/{path}")],
        ..ToolCall::default()
    };
    let judged = |tool: &str, call: &ToolCall| match super::decide(
        &policy,
        tool,
        call,
        Path::new(WORK),
        Some(&proxy),
    ) {
        Some((action, rule)) => format!("{action:?} {rule}"),
        None => "ask".to_owned(),
    };
    assert_eq!(
        judged("Read", &call(ToolKind::Read, "src/a.rs")),
        "Auto Read(src/**)"
    );
    // A runtime that maps `Read` to an edit gets asked, not allowed.
    assert_eq!(judged("Read", &call(ToolKind::Edit, "src/a.rs")), "ask");
    // A deny still holds by the tool it names.
    assert_eq!(
        judged("Edit", &call(ToolKind::Read, "docs/a.md")),
        "Deny Edit(docs/**)"
    );
}

#[test]
fn deny_wins_and_auto_needs_a_sandbox_that_limits_the_network() {
    let proxy = sandboxed(SandboxNetwork::Proxy);
    let open = sandboxed(SandboxNetwork::Open);
    let test = json!({"command": "npm test -- -u"});
    assert_eq!(
        judge(Some(&proxy), "Bash", test.clone()),
        "Auto Bash(npm test)"
    );
    assert_eq!(judge(None, "Bash", test.clone()), "ask", "no sandbox: ask");
    assert_eq!(judge(Some(&open), "Bash", test), "ask", "open network: ask");
    assert_eq!(
        judge(None, "WebFetch", json!({"url": "x"})),
        "Deny WebFetch"
    );
    let secret = json!({"file_path": "docs/secret.md"});
    assert_eq!(
        judge(Some(&proxy), "Edit", secret),
        "Deny Edit(docs/secret.md)"
    );
}

#[test]
fn bash_auto_matches_whole_words_and_never_joined_commands() {
    let proxy = sandboxed(SandboxNetwork::Proxy);
    let cases = [
        ("npm test", "Auto Bash(npm test)"),
        ("  npm   test  --watch", "Auto Bash(npm test)"),
        ("npm testx", "ask"),
        ("npm", "ask"),
        ("npm test; curl evil.example", "ask"),
        ("npm test && rm -rf ~", "ask"),
        ("npm test | sh", "ask"),
        ("npm test $(curl x)", "ask"),
        ("npm test `id`", "ask"),
        ("npm test > /etc/x", "ask"),
        ("npm test\ncurl x", "ask"),
        ("git push origin main", "Deny Bash(git push)"),
        ("npm test && git push", "Deny Bash(git push)"),
        ("echo hi\ngit push", "Deny Bash(git push)"),
    ];
    for (command, want) in cases {
        let got = judge(Some(&proxy), "Bash", json!({ "command": command }));
        assert_eq!(got, want, "{command:?}");
    }
}

#[test]
fn an_exact_bash_rule_matches_only_the_whole_command() {
    let exact = |action| PolicyRule {
        exact: true,
        ..rule("Bash", Some("npm test"), action)
    };
    let proxy = sandboxed(SandboxNetwork::Proxy);
    let decided = |policy: &Policy, command: &str| {
        let input = json!({ "command": command });
        decide(policy, "Bash", &input, Path::new(WORK), Some(&proxy)).map(|(a, _)| a)
    };
    let auto = Policy {
        quiet: false,
        rules: vec![exact(PolicyAction::Auto)],
    };
    assert_eq!(decided(&auto, "npm test"), Some(PolicyAction::Auto));
    assert_eq!(decided(&auto, "  npm   test "), Some(PolicyAction::Auto));
    for command in ["npm test -- -u", "npm test x", "npm", "npm testx"] {
        assert_eq!(decided(&auto, command), None, "{command:?}");
    }
    // An exact deny or ask would let longer commands through: refused,
    // and one stored anyway matches the prefix.
    for action in [PolicyAction::Deny, PolicyAction::Ask] {
        let refused = check(&exact(action)).err().map(|e| e.message);
        assert!(
            refused
                .as_deref()
                .is_some_and(|m| m.contains("only an auto rule")),
            "{refused:?}"
        );
        let stored = Policy {
            quiet: false,
            rules: vec![exact(action)],
        };
        assert_eq!(decided(&stored, "npm test -- -u"), Some(action));
    }
    // Exact means nothing without a Bash match.
    assert!(check(&exact(PolicyAction::Auto)).is_ok());
    for bad in [
        PolicyRule {
            exact: true,
            ..rule("Bash", None, PolicyAction::Auto)
        },
        PolicyRule {
            exact: true,
            ..rule("Edit", Some("a.md"), PolicyAction::Auto)
        },
    ] {
        assert!(check(&bad).is_err(), "{bad:?}");
    }
    // Stored rules without the field keep the prefix meaning.
    let stored: PolicyRule =
        serde_json::from_value(json!({"tool": "Bash", "match": "npm test", "action": "auto"}))
            .unwrap_or_else(|e| panic!("{e}"));
    assert!(!stored.exact);
}

#[test]
fn file_globs_stay_inside_the_sessions_folder() {
    let proxy = sandboxed(SandboxNetwork::Blocked);
    let abs = |rel: &str| Path::new(WORK).join(rel).to_string_lossy().into_owned();
    let cases = [
        ("docs/a.md".to_owned(), "Auto Edit(docs/**)"),
        ("docs/deep/er/b.md".to_owned(), "Auto Edit(docs/**)"),
        (abs("docs/c.md"), "Auto Edit(docs/**)"),
        ("docs/../src/main.rs".to_owned(), "ask"),
        ("../other/docs/a.md".to_owned(), "ask"),
        ("../w/docs/a.md".to_owned(), "Auto Edit(docs/**)"),
        ("src/docs/a.md".to_owned(), "ask"),
        ("/elsewhere/docs/a.md".to_owned(), "ask"),
    ];
    for (path, want) in cases {
        let got = judge(Some(&proxy), "Edit", json!({ "file_path": path }));
        assert_eq!(got, want, "{path:?}");
    }
    assert_eq!(
        judge(Some(&proxy), "Write", json!({"file_path": "docs/a.md"})),
        "ask"
    );
}

#[test]
fn set_refuses_rules_that_cannot_mean_what_they_say() {
    let bad = [
        rule("", None, PolicyAction::Auto),
        rule(" Bash", None, PolicyAction::Auto),
        rule("Bash", Some("  "), PolicyAction::Auto),
        rule("Edit", Some("/etc/**"), PolicyAction::Auto),
        rule("Edit", Some("../x"), PolicyAction::Auto),
        rule("Edit", Some("C:/x"), PolicyAction::Auto),
        rule("WebFetch", Some("*.evil.com"), PolicyAction::Deny),
        rule("WebFetch", Some("Evil.com"), PolicyAction::Deny),
        rule("WebFetch", Some("https://evil.com"), PolicyAction::Deny),
        // These could never match.
        rule("Grep", Some(".env"), PolicyAction::Deny),
        rule("Edit", Some("secrets/"), PolicyAction::Deny),
        rule("Edit", Some("./a.md"), PolicyAction::Deny),
        rule("mcp__ovra__spawn_agent", None, PolicyAction::Deny),
    ];
    for r in bad {
        assert!(check(&r).is_err(), "{r:?}");
    }
    assert!(check(&rule("Edit", Some("src/**/*.rs"), PolicyAction::Deny)).is_ok());
    assert!(check(&rule("WebFetch", None, PolicyAction::Deny)).is_ok());
    assert!(check(&rule("WebFetch", Some("docs.rs"), PolicyAction::Auto)).is_ok());
}

#[test]
fn webfetch_matches_the_urls_host_exactly() {
    let policy = Policy {
        quiet: false,
        rules: vec![rule("WebFetch", Some("docs.rs"), PolicyAction::Auto)],
    };
    let proxy = sandboxed(SandboxNetwork::Proxy);
    let decided = |url: &str| {
        let input = json!({ "url": url, "prompt": "x" });
        decide(&policy, "WebFetch", &input, Path::new(WORK), Some(&proxy)).is_some()
    };
    for url in [
        "https://docs.rs/serde",
        "HTTPS://Docs.RS:443/serde?x#y",
        "http://docs.rs",
    ] {
        assert!(decided(url), "{url}");
    }
    for url in [
        "https://docs.rs.evil.com/",
        "https://evil.com/docs.rs",
        "https://api.docs.rs/",
        "https://docs.rs@evil.com/",
        "https://evil.com\\@docs.rs/",
        "ftp://docs.rs/",
        "docs.rs",
        "https://docs.rs:x/",
    ] {
        assert!(!decided(url), "{url}");
    }
}

#[test]
fn a_webfetch_deny_covers_subdomains_and_holds_for_unreadable_urls() {
    let proxy = sandboxed(SandboxNetwork::Proxy);
    for action in [PolicyAction::Deny, PolicyAction::Ask] {
        let policy = Policy {
            quiet: false,
            rules: vec![rule("WebFetch", Some("evil.com"), action)],
        };
        let decided = |input: serde_json::Value| {
            decide(&policy, "WebFetch", &input, Path::new(WORK), Some(&proxy)).map(|(a, _)| a)
        };
        let url = |url: &str| json!({ "url": url, "prompt": "x" });
        for held in [
            "https://evil.com/",
            "https://EVIL.com./x",
            "https://api.evil.com/",
            "https://a.b.evil.com:8443/",
            "https://user@evil.com/",
            "https:evil.com",
            "https:/evil.com",
            "https://evil%2Ecom/",
            // An ideographic full stop, which URL parsers read as a dot.
            "https://evil\u{3002}com/",
            // Full-width letters, which IDNA maps to `evil`.
            "https://\u{ff45}\u{ff56}\u{ff49}\u{ff4c}.com/",
            // A Cyrillic `e`: Ovra cannot tell such a name apart.
            "https://\u{0435}vil.com/",
            "https://ev\til.com/",
            "https:////evil.com/",
        ] {
            assert_eq!(decided(url(held)), Some(action), "{held}");
        }
        assert_eq!(decided(json!({"prompt": "x"})), Some(action), "no url");
        for other in [
            "https://notevil.com/",
            "https://evil.com.example/",
            "https://example.com/evil.com",
            "https://xn--vil-8cd.com/",
        ] {
            assert_eq!(decided(url(other)), None, "{other}");
        }
    }
    // Auto allows only the host it names, and only a URL Ovra can read.
    let auto = Policy {
        quiet: false,
        rules: vec![rule("WebFetch", Some("docs.rs"), PolicyAction::Auto)],
    };
    for url in [
        "https://api.docs.rs/",
        "https:docs.rs",
        "https://docs%2Ers/",
    ] {
        let input = json!({ "url": url });
        let got = decide(&auto, "WebFetch", &input, Path::new(WORK), Some(&proxy));
        assert_eq!(got, None, "{url}");
    }
}

#[test]
fn an_ask_rule_wins_over_an_auto_rule() {
    let policy = Policy {
        quiet: false,
        rules: vec![
            rule("Bash", Some("npm test"), PolicyAction::Auto),
            rule("Bash", Some("npm test -- --update"), PolicyAction::Ask),
        ],
    };
    let sandbox = SandboxReport {
        name: "test".into(),
        network: SandboxNetwork::Blocked,
        gaps: Vec::new(),
    };
    let decided = |command: &str| {
        decide(
            &policy,
            "Bash",
            &json!({ "command": command }),
            Path::new(WORK),
            Some(&sandbox),
        )
        .map(|(action, _)| action)
    };
    assert_eq!(decided("npm test"), Some(PolicyAction::Auto));
    assert_eq!(decided("npm test -- --update"), Some(PolicyAction::Ask));
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn a_deny_holds_when_the_folders_case_differs() {
    let policy = Policy {
        quiet: false,
        rules: vec![rule("Read", Some(".env"), PolicyAction::Deny)],
    };
    let upper = Path::new(WORK).join(".ENV");
    let path = upper.to_string_lossy().to_uppercase();
    let decided = decide(
        &policy,
        "Read",
        &json!({ "file_path": path }),
        Path::new(WORK),
        None,
    );
    assert_eq!(decided.map(|(a, _)| a), Some(PolicyAction::Deny));
}

#[tokio::test]
async fn the_person_sets_a_projects_policy_and_bad_rules_are_refused() {
    use std::sync::Arc;

    use crate::Core;
    use crate::fakes::MemoryProjects;
    use crate::protocol::policy::{PolicyEvent, PolicyResponse};
    use crate::protocol::{ErrorCode, Event, Request, Response};

    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .build();
    let call = |json: serde_json::Value| {
        let core = core.clone();
        async move {
            let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
            core.handle(request).await
        }
    };
    let root = crate::test_path!("/w");
    let added = call(json!({"method": "projects.add", "params": {"root": root}})).await;
    assert!(added.is_ok(), "{added:?}");
    let get = json!({"method": "policy.get", "params": {"project": "p1"}});
    let empty = call(get.clone()).await;
    assert_eq!(
        empty.ok(),
        Some(Response::Policy(PolicyResponse::Policy(Policy::default())))
    );

    let mut rx = core.subscribe();
    let set = json!({"method": "policy.set", "params": {"project": "p1", "policy": {"rules": [
        {"tool": "Bash", "match": "npm test", "action": "auto"}]}}});
    assert!(call(set).await.is_ok());
    assert!(matches!(
        rx.try_recv(),
        Ok(Event::Policy(PolicyEvent::Changed { .. }))
    ));
    let Ok(Response::Policy(PolicyResponse::Policy(kept))) = call(get).await else {
        panic!("no policy");
    };
    assert_eq!(kept.rules.len(), 1);

    // A change built from an older policy would drop the rule just kept.
    let stale = json!({"method": "policy.set", "params": {"project": "p1",
        "policy": {"rules": [{"tool": "WebFetch", "action": "ask"}]},
        "expected": {"rules": []}}});
    assert_eq!(
        call(stale).await.err().map(|e| e.code),
        Some(ErrorCode::Conflict)
    );
    assert!(rx.try_recv().is_err(), "a refused change was announced");
    let current = json!({"method": "policy.set", "params": {"project": "p1",
        "policy": {"rules": [kept.rules[0].clone(), {"tool": "WebFetch", "action": "deny"}]},
        "expected": kept}});
    let Ok(Response::Policy(PolicyResponse::Policy(both))) = call(current).await else {
        panic!("the change built from the stored policy was refused");
    };
    assert_eq!(both.rules.len(), 2);
    let Ok(Response::Policy(PolicyResponse::Policy(stored))) =
        call(json!({"method": "policy.get", "params": {"project": "p1"}})).await
    else {
        panic!("no policy");
    };
    assert_eq!(stored, both);

    let bad = json!({"method": "policy.set", "params": {"project": "p1", "policy": {"rules": [
        {"tool": "Edit", "match": "/etc/passwd", "action": "auto"}]}}});
    assert_eq!(
        call(bad).await.err().map(|e| e.code),
        Some(ErrorCode::BadRequest)
    );
    let unknown = json!({"method": "policy.get", "params": {"project": "p9"}});
    assert_eq!(
        call(unknown).await.err().map(|e| e.code),
        Some(ErrorCode::NotFound)
    );
}

#[test]
fn a_commands_program_is_read_as_the_shell_finds_it() {
    let cases = [
        ("env", vec!["env"]),
        ("\"ENV\"", vec!["env"]),
        ("e\\nv", vec!["env", "nv"]),
        ("'e'nv.EXE", vec!["env"]),
        ("/usr/bin/env", vec!["env"]),
        (".\\bin\\bash.exe", vec![".binbash", "bash"]),
        ("python3.12", vec!["python"]),
        (".", vec!["."]),
        ("!", vec!["!"]),
        ("7z", vec!["7z"]),
    ];
    for (word, want) in cases {
        assert_eq!(program(word), want, "{word}");
    }
}

#[test]
fn always_allow_offers_the_narrowest_rule_or_none() {
    let proxy = sandboxed(SandboxNetwork::Proxy);
    // `@/` stands for the session's folder, so the lines read the same everywhere.
    let abs = |input: &serde_json::Value| {
        let mut input = input.clone();
        if let Some(rel) = input["file_path"]
            .as_str()
            .and_then(|p| p.strip_prefix("@/"))
        {
            input["file_path"] = Path::new(WORK).join(rel).to_string_lossy().into();
        }
        input
    };
    let cases = [
        ("Bash", json!({"command": "npm test"})),
        ("Bash", json!({"command": "  cargo   test -p ovra-core "})),
        ("Bash", json!({"command": "git commit -m \"fix typo\""})),
        ("Bash", json!({"command": "npm test && curl x"})),
        ("Bash", json!({"command": "sh -c 'npm test'"})),
        ("Bash", json!({"command": "env npm test"})),
        ("Bash", json!({"command": "\"env\" npm test"})),
        ("Bash", json!({"command": "\\env npm test"})),
        ("Bash", json!({"command": "e\\nv npm test"})),
        ("Bash", json!({"command": "'e'nv npm test"})),
        ("Bash", json!({"command": "ENV npm test"})),
        ("Bash", json!({"command": "env.exe npm test"})),
        ("Bash", json!({"command": "Env.EXE npm test"})),
        ("Bash", json!({"command": "./bash x.sh"})),
        ("Bash", json!({"command": ".\\bash.exe x.sh"})),
        ("Bash", json!({"command": "bin\\sudo npm test"})),
        ("Bash", json!({"command": "bash5 x.sh"})),
        ("Bash", json!({"command": "! npm test"})),
        ("Bash", json!({"command": ". ./x.sh"})),
        ("Bash", json!({"command": "source x.sh"})),
        ("Bash", json!({"command": "builtin eval x"})),
        ("Bash", json!({"command": "stdbuf -o0 npm test"})),
        ("Bash", json!({"command": "setsid npm test"})),
        ("Bash", json!({"command": "busybox sh"})),
        ("Bash", json!({"command": "systemd-run npm test"})),
        ("Bash", json!({"command": "npx cowsay"})),
        ("Bash", json!({"command": "uvx ruff"})),
        ("Bash", json!({"command": "wsl npm test"})),
        ("Bash", json!({"command": "start npm"})),
        ("Bash", json!({"command": "node"})),
        ("Bash", json!({"command": "python3.12"})),
        ("Bash", json!({"command": "Python3"})),
        ("Bash", json!({"command": "git"})),
        ("Bash", json!({"command": "git status"})),
        ("Bash", json!({"command": "node scripts/build.js"})),
        ("Bash", json!({"command": "rm *.tmp"})),
        ("Bash", json!({"command": "cat src/?.rs"})),
        ("Bash", json!({"command": "cat [.]claude/settings.json"})),
        ("Bash", json!({"command": "cat {.,}claude/x"})),
        ("Bash", json!({"command": "cat src$X/a"})),
        ("Bash", json!({"command": "cat .cl\"au\"de/x"})),
        ("Bash", json!({"command": "cat .c'laude'"})),
        ("Bash", json!({"command": "cat .cl\\aude/settings.json"})),
        ("Bash", json!({"command": "cp x .hus\\ky/pre-commit"})),
        ("Bash", json!({"command": "rg --file=\"x\" a"})),
        ("Bash", json!({"command": "echo 'two words'"})),
        ("Bash", json!({"command": "FOO=1 npm test"})),
        ("Bash", json!({"command": "cat /etc/passwd"})),
        ("Bash", json!({"command": "cat ../other/a"})),
        ("Bash", json!({"command": "cat ~/.ssh/id_rsa"})),
        ("Bash", json!({"command": "echo $HOME"})),
        ("Bash", json!({"command": "rg --file=/etc/x a"})),
        ("Bash", json!({"command": "cat ./CLAUDE.md"})),
        ("Bash", json!({"command": "rm -rf .claude"})),
        (
            "Bash",
            json!({"command": format!("echo {}", "a".repeat(200))}),
        ),
        ("Edit", json!({"file_path": "@/src/main.rs"})),
        ("Write", json!({"file_path": "docs/new.md"})),
        ("Edit", json!({"file_path": "/elsewhere/a.rs"})),
        ("Edit", json!({"file_path": "@/../other/a.rs"})),
        ("Edit", json!({"file_path": "@/.claude/settings.json"})),
        ("Write", json!({"file_path": "@/src/CLAUDE.md"})),
        ("Write", json!({"file_path": "@/.gitattributes"})),
        ("Edit", json!({"file_path": "@/src/a*.rs"})),
        ("Edit", json!({"file_path": "@/.git/config"})),
        ("Write", json!({"file_path": "@/.git/hooks/pre-commit"})),
        ("Write", json!({"file_path": "@/sub/.GIT/hooks/x"})),
        ("Write", json!({"file_path": "@/.git./hooks/x"})),
        ("Write", json!({"file_path": "@/GIT~1/hooks/x"})),
        ("Edit", json!({"file_path": "@/PACKAG~1.JSO"})),
        ("Edit", json!({"file_path": "@/PYPROJ~1.TOM"})),
        ("Edit", json!({"file_path": "@/src/notes~2/a.md"})),
        ("Edit", json!({"file_path": "@/src/notes~/a.md"})),
        ("Edit", json!({"file_path": "@/package.json"})),
        ("Edit", json!({"file_path": "@/ui/package.json"})),
        ("Edit", json!({"file_path": "@/Makefile"})),
        ("Edit", json!({"file_path": "@/makefile"})),
        ("Edit", json!({"file_path": "@/GNUmakefile"})),
        ("Edit", json!({"file_path": "@/build.rs"})),
        ("Edit", json!({"file_path": "@/crates/x/Cargo.toml"})),
        ("Edit", json!({"file_path": "@/pyproject.toml"})),
        ("Edit", json!({"file_path": "@/setup.py"})),
        ("Edit", json!({"file_path": "@/.npmrc"})),
        ("Edit", json!({"file_path": "@/justfile"})),
        ("Edit", json!({"file_path": "@/Justfile"})),
        ("Write", json!({"file_path": "@/.github/workflows/ci.yml"})),
        (
            "Write",
            json!({"file_path": "@/.GitHub/Workflows/deep/x.yml"}),
        ),
        ("Edit", json!({"file_path": "@/.cargo/config.toml"})),
        ("Edit", json!({"file_path": "@/src/git.rs"})),
        ("Edit", json!({"file_path": "@/.github/README.md"})),
        ("Edit", json!({"file_path": "@/docs/package.json.md"})),
        ("Read", json!({"file_path": "@/src/main.rs"})),
        (
            "WebFetch",
            json!({"url": "https://docs.rs/serde", "prompt": "x"}),
        ),
        (
            "WebFetch",
            json!({"url": "https://github.com/x", "prompt": "x"}),
        ),
        (
            "WebFetch",
            json!({"url": "https://8.8.8.8/", "prompt": "x"}),
        ),
        (
            "WebFetch",
            json!({"url": "https://docs.rs@evil.com/", "prompt": "x"}),
        ),
        ("WebSearch", json!({"query": "serde"})),
        ("mcp__github__create_issue", json!({})),
    ];
    let lines: Vec<String> = cases
        .iter()
        .map(|(tool, input)| {
            let offered = always_rule(tool, &abs(input), Path::new(WORK), Some(&proxy));
            let offered = offered.map_or("-".to_owned(), |r| {
                let exact = if r.exact { " exactly" } else { "" };
                format!("{}{exact}", text(&r))
            });
            format!("{tool} {} => {offered}", input)
        })
        .collect();
    insta::assert_snapshot!(lines.join("\n"));

    // Only where an auto rule would count.
    let test = json!({"command": "npm test"});
    let open = sandboxed(SandboxNetwork::Open);
    assert_eq!(always_rule("Bash", &test, Path::new(WORK), None), None);
    assert_eq!(
        always_rule("Bash", &test, Path::new(WORK), Some(&open)),
        None
    );
}

#[tokio::test]
async fn always_allow_adds_the_rule_answers_and_the_next_call_is_allowed() {
    use std::sync::Arc;

    use crate::Core;
    use crate::agents::AgentHost;
    use crate::fakes::{MemoryAudit, MemoryProjects, ScriptedAgent, Step};
    use crate::protocol::agents::SessionId;
    use crate::protocol::audit::{AuditEvent, DecidedBy};
    use crate::protocol::permissions::{Decision, PermissionsEvent};
    use crate::protocol::policy::{PolicyEvent, PolicyResponse};
    use crate::protocol::projects::ProjectId;
    use crate::protocol::{ErrorCode, Event, Request, Response};
    use crate::services::AuditLog;

    let audit = Arc::new(MemoryAudit::default());
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(ScriptedAgent::new(vec![vec![Step::Say("hi")]])))
        .audit(audit.clone())
        .build();
    let call = |json: serde_json::Value| {
        let core = core.clone();
        async move {
            let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
            core.handle(request).await
        }
    };
    let root = crate::test_path!("/w");
    assert!(
        call(json!({"method": "projects.add", "params": {"root": root}}))
            .await
            .is_ok()
    );
    let start = json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "direct", "prompt": "x"}});
    assert!(call(start).await.is_ok());
    let boxed = AgentHost::new(
        SessionId("s1".into()),
        ProjectId("p1".into()),
        core.inner_sessions(),
        core.sink(),
    );
    boxed.sandboxed(None);

    let mut rx = core.subscribe();
    let test = json!({"command": "npm test"});
    let asking = |host: AgentHost| {
        let test = test.clone();
        tokio::spawn(async move { host.ask(crate::fakes::tool("Bash", test), None, None).await })
    };
    let requested = |rx: &mut tokio::sync::broadcast::Receiver<Event>| {
        let mut rx = rx.resubscribe();
        async move {
            loop {
                if let Some(Event::Permissions(PermissionsEvent::Requested(r))) =
                    Core::next_event(&mut rx).await
                {
                    return r;
                }
            }
        }
    };
    // Without a sandbox nothing is offered, and asking for it is refused.
    let waiting = requested(&mut rx);
    let unsandboxed = asking(boxed.clone());
    let request = waiting.await;
    assert_eq!(request.always, None);
    let answer = |id: &str| {
        json!({"method": "permissions.answer", "params": {
        "id": id, "decision": "allow", "always": true}})
    };
    assert_eq!(
        call(answer(&request.id.0)).await.err().map(|e| e.code),
        Some(ErrorCode::BadRequest)
    );
    unsandboxed.abort();
    boxed.sandboxed(Some(sandboxed(SandboxNetwork::Proxy)));

    let waiting = requested(&mut rx);
    let sandboxed_call = asking(boxed.clone());
    let request = waiting.await;
    let rule = request
        .always
        .clone()
        .unwrap_or_else(|| panic!("no rule offered"));
    assert_eq!(text(&rule), "Bash(npm test)");
    assert!(rule.exact, "an offered command rule is not a prefix");
    let deny = json!({"method": "permissions.answer", "params": {
        "id": request.id.0, "decision": "deny", "always": true}});
    assert_eq!(
        call(deny).await.err().map(|e| e.code),
        Some(ErrorCode::BadRequest)
    );
    let mut events = rx.resubscribe();
    assert!(call(answer(&request.id.0)).await.is_ok());
    let allowed = sandboxed_call.await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(allowed.decision, Decision::Allow);
    let changed = std::iter::from_fn(|| events.try_recv().ok())
        .any(|e| matches!(e, Event::Policy(PolicyEvent::Changed { .. })));
    assert!(changed, "the new rule was not announced");
    let Ok(Response::Policy(PolicyResponse::Policy(kept))) =
        call(json!({"method": "policy.get", "params": {"project": "p1"}})).await
    else {
        panic!("no policy");
    };
    assert_eq!(kept.rules, vec![rule.clone()]);
    assert!(kept.rules[0].from_approval);

    // The next identical call is decided by the rule, without a request.
    let again = boxed
        .ask(crate::fakes::tool("Bash", test.clone()), None, None)
        .await;
    assert_eq!(again.decision, Decision::Allow);
    // A longer command starting the same way is asked about.
    let longer = json!({"command": "npm test -- --reporter x"});
    let proxy = sandboxed(SandboxNetwork::Proxy);
    assert_eq!(
        decide(&kept, "Bash", &longer, Path::new(WORK), Some(&proxy)),
        None
    );

    // The log names who added which rule, for the session that asked.
    let entries = audit.list(&ProjectId("p1".into()), Default::default(), 100);
    let entries = entries.unwrap_or_default();
    let added: Vec<_> = entries
        .iter()
        .filter_map(|e| match &e.event {
            AuditEvent::RuleAdded { rule, by } => {
                Some((e.session.clone(), rule.clone(), by.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        added,
        vec![(Some(SessionId("s1".into())), rule, DecidedBy::Person)]
    );
    let by_rule = entries.iter().any(|e| {
        matches!(&e.event, AuditEvent::ToolDecided { by: DecidedBy::Policy { rule }, .. }
            if rule == "Bash(npm test)")
    });
    assert!(by_rule, "the second call was not decided by the rule");
}

#[tokio::test]
async fn always_allow_takes_the_request_first_and_adds_no_rule_twice() {
    use std::sync::Arc;

    use crate::Core;
    use crate::agents::AgentHost;
    use crate::fakes::{MemoryProjects, ScriptedAgent, Step};
    use crate::protocol::agents::SessionId;
    use crate::protocol::permissions::{Decision, PermissionsEvent};
    use crate::protocol::policy::PolicyResponse;
    use crate::protocol::projects::ProjectId;
    use crate::protocol::{ErrorCode, Event, Request, Response};

    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(ScriptedAgent::new(vec![vec![Step::Say("hi")]])))
        .build();
    let call = |json: serde_json::Value| {
        let core = core.clone();
        async move {
            let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
            core.handle(request).await
        }
    };
    let root = crate::test_path!("/w");
    assert!(
        call(json!({"method": "projects.add", "params": {"root": root}}))
            .await
            .is_ok()
    );
    let start = json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "direct", "prompt": "x"}});
    assert!(call(start).await.is_ok());
    let host = AgentHost::new(
        SessionId("s1".into()),
        ProjectId("p1".into()),
        core.inner_sessions(),
        core.sink(),
    );
    host.sandboxed(Some(sandboxed(SandboxNetwork::Blocked)));
    let rx = core.subscribe();
    let ask = || {
        let host = host.clone();
        tokio::spawn(async move {
            host.ask(
                crate::fakes::tool("Bash", json!({"command": "npm test"})),
                None,
                None,
            )
            .await
        })
    };
    let requested = || {
        let mut rx = rx.resubscribe();
        async move {
            loop {
                if let Some(Event::Permissions(PermissionsEvent::Requested(r))) =
                    Core::next_event(&mut rx).await
                {
                    return r;
                }
            }
        }
    };
    let set = |rules: serde_json::Value| json!({"method": "policy.set", "params": {"project": "p1", "policy": {"rules": rules}}});
    let always = |id: &str| {
        json!({"method": "permissions.answer", "params": {
            "id": id, "decision": "allow", "always": true}})
    };
    let rules = || async {
        match call(json!({"method": "policy.get", "params": {"project": "p1"}})).await {
            Ok(Response::Policy(PolicyResponse::Policy(p))) => p.rules,
            other => panic!("{other:?}"),
        }
    };
    let pending =
        || async { format!("{:?}", call(json!({"method": "permissions.pending"})).await) };

    // A rule the person wrote in Settings is the same rule: none is added.
    let waiting = requested();
    let asking = ask();
    let request = waiting.await;
    let mine = json!([{"tool": "Bash", "match": "npm test", "action": "auto", "exact": true}]);
    assert!(call(set(mine)).await.is_ok());
    assert!(call(always(&request.id.0)).await.is_ok());
    let answered = asking.await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(answered.decision, Decision::Allow);
    let kept = rules().await;
    assert_eq!(kept.len(), 1);
    assert!(!kept[0].from_approval, "{kept:?}");

    // A request answered already adds no rule, and says so.
    assert!(call(set(json!([]))).await.is_ok());
    let waiting = requested();
    let asking = ask();
    let request = waiting.await;
    let once = json!({"method": "permissions.answer", "params": {
        "id": request.id.0, "decision": "allow"}});
    assert!(call(once).await.is_ok());
    assert!(asking.await.is_ok());
    let gone = call(always(&request.id.0)).await.err();
    assert_eq!(gone.as_ref().map(|e| e.code), Some(ErrorCode::NotFound));
    assert!(
        gone.is_some_and(|e| e.message.contains("no rule was added")),
        "the refusal does not say no rule was added"
    );
    assert_eq!(rules().await, Vec::new());

    // Refused once a rule asks: nothing is added and the request still
    // waits for Allow once or Deny.
    let waiting = requested();
    let asking = ask();
    let request = waiting.await;
    assert!(request.always.is_some());
    let ask_rule = json!([{"tool": "Bash", "match": "npm", "action": "ask"}]);
    assert!(call(set(ask_rule)).await.is_ok());
    assert_eq!(
        call(always(&request.id.0)).await.err().map(|e| e.code),
        Some(ErrorCode::BadRequest)
    );
    assert_eq!(rules().await.len(), 1);
    assert!(pending().await.contains(&request.id.0), "it was dropped");
    let deny = json!({"method": "permissions.answer", "params": {
        "id": request.id.0, "decision": "deny"}});
    assert!(call(deny).await.is_ok());
    let denied = asking.await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(denied.decision, Decision::Deny);
}

fn push_rule(pattern: Option<&str>, action: PolicyAction) -> PolicyRule {
    PolicyRule {
        tool: super::PUSH_TOOL.into(),
        pattern: pattern.map(str::to_owned),
        action,
        exact: false,
        from_approval: false,
    }
}

#[test]
fn push_rules_take_a_remote_and_a_branch_or_every_branch_below_one() {
    for good in [
        None,
        Some("origin ovra/*"),
        Some("origin fix-login"),
        Some("up main"),
    ] {
        assert!(
            check(&push_rule(good, PolicyAction::Auto)).is_ok(),
            "{good:?}"
        );
    }
    for bad in [
        "origin",
        "origin *",
        "origin ovra/**",
        "https://x@github.com/o/r main",
        "--upload-pack=x main",
        "origin --force",
        "origin a..b",
        "origin  main",
    ] {
        assert!(
            check(&push_rule(Some(bad), PolicyAction::Deny)).is_err(),
            "{bad}"
        );
    }
    let exact = PolicyRule {
        exact: true,
        ..push_rule(Some("origin main"), PolicyAction::Auto)
    };
    assert!(check(&exact).is_err());
}

#[test]
fn push_rules_decide_pushes_and_never_a_tool_of_that_name() {
    use super::{PushTarget, decide_push, protected_branch};
    let contained = sandboxed(SandboxNetwork::Blocked);
    let open = sandboxed(SandboxNetwork::Open);
    let policy = Policy {
        quiet: false,
        rules: vec![
            push_rule(Some("origin ovra/*"), PolicyAction::Auto),
            push_rule(Some("origin ovra/secret/*"), PolicyAction::Deny),
            push_rule(Some("origin team"), PolicyAction::Ask),
            push_rule(None, PolicyAction::Auto),
        ],
    };
    let judge = |remote: &str, branch: &str, sandbox: Option<&SandboxReport>| {
        let target = PushTarget {
            remote,
            branch,
            protected: protected_branch(branch, Some("trunk-of-team")),
        };
        match decide_push(&policy, &target, sandbox) {
            Some((action, rule)) => format!("{action:?} {rule}"),
            None => "ask".to_owned(),
        }
    };
    let c = Some(&contained);
    assert_eq!(judge("origin", "ovra/t1/w", c), "Auto Push(origin ovra/*)");
    assert_eq!(
        judge("origin", "ovra/secret/w", c),
        "Deny Push(origin ovra/secret/*)"
    );
    assert_eq!(judge("origin", "team", c), "Ask Push(origin team)");
    assert_eq!(judge("origin", "feature", c), "Auto Push");
    // Wide rules never push to a shared branch without asking.
    for shared in [
        "main",
        "Master",
        "release/1.2",
        "release-3",
        "hotfix/x",
        "trunk-of-team",
    ] {
        assert_eq!(judge("origin", shared, c), "ask", "{shared}");
    }
    // `auto` counts only where the sandbox limits the network.
    assert_eq!(judge("origin", "ovra/t1/w", Some(&open)), "ask");
    assert_eq!(judge("origin", "ovra/t1/w", None), "ask");
    assert_eq!(judge("upstream", "ovra/t1/w", None), "ask");

    // A rule that names the shared branch exactly does push to it.
    let named = Policy {
        quiet: false,
        rules: vec![push_rule(Some("origin main"), PolicyAction::Auto)],
    };
    let main = PushTarget {
        remote: "origin",
        branch: "main",
        protected: true,
    };
    assert!(decide_push(&named, &main, c).is_some());

    // An agent's own tool named `Push` is not decided by push rules.
    let call = crate::protocol::agents::ToolCall::default();
    assert_eq!(
        super::decide(&policy, super::PUSH_TOOL, &call, Path::new("/w"), c),
        None
    );
}

#[test]
fn always_allow_offers_ovras_branches_and_never_another_one() {
    use super::{PushTarget, always_push};
    let contained = sandboxed(SandboxNetwork::Proxy);
    let offered = |branch: &str, protected: bool| {
        let target = PushTarget {
            remote: "origin",
            branch,
            protected,
        };
        always_push(&target, Some(&contained)).map(|r| text(&r))
    };
    assert_eq!(
        offered("ovra/t1/w", false).as_deref(),
        Some("Push(origin ovra/*)")
    );
    assert_eq!(offered("fix-login", false), None);
    assert_eq!(offered("main", true), None);
    let target = PushTarget {
        remote: "origin",
        branch: "fix-login",
        protected: false,
    };
    assert_eq!(always_push(&target, None), None);
}

#[test]
fn use_key_rules_name_one_key_and_never_match_an_agents_tool() {
    use super::keys;
    let bash = crate::protocol::agents::ToolCall::default();
    assert!(check(&rule("UseKey", Some("github"), PolicyAction::Auto)).is_ok());
    assert!(check(&rule("UseKey", None, PolicyAction::Deny)).is_ok());
    for bad in [
        rule("UseKey", None, PolicyAction::Auto),
        rule("UseKey", Some("a b"), PolicyAction::Ask),
    ] {
        assert!(check(&bad).is_err(), "{bad:?}");
    }
    let policy = Policy {
        quiet: false,
        rules: vec![
            rule("UseKey", Some("github"), PolicyAction::Auto),
            rule("UseKey", Some("stripe"), PolicyAction::Deny),
        ],
    };
    let contained = sandboxed(SandboxNetwork::Proxy);
    let decided = |name: &str, sandbox| {
        keys::decide(&policy, name, sandbox).map(|(a, r)| format!("{a:?} {r}"))
    };
    assert_eq!(
        decided("github", Some(&contained)).as_deref(),
        Some("Auto UseKey(github)")
    );
    assert_eq!(
        decided("github", None),
        None,
        "auto counts only in a sandbox"
    );
    assert_eq!(
        decided("stripe", None).as_deref(),
        Some("Deny UseKey(stripe)")
    );
    assert_eq!(decided("npm", Some(&contained)), None);
    // An agent's own tool called UseKey is no key.
    assert_eq!(
        super::decide(&policy, "UseKey", &bash, Path::new("/w"), Some(&contained)),
        None
    );
    assert_eq!(
        keys::always("github", Some(&contained))
            .map(|r| text(&r))
            .as_deref(),
        Some("UseKey(github)")
    );
    assert_eq!(keys::always("github", None), None);
}
