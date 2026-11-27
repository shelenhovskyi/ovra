use ovra_core::services::ServerProfile;

use super::*;

fn server(project: &Path, cache: &Path) -> ServerProfile {
    ServerProfile {
        project: project.to_path_buf(),
        cache_dir: cache.to_path_buf(),
        read: Vec::new(),
        network: ProfileNetwork::Blocked,
    }
}

/// What `ovra-lsp` passes for rust-analyzer.
fn server_line(program: &Path, cache: &Path) -> CommandLine {
    let target = cache.join("target").into_os_string();
    let tmp = cache.join("tmp").into_os_string();
    CommandLine {
        program: program.to_path_buf(),
        args: Vec::new(),
        env: vec![
            ("CARGO_TARGET_DIR".into(), Some(target)),
            ("TMPDIR".into(), Some(tmp)),
            ("CARGO_NET_OFFLINE".into(), Some("true".into())),
        ],
        cwd: "/".into(),
    }
}

/// Ovra's environment: what agents keep, and secrets.
fn outer_env() -> Vec<OsString> {
    let mut env = Root::outer_env();
    env.extend(
        [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "CLAUDE_CODE_USE_BEDROCK",
            "AWS_REGION",
            "GOOGLE_CLOUD_PROJECT",
            "OVRA_FAKE_CLAUDE_FIXTURE",
            "CARGO_HOME",
        ]
        .map(Into::into),
    );
    env
}

#[test]
fn a_language_server_reads_the_project_and_writes_only_its_cache() {
    let root = Root::new("server");
    let project = root.dir("home/projects/app");
    root.file("home/projects/app/.git/config");
    root.file("home/projects/app/build.rs");
    root.file("home/.ssh/id_rsa");
    root.file("home/.claude/.credentials.json");
    root.file("home/.claude.json");
    root.file("home/.cargo/credentials.toml");
    root.dir("home/.cargo/registry/src");
    root.dir("home/.cargo/bin");
    root.dir("home/.rustup/toolchains");
    root.dir("home/.npm/_cacache");
    root.dir("home/.cache/pip");
    let cache = root.dir("data/lsp/p1/rust");
    let program = root.file("home/.cargo/bin/rust-analyzer");
    let mut setup = root.setup("HOME/.cargo/bin:/usr/bin");
    setup.toolchains.push(root.0.join("home/.rustup"));
    let plan = Plan::server(&setup, &server(&project, &cache)).unwrap();
    let line = plan
        .line(
            Path::new("/usr/bin/bwrap"),
            &setup.helper,
            Landlock::Require,
            server_line(&program, &cache),
            &outer_env(),
        )
        .unwrap();
    insta::assert_snapshot!(root.show(&line));

    let home = &setup.home;
    assert_eq!(line.cwd, project);
    assert!(line.args.contains(&"--unshare-net".into()));
    // The project read-only; the cache the only folder bound writable.
    assert!(plan.mounts.contains(&Mount::Ro(project.clone())));
    let rw: Vec<&Mount> = plan
        .mounts
        .iter()
        .filter(|m| matches!(m, Mount::Rw(_) | Mount::Private { .. }))
        .collect();
    assert_eq!(rw, [&Mount::Rw(cache.clone())]);
    assert_eq!(
        plan.writable,
        [PathBuf::from("/tmp"), "/dev".into(), cache.clone()]
    );
    // No agent state, nothing of claude's.
    let claude = home.join(".claude");
    for mount in &plan.mounts {
        let (Mount::Ro(p) | Mount::Rw(p) | Mount::Null(p) | Mount::Tmpfs(p)) = mount else {
            panic!("{mount:?}");
        };
        assert!(
            !p.starts_with(&claude) && *p != home.join(".claude.json"),
            "{p:?}"
        );
    }
    // The user's caches read from where they are; nothing made for them.
    assert!(plan.mounts.contains(&Mount::Ro(home.join(".npm"))));
    assert!(plan.mounts.contains(&Mount::Ro(home.join(".cache/pip"))));
    assert!(plan.mounts.contains(&Mount::Ro(home.join(".cargo"))));
    assert!(plan.mounts.contains(&Mount::Ro(home.join(".rustup"))));
    assert!(!root.0.join("data/sandbox-caches").exists());
    assert!(!home.join(".cache/pnpm").exists());
    // The line's own temporary folder wins over the plan's.
    let tmpdir = line.env.iter().rev().find(|(n, _)| n == "TMPDIR");
    assert_eq!(
        tmpdir,
        Some(&("TMPDIR".into(), Some(cache.join("tmp").into())))
    );
}

#[test]
fn a_language_server_keeps_no_name_of_claude_or_a_cloud_provider() {
    for name in ENV_PERSON.iter().chain(&ENV_TOOLS) {
        for word in [
            "ANTHROPIC",
            "CLAUDE",
            "AWS",
            "GOOGLE",
            "CLOUD",
            "OVRA",
            "TOKEN",
            "KEY",
        ] {
            assert!(!name.contains(word), "{name}");
        }
    }
    let root = Root::new("server-env");
    let project = root.dir("project");
    let cache = root.dir("data/lsp/p1/rust");
    let setup = root.setup("/usr/bin");
    let line = Plan::server(&setup, &server(&project, &cache))
        .unwrap()
        .line(
            Path::new("/usr/bin/bwrap"),
            &setup.helper,
            Landlock::Try,
            server_line(Path::new("/bin/sh"), &cache),
            &outer_env(),
        )
        .unwrap();
    for name in [
        "ANTHROPIC_API_KEY",
        "OVRA_FAKE_CLAUDE_FIXTURE",
        "AWS_REGION",
        "GITHUB_TOKEN",
    ] {
        assert!(removed(&line.env, &name.into()), "{name}");
        let kept = line
            .args
            .windows(2)
            .any(|w| w[0] == "--env" && w[1] == name);
        assert!(!kept, "{name}");
    }
    for name in [
        "HOME",
        "CARGO_HOME",
        "CARGO_TARGET_DIR",
        "CARGO_NET_OFFLINE",
    ] {
        let kept = line
            .args
            .windows(2)
            .any(|w| w[0] == "--env" && w[1] == name);
        assert!(kept, "{name}");
    }
}

#[test]
fn a_language_server_gets_no_network_and_no_folder_it_could_misuse() {
    let root = Root::new("server-refused");
    let setup = root.setup("/usr/bin");
    let project = root.dir("project");
    let cache = root.dir("data/lsp/p1/rust");
    let socket = root.file("data/proxy/s.sock");
    let refused = |profile: &ServerProfile, why: &str| {
        let err = Plan::server(&setup, profile).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
        assert!(err.message.contains(why), "{}", err.message);
    };
    for network in [
        ProfileNetwork::Open,
        ProfileNetwork::Proxy {
            address: socket.display().to_string(),
            ca_bundle: None,
            variables: Vec::new(),
        },
    ] {
        let mut profile = server(&project, &cache);
        profile.network = network;
        refused(&profile, "without the network");
    }
    for dir in [setup.home.clone(), root.0.clone(), PathBuf::from("/")] {
        refused(&server(&dir, &cache), "holds the home folder");
        let mut read = server(&project, &cache);
        read.read.push(dir);
        refused(&read, "holds the home folder");
    }
    refused(
        &server(&project, &root.dir("project/target/lsp")),
        "outside the project",
    );
    refused(
        &server(&root.dir("data/lsp/p1"), &cache),
        "outside the project",
    );
    refused(
        &server(&project, &root.dir("elsewhere/lsp")),
        "inside the lsp folder of Ovra's data folder",
    );
    refused(
        &server(&project, &root.0.join("data")),
        "inside the lsp folder of Ovra's data folder",
    );
    let mut read = server(&project, &cache);
    read.read.push(root.dir("data/worktrees"));
    refused(&read, "it is Ovra's");
}

#[test]
fn a_language_server_cache_is_only_in_ovras_lsp_folder() {
    let root = Root::new("server-cache");
    let mut setup = root.setup("/usr/bin");
    // The temporary folder is hidden too: it holds the netproxy's sockets.
    let temp = root.dir("tmp");
    setup.hidden.push(temp.clone());
    let project = root.dir("project");
    for cache in [
        root.dir("tmp/ovra-netproxy"),
        root.dir("data/sandbox-caches/app"),
        root.dir("data/worktrees/app"),
        root.dir("data/lsp"),
    ] {
        let err = Plan::server(&setup, &server(&project, &cache)).unwrap_err();
        assert!(err.message.contains("lsp folder"), "{}", err.message);
    }
    let cache = root.dir("data/lsp/p1/rust");
    assert!(Plan::server(&setup, &server(&project, &cache)).is_ok());
}

#[test]
fn a_language_server_reads_no_secrets_and_nothing_of_claude() {
    let root = Root::new("server-secrets");
    let setup = root.setup("/usr/bin");
    let project = root.dir("project");
    let cache = root.dir("data/lsp/p1/rust");
    for path in [
        "home/.config",
        "home/.config/gh",
        "home/.ssh",
        "home/.aws/sso",
        "home/.gnupg",
        "home/.docker",
        "home/.kube",
        "home/.local",
        "home/.local/share/keyrings",
        "home/.claude",
        "home/.claude/agents",
        "home/.claude.json",
        "home/.codex",
        "home/.codex/auth.json",
        "home/.gemini",
        "home/.gemini/oauth_creds.json",
        "home/.netrc",
        "home/.git-credentials",
        "home/.npmrc",
        "home/.cargo/credentials.toml",
        "home/.cargo/credentials",
        "home/.config/gcloud/credentials.json",
    ] {
        let path = if path.contains("credentials") || path.ends_with("rc") || path.ends_with("json")
        {
            root.file(path)
        } else {
            root.dir(path)
        };
        let mut profile = server(&project, &cache);
        profile.read.push(path.clone());
        let err = Plan::server(&setup, &profile).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
        assert!(
            err.message.contains("holds secrets"),
            "{path:?}: {}",
            err.message
        );
    }
    // A toolchain folder is fine: its own secrets are covered.
    let mut profile = server(&project, &cache);
    profile.read.push(root.dir("home/.local/lib/node"));
    assert!(Plan::server(&setup, &profile).is_ok());
}

#[test]
fn ovra_data_inside_a_folder_a_server_reads_stays_hidden_but_its_cache() {
    let root = Root::new("server-data-inside");
    let shared = root.dir("shared");
    let project = root.dir("project");
    root.file("shared/data/store.db");
    let cache = root.dir("shared/data/lsp/p1/rust");
    let mut setup = root.setup("/usr/bin");
    setup.hidden = vec![shared.join("data")];
    setup.data_dir = shared.join("data");
    let mut profile = server(&project, &cache);
    profile.read.push(shared.clone());
    let plan = Plan::server(&setup, &profile).unwrap();
    let n = plan.mounts.len();
    assert_eq!(
        plan.mounts[n - 2..],
        [Mount::Tmpfs(shared.join("data")), Mount::Rw(cache.clone())]
    );
    let line = plan.line(
        Path::new("/usr/bin/bwrap"),
        &setup.helper,
        Landlock::Try,
        server_line(Path::new("/bin/sh"), &cache),
        &outer_env(),
    );
    assert!(line.is_ok(), "{:?}", line.err());
}
