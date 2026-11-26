#![allow(clippy::unwrap_used)]

use std::ffi::OsStr;

use super::*;

/// The Claude runtime's own lists, compiled here so the plans below are
/// those of `claude`'s real state. Read as source: this crate cannot depend
/// on the runtime, so the crate graph does not see this.
#[path = "../../../agent-runtime/src/claude_lists.rs"]
mod claude_lists;

/// `claude`'s state, in the account's folder `config_dir` if given.
fn claude(config_dir: Option<PathBuf>) -> Option<AgentState> {
    Some(AgentState {
        lists: &claude_lists::CLAUDE_LISTS,
        config_dir,
    })
}

const PROTECTED: Protected = Protected {
    work_dir: &[".claude", "**/CLAUDE.md"],
};

struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ovra-plan-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(fs::canonicalize(dir).unwrap())
    }

    fn dir(&self, path: &str) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn file(&self, path: &str) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "").unwrap();
        path
    }

    fn setup(&self, path: &str) -> Setup {
        Setup {
            helper: self.file("bin/ovra"),
            home: self.dir("home"),
            path: path
                .replace("HOME", &self.0.join("home").display().to_string())
                .into(),
            toolchains: vec![self.0.join("home/.cargo")],
            hidden: vec![self.0.join("data")],
            data_dir: self.0.join("data"),
            caches: self.0.join("data/sandbox-caches"),
            protected: PROTECTED,
        }
    }

    /// Ovra's environment in these tests: two kept names, the rest removed.
    fn outer_env() -> Vec<OsString> {
        [
            "HOME",
            "LANG",
            "GITHUB_TOKEN",
            "SSH_AUTH_SOCK",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "CLAUDECODE",
        ]
        .map(Into::into)
        .to_vec()
    }

    /// The arguments of `line`, one per line, with this root as `ROOT`.
    fn show(&self, line: &CommandLine) -> String {
        let root = self.0.display().to_string();
        let key = project_key(&self.0);
        let mut out = String::new();
        let mut args = line.args.iter().map(|a| {
            a.to_string_lossy()
                .replace(&root, "ROOT")
                .replace(&key, "-ROOT")
        });
        while let Some(arg) = args.next() {
            out.push_str(&arg);
            if arg.starts_with("--") && !arg.starts_with("--unshare") && arg != "--" {
                match arg.as_str() {
                    "--ro-bind" | "--bind" => {
                        let (from, to) = (
                            args.next().unwrap_or_default(),
                            args.next().unwrap_or_default(),
                        );
                        if from == to {
                            out.push_str(&format!(" {from}"));
                        } else {
                            out.push_str(&format!(" {from} {to}"));
                        }
                    }
                    "--cap-drop" | "--tmpfs" | "--chdir" | "--rw" | "--landlock" | "--dev"
                    | "--proc" => {
                        out.push_str(&format!(" {}", args.next().unwrap_or_default()));
                    }
                    _ => {}
                }
            }
            out.push('\n');
        }
        for (name, value) in &line.env {
            let value = value
                .as_deref()
                .map(|v| v.to_string_lossy().replace(&root, "ROOT"));
            out.push_str(&format!("env {} = {value:?}\n", name.to_string_lossy()));
        }
        // The resolver's file kept under `/run` depends on the machine
        // (systemd-resolved's stub, or none).
        out.lines()
            .filter(|l| !l.starts_with("--ro-bind /run/"))
            .map(|l| format!("{l}\n"))
            .collect()
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn profile(work_dir: &Path) -> SandboxProfile {
    SandboxProfile {
        kind: ovra_core::services::ProfileKind::Agent,
        agent_state: claude(None),
        project: Some(ProjectId("p1".into())),
        work_env: None,
        work_dir: work_dir.to_path_buf(),
        work_dir_read_only: false,
        read: Vec::new(),
        write: Vec::new(),
        expose: Vec::new(),
        forward: Vec::new(),
        network: ProfileNetwork::Open,
    }
}

fn line(program: &Path, cwd: &Path) -> CommandLine {
    CommandLine {
        program: program.to_path_buf(),
        args: vec!["-p".into()],
        env: vec![("CLAUDECODE".into(), None)],
        cwd: cwd.to_path_buf(),
    }
}

#[test]
fn a_direct_session_in_a_repository() {
    let root = Root::new("direct");
    let project = root.dir("projects/app");
    root.file("projects/app/.claude/settings.json");
    root.file("projects/app/docs/guide/CLAUDE.md");
    root.file("projects/app/.git/config");
    root.dir("projects/app/.git/modules/lib");
    root.file("home/.ssh/id_ed25519");
    root.file("home/.claude.json");
    root.file("home/.cargo/credentials.toml");
    root.dir("home/.cargo/bin");
    root.dir("home/.local/bin");
    root.dir("data/worktrees");
    let claude = root.file("home/.local/share/claude/versions/2.1.0");
    let setup = root.setup("HOME/.local/bin:HOME/.cargo/bin:HOME:.:relative:/usr/bin");
    let plan = Plan::new(&setup, &profile(&project)).unwrap();
    let line = plan.line(
        Path::new("/usr/bin/bwrap"),
        &setup.helper,
        Landlock::Require,
        line(&claude, &project),
        &Root::outer_env(),
    );
    let line = line.unwrap();
    assert_eq!(line.program, setup.helper);
    insta::assert_snapshot!(root.show(&line));
    // The whole git folder is read-only: nothing in it needs making.
    assert!(!project.join(".git/hooks").exists());
}

#[test]
fn a_worker_reads_its_git_folder_behind_a_proxy() {
    let root = Root::new("worker");
    let common = root.dir("projects/app/.git");
    root.file("projects/app/.git/config");
    let own = root.dir("projects/app/.git/worktrees/w1");
    let worktree = root.dir("data/worktrees/app/t/w1");
    fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", own.display()),
    )
    .unwrap();
    fs::write(
        own.join("gitdir"),
        format!("{}\n", worktree.join(".git").display()),
    )
    .unwrap();
    fs::write(own.join("commondir"), "../..\n").unwrap();
    let socket = root.file("data/proxy/w1.sock");
    let bundle = root.file("data/proxy/ca.pem");
    let setup = root.setup("/usr/bin");
    let mut profile = profile(&worktree);
    profile.read.push(common);
    profile.network = ProfileNetwork::Proxy {
        address: socket.display().to_string(),
        ca_bundle: Some(bundle),
    };
    profile.work_env = Some(WorkEnv {
        port: 41010,
        worktree_root: worktree.clone(),
        main_root: root.0.join("projects/app"),
    });
    let plan = Plan::new(&setup, &profile).unwrap();
    let line = plan.line(
        Path::new("/usr/bin/bwrap"),
        &setup.helper,
        Landlock::Try,
        line(Path::new("/bin/sh"), &worktree),
        &Root::outer_env(),
    );
    insta::assert_snapshot!(root.show(&line.unwrap()));
    // Its git folder stays read-only with the rest of the repository's.
    assert!(!own.join("config.worktree").exists());
}

/// The folder `plan` binds at `~/.npm`.
fn npm_cache(plan: &Plan) -> PathBuf {
    let found = plan.mounts.iter().find_map(|m| match m {
        Mount::Private { from, at } if at.ends_with(".npm") => Some(from.clone()),
        _ => None,
    });
    found.unwrap()
}

#[test]
fn the_work_dirs_of_one_project_share_its_package_caches() {
    let root = Root::new("caches");
    root.dir("home/.npm");
    let setup = root.setup("/usr/bin");
    let main = root.dir("projects/b-c");
    let worktree = root.dir("data/worktrees/p1/t/w1");
    let other = root.dir("projects/b_c");
    let of = |dir: &Path, project: Option<&str>| {
        let mut profile = profile(dir);
        profile.project = project.map(|p| ProjectId(p.into()));
        npm_cache(&Plan::new(&setup, &profile).unwrap())
    };
    let shared = of(&main, Some("p1"));
    assert_eq!(of(&worktree, Some("p1")), shared);
    assert!(shared.starts_with(&setup.caches));
    assert_ne!(of(&other, Some("p2")), shared);
    // Without a project, each folder has its own, even where their paths
    // differ only in punctuation.
    assert_ne!(of(&main, None), of(&other, None));
    assert_ne!(of(&main, None), shared);
}

#[test]
fn a_git_file_that_points_elsewhere_gets_nothing_writable() {
    let root = Root::new("forged");
    let common = root.dir("projects/app/.git");
    let elsewhere = root.dir("projects/app/.git/worktrees/other");
    let worktree = root.dir("data/worktrees/app/t/w1");
    fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", elsewhere.display()),
    )
    .unwrap();
    // `other` points back at another worktree, not this one.
    fs::write(elsewhere.join("gitdir"), "/somewhere/else/.git\n").unwrap();
    let mut profile = profile(&worktree);
    profile.read.push(common);
    let plan = Plan::new(&root.setup("/usr/bin"), &profile).unwrap();
    assert!(!plan.writable.contains(&elsewhere));
    assert!(plan.mounts.contains(&Mount::Ro(worktree.join(".git"))));
}

#[test]
fn the_home_folder_and_its_ancestors_are_never_a_session_folder() {
    let root = Root::new("home");
    let setup = root.setup("/usr/bin");
    for dir in [setup.home.clone(), root.0.clone(), PathBuf::from("/")] {
        let err = Plan::new(&setup, &profile(&dir)).unwrap_err();
        assert!(
            err.message.contains("holds the home folder"),
            "{}",
            err.message
        );
        let mut read = profile(&root.dir("project"));
        read.read.push(dir);
        assert!(Plan::new(&setup, &read).is_err());
    }
}

#[test]
fn a_program_in_the_home_folder_brings_its_own_folder_only() {
    let root = Root::new("program");
    let setup = root.setup("/usr/bin");
    let plan = Plan::new(&setup, &profile(&root.dir("project"))).unwrap();
    let home = &setup.home;
    let package = home.join(".npm-global/lib/node_modules/claude/cli.js");
    assert_eq!(plan.program_root(&package), package.parent().unwrap());
    for alone in [
        home.join(".local/bin/claude"),
        home.join("claude"),
        home.join("bin/claude"),
    ] {
        assert_eq!(plan.program_root(&alone), alone);
    }
    assert_eq!(
        plan.program_root(Path::new("/usr/bin/claude")),
        Path::new("/usr/bin/claude")
    );
    assert!(OsStr::new("x") != plan.path);
}

/// A build next to the helper in the home folder (`target/debug` of a
/// checkout there): the program's own folder already holds the helper.
#[test]
fn a_program_in_the_helpers_own_folder_runs() {
    let root = Root::new("program-by-helper");
    let mut setup = root.setup("/usr/bin");
    setup.helper = root.file("home/work/ovra/target/debug/ovra");
    let program = root.file("home/work/ovra/target/debug/fake-lsp");
    let project = root.dir("project");
    let plan = Plan::new(&setup, &profile(&project)).unwrap();
    let line = plan.line(
        Path::new("/usr/bin/bwrap"),
        &setup.helper,
        Landlock::Require,
        line(&program, &project),
        &Root::outer_env(),
    );
    let shown = root.show(&line.unwrap());
    assert!(shown.contains("--ro-bind ROOT/home/work/ovra/target/debug\n"));
    assert!(!shown.contains("--ro-bind ROOT/home/work/ovra/target/debug/ovra\n"));
}

#[test]
fn ovra_data_inside_the_project_stays_hidden() {
    let root = Root::new("data-inside");
    let project = root.dir("project");
    root.file("project/data/worktrees/w/CLAUDE.md");
    let mut setup = root.setup("/usr/bin");
    setup.hidden = vec![project.join("data")];
    let plan = Plan::new(&setup, &profile(&project)).unwrap();
    assert_eq!(
        plan.mounts.last(),
        Some(&Mount::Tmpfs(project.join("data")))
    );
    assert!(
        !plan
            .mounts
            .iter()
            .any(|m| matches!(m, Mount::Rw(p) if p.starts_with(project.join("data"))))
    );
}

#[test]
fn a_mount_that_would_cover_an_earlier_one_fails_the_plan() {
    let secret = Mount::Null("/h/.cargo/credentials".into());
    let cover = Mount::Ro("/h/.cargo".into());
    assert!(nothing_lost(&[cover.clone(), secret.clone()]).is_ok());
    assert!(nothing_lost(&[secret.clone(), cover.clone()]).is_err());
    assert!(nothing_lost(&[secret.clone(), cover, secret]).is_ok());
}

#[test]
fn claude_settings_are_never_a_session_folder() {
    let root = Root::new("claude-dir");
    let setup = root.setup("/usr/bin");
    let inside = root.dir("home/.claude/agents");
    assert!(Plan::new(&setup, &profile(&inside)).is_err());
    let mut write = profile(&root.dir("project"));
    write.write.push(root.file("home/.claude.json"));
    assert!(Plan::new(&setup, &write).is_err());
    // Not even for a profile that names no agent state.
    let mut stateless = profile(&inside);
    stateless.agent_state = None;
    assert!(Plan::new(&setup, &stateless).is_err());
    // Nor other agents' settings, whichever provider the session runs.
    for other in ["home/.codex", "home/.gemini/extensions"] {
        assert!(Plan::new(&setup, &profile(&root.dir(other))).is_err());
        let mut write = profile(&root.dir("project"));
        write.write.push(root.dir(other));
        assert!(Plan::new(&setup, &write).is_err(), "{other}");
    }
}

#[test]
fn an_agent_profile_without_agent_state_gets_none_of_claudes() {
    let root = Root::new("stateless");
    let project = root.dir("projects/app");
    root.file("home/.claude/.credentials.json");
    root.file("home/.claude.json");
    let setup = root.setup("/usr/bin");
    let mut profile = profile(&project);
    profile.agent_state = None;
    let plan = Plan::new(&setup, &profile).unwrap();
    let home = root.0.join("home");
    let shows = |path: &Path| {
        plan.mounts.iter().any(|m| match m {
            Mount::Ro(p) | Mount::Rw(p) | Mount::Tmpfs(p) => p.starts_with(path),
            _ => false,
        })
    };
    assert!(!shows(&home.join(".claude")));
    assert!(!shows(&home.join(".claude.json")));
    let mut outer_env = Root::outer_env();
    outer_env.extend(["ANTHROPIC_API_KEY", "OVRA_FAKE_CLAUDE_FIXTURE"].map(OsString::from));
    let line = plan
        .line(
            Path::new("/usr/bin/bwrap"),
            &setup.helper,
            Landlock::Try,
            line(Path::new("/bin/sh"), &project),
            &outer_env,
        )
        .unwrap();
    for name in ["ANTHROPIC_API_KEY", "OVRA_FAKE_CLAUDE_FIXTURE"] {
        assert!(removed(&line.env, &name.into()), "{name}");
    }
    assert!(line.args.iter().all(|a| a != "CLAUDE_CODE_USE_BEDROCK"));
}

#[test]
fn an_accounts_own_folder_comes_back_instead_of_the_default_one() {
    let root = Root::new("account");
    let project = root.dir("projects/app");
    root.file("home/.claude/.credentials.json");
    root.file("home/.claude.json");
    let work = root.dir("home/.claude-work");
    root.file("home/.claude-work/.credentials.json");
    root.file("home/.claude-work/.claude.json");
    root.dir("home/.claude-other");
    // The link the user typed is in the hidden home folder; claude is
    // pointed at the folder itself.
    std::os::unix::fs::symlink(&work, root.0.join("home/work-link")).unwrap();
    let setup = root.setup("/usr/bin");
    let mut profile = profile(&project);
    profile.agent_state = claude(Some(root.0.join("home/work-link")));
    let plan = Plan::new(&setup, &profile).unwrap();
    let home = root.0.join("home");
    let shown = |path: &Path| {
        plan.mounts.iter().any(|m| match m {
            Mount::Ro(p) | Mount::Rw(p) => path.starts_with(p),
            _ => false,
        })
    };
    assert!(plan.mounts.contains(&Mount::Ro(work.clone())));
    assert!(plan.mounts.contains(&Mount::Rw(work.join(".claude.json"))));
    assert!(
        plan.mounts
            .contains(&Mount::Rw(work.join(".credentials.json")))
    );
    assert!(
        plan.mounts
            .contains(&Mount::Tmpfs(work.join("shell-snapshots")))
    );
    assert!(!shown(&home.join(".claude")));
    assert!(!shown(&home.join(".claude.json")));
    assert!(!shown(&home.join(".claude-other")));
    let line = plan
        .line(
            Path::new("/usr/bin/bwrap"),
            &setup.helper,
            Landlock::Try,
            line(Path::new("/bin/sh"), &project),
            &Root::outer_env(),
        )
        .unwrap();
    let set: Vec<_> = line
        .env
        .iter()
        .filter(|(n, _)| n == "CLAUDE_CONFIG_DIR")
        .collect();
    assert_eq!(
        set.last().and_then(|(_, v)| v.as_deref()),
        Some(work.as_os_str())
    );
    assert!(
        line.args
            .windows(2)
            .any(|w| w[0] == "--env" && w[1] == "CLAUDE_CONFIG_DIR")
    );

    // A pod gets no account at all.
    let mut pod = profile.clone();
    pod.kind = ovra_core::services::ProfileKind::Pod;
    let pod = Plan::new(&setup, &pod).unwrap();
    assert!(!pod.mounts.contains(&Mount::Ro(work.clone())));
    assert_eq!(pod.agent_state, None);

    // And the agent cannot write it as a session folder.
    let mut write = profile.clone();
    write.write.push(work.join("agents"));
    fs::create_dir_all(work.join("agents")).unwrap();
    assert!(Plan::new(&setup, &write).is_err());
}

#[test]
fn an_account_folder_must_be_a_claude_folder_right_in_the_home_folder() {
    let root = Root::new("account-bad");
    let project = root.dir("home/projects/app");
    root.dir("home/.ssh");
    root.dir("home/.mozilla");
    root.dir("home/.claude");
    root.dir("home/projects/app/.claude-x");
    root.dir("home/tools/.claude-work");
    root.dir("elsewhere/.claude-work");
    let setup = root.setup("/usr/bin");
    let refused = |dir: PathBuf| {
        let mut profile = profile(&project);
        profile.agent_state = claude(Some(dir));
        Plan::new(&setup, &profile).err().map(|e| e.message)
    };
    let home = root.0.join("home");
    let says = |dir: PathBuf, what: &str| {
        let message = refused(dir).unwrap_or_default();
        assert!(message.contains(what), "{message}");
    };
    says(home.join(".claude-missing"), "does not exist");
    let right_in_home = "right in the home folder";
    for dir in [
        home.clone(),
        home.join(".ssh"),
        home.join(".mozilla"),
        home.join(".claude"),
        home.join("projects"),
        home.join("projects/app/.claude-x"),
        home.join("tools/.claude-work"),
        root.0.join("elsewhere/.claude-work"),
    ] {
        says(dir, right_in_home);
    }
    root.file("home/.claude-file");
    says(home.join(".claude-file"), "not a folder");
}

#[test]
fn no_session_writes_any_accounts_claude_state() {
    let root = Root::new("account-write");
    let setup = root.setup("/usr/bin");
    let work = root.dir("home/.claude-work");
    let inside_default = root.dir("home/.claude/agents");
    let project = root.dir("projects/app");
    // With an account set, the default account's folder stays protected.
    let mut account = profile(&inside_default);
    account.agent_state = claude(Some(work.clone()));
    assert!(Plan::new(&setup, &account).is_err());
    let mut state = profile(&project);
    state.agent_state = claude(Some(work.clone()));
    state.write.push(root.file("home/.claude.json"));
    assert!(Plan::new(&setup, &state).is_err());
    // And a default-account session cannot work in an account's folder.
    assert!(Plan::new(&setup, &profile(&work)).is_err());
    let mut write = profile(&project);
    write.write.push(root.dir("home/.claude-work/agents"));
    assert!(Plan::new(&setup, &write).is_err());
}

#[test]
fn path_entries_inside_the_project_and_nested_repositories() {
    let root = Root::new("nested");
    let project = root.dir("home/project");
    root.dir("home/project/node_modules/.bin");
    root.file("home/project/vendor/lib/.git/config");
    let path = format!("{}/node_modules/.bin:/usr/bin", project.display());
    let mut setup = root.setup("/usr/bin");
    setup.path = path.into();
    let plan = Plan::new(&setup, &profile(&project)).unwrap();
    let line = plan.line(
        Path::new("/usr/bin/bwrap"),
        &setup.helper,
        Landlock::Try,
        line(Path::new("/bin/sh"), &project),
        &Root::outer_env(),
    );
    assert!(line.is_ok(), "{:?}", line.err());
    assert!(
        plan.mounts
            .contains(&Mount::Ro(project.join("vendor/lib/.git")))
    );
}

#[test]
fn a_missing_ovra_folder_is_made_and_kept_read_only() {
    let root = Root::new("ovra-dir");
    let project = root.dir("project");
    let mut setup = root.setup("/usr/bin");
    setup.protected = Protected {
        work_dir: &[".claude", ".ovra"],
    };
    let plan = Plan::new(&setup, &profile(&project)).unwrap();
    assert!(project.join(".ovra").is_dir());
    assert!(plan.mounts.contains(&Mount::Ro(project.join(".ovra"))));
    assert!(plan.temporary.contains(&project.join(".ovra")));
}

#[test]
fn missing_protected_files_are_made_empty_and_kept_read_only_for_the_session() {
    let root = Root::new("files-made");
    let project = root.dir("project");
    root.file("project/AGENTS.md");
    std::fs::write(project.join(".envrc"), "use flake").unwrap();
    let mut setup = root.setup("/usr/bin");
    setup.protected = Protected {
        work_dir: &[
            ".mcp.json",
            "**/CLAUDE.md",
            "**/AGENTS.md",
            ".envrc",
            ".worktreeinclude",
        ],
    };
    let plan = Plan::new(&setup, &profile(&project)).unwrap();
    for name in [".mcp.json", "CLAUDE.md", ".worktreeinclude"] {
        let path = project.join(name);
        assert_eq!(std::fs::read(&path).unwrap(), b"", "{name}");
        assert!(plan.mounts.contains(&Mount::Ro(path)), "{name}");
    }
    // Only what was missing and is still empty goes again; what the list
    // does not name is never made.
    assert_eq!(
        plan.temporary,
        [
            project.join("CLAUDE.md"),
            project.join(".mcp.json"),
            project.join(".worktreeinclude")
        ]
    );
    assert!(!project.join("CLAUDE.local.md").exists());
    assert!(plan.mounts.contains(&Mount::Ro(project.join(".envrc"))));
}

#[test]
fn hidden_folders_inside_each_other_are_hidden_once() {
    let root = Root::new("hidden-nested");
    let project = root.dir("project");
    root.dir("data");
    let mut setup = root.setup("/usr/bin");
    // Ovra's data folder in the temporary folder, which is hidden too.
    setup.hidden = vec![root.0.join("data"), root.0.clone(), PathBuf::from("/tmp")];
    let plan = Plan::new(&setup, &profile(&project)).unwrap();
    let line = plan.line(
        Path::new("/usr/bin/bwrap"),
        &setup.helper,
        Landlock::Try,
        line(Path::new("/bin/sh"), &project),
        &Root::outer_env(),
    );
    assert!(line.is_ok(), "{:?}", line.err());
}

fn proxied(root: &Root, work_dir: &Path) -> SandboxProfile {
    let mut profile = profile(work_dir);
    profile.network = ProfileNetwork::Proxy {
        address: root.file("data/proxy/s.sock").display().to_string(),
        ca_bundle: None,
    };
    profile
}

#[test]
fn a_server_pod_listens_in_its_own_network_and_writes_its_pod_folder() {
    let root = Root::new("pod");
    let project = root.dir("projects/app");
    let pod = root.dir("data/pods/p1");
    let setup = root.setup("/usr/bin");
    // Claude's login and state: none of it is shown to a pod.
    root.file("home/.claude/.credentials.json");
    root.file("home/.claude/history.jsonl");
    root.file("home/.claude.json");
    let mut profile = proxied(&root, &project);
    profile.write.push(pod.clone());
    profile.expose = vec![5173, 8080];
    profile.kind = ProfileKind::Pod;
    let plan = Plan::new(&setup, &profile).unwrap();
    let mut outer_env = Root::outer_env();
    outer_env
        .extend(["ANTHROPIC_API_KEY", "CLAUDE_CODE_USE_BEDROCK", "AWS_REGION"].map(OsString::from));
    let line = plan.line(
        Path::new("/usr/bin/bwrap"),
        &setup.helper,
        Landlock::Require,
        CommandLine {
            program: setup.helper.clone(),
            args: vec!["--ovra-pod".into(), pod.into()],
            env: Vec::new(),
            cwd: project.clone(),
        },
        &outer_env,
    );
    insta::assert_snapshot!(root.show(&line.unwrap()));
    // `claude`'s folder is neither made nor bound for a pod.
    assert!(!root.0.join("home/.claude/projects").exists());
    // The protected folders it made stay: an agent may have bound them since.
    // The empty files it made go: they would show in git status.
    assert!(project.join(".claude").is_dir());
    assert_eq!(plan.temporary, [project.join("CLAUDE.md")]);
    assert!(plan.record.is_some());
}

#[test]
fn the_agent_browser_gets_its_profile_folder_and_none_of_claudes_state() {
    let root = Root::new("browser");
    let folder = root.dir("tmp/ovra-browser-1");
    let setup = root.setup("/usr/bin");
    // Claude's login and state, and the home folder's tools: none of it is
    // shown to a browser that runs web pages without its own sandbox.
    root.file("home/.claude/.credentials.json");
    root.file("home/.claude/history.jsonl");
    root.file("home/.claude.json");
    root.file("home/.gitconfig");
    root.dir("home/.cargo/bin");
    let mut profile = proxied(&root, &folder);
    profile.kind = ProfileKind::Browser;
    let plan = Plan::new(&setup, &profile).unwrap();
    let mut outer_env = Root::outer_env();
    outer_env
        .extend(["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "AWS_REGION"].map(OsString::from));
    let line = plan.line(
        Path::new("/usr/bin/bwrap"),
        &setup.helper,
        Landlock::Require,
        line(Path::new("/bin/sh"), &folder),
        &outer_env,
    );
    let shown = root.show(&line.unwrap());
    insta::assert_snapshot!(shown);
    // The profile folder's own protected `.claude` is an empty placeholder;
    // `= None` lines are variables the line removes.
    let kept: String = shown.lines().filter(|l| !l.ends_with("= None")).collect();
    for hidden in [
        "home/.claude",
        ".claude.json",
        ".gitconfig",
        "home/.cargo",
        "ANTHROPIC",
        "CLAUDE_CODE_OAUTH",
        "AWS_",
    ] {
        assert!(
            !kept.contains(hidden),
            "{hidden} reaches the browser:\n{shown}"
        );
    }
    assert!(!root.0.join("home/.claude/projects").exists());
}

#[test]
fn an_agent_forwards_its_servers_ports_through_the_proxy() {
    let root = Root::new("forward");
    let project = root.dir("projects/app");
    let setup = root.setup("/usr/bin");
    let mut profile = proxied(&root, &project);
    profile.forward = vec![8080, 5173, 8080];
    let plan = Plan::new(&setup, &profile).unwrap();
    let line = plan.line(
        Path::new("/usr/bin/bwrap"),
        &setup.helper,
        Landlock::Require,
        line(Path::new("/bin/sh"), &project),
        &Root::outer_env(),
    );
    insta::assert_snapshot!(root.show(&line.unwrap()));
}

#[test]
fn ports_a_sandbox_cannot_keep_to_itself_are_refused() {
    let root = Root::new("ports");
    let project = root.dir("project");
    let setup = root.setup("/usr/bin");
    let refused = |profile: &SandboxProfile| Plan::new(&setup, profile).unwrap_err().message;
    let mut open = profile(&project);
    open.forward = vec![5173];
    assert_eq!(
        refused(&open),
        "forwarding ports to servers needs the session's netproxy"
    );
    open.network = ProfileNetwork::Blocked;
    assert!(refused(&open).contains("needs the session's netproxy"));
    let mut pod = profile(&project);
    pod.expose = vec![5173];
    pod.network = ProfileNetwork::Blocked;
    assert_eq!(refused(&pod), "only a server pod exposes ports");
    pod.network = ProfileNetwork::Open;
    pod.kind = ProfileKind::Pod;
    assert!(refused(&pod).contains("needs a network of its own"));
    pod.network = ProfileNetwork::Blocked;
    assert!(Plan::new(&setup, &pod).is_ok());
    let mut proxy = proxied(&root, &project);
    proxy.forward = vec![PROXY_PORT];
    assert!(refused(&proxy).contains("port 3128"));
    proxy.forward = vec![0];
    assert!(refused(&proxy).contains("port 0"));
    proxy.forward = vec![5173];
    proxy.expose = vec![5173];
    proxy.kind = ProfileKind::Pod;
    assert!(refused(&proxy).contains("both exposed and forwarded"));
}

mod server;

#[test]
fn a_research_session_sees_the_project_read_only_and_makes_nothing_in_it() {
    let root = Root::new("research");
    let project = root.dir("project");
    root.file("project/src/main.rs");
    root.file("project/data/worktrees/w/CLAUDE.md");
    let mut setup = root.setup("/usr/bin");
    setup.protected = Protected {
        work_dir: &[".claude", ".ovra", ".mcp.json"],
    };
    setup.hidden = vec![project.join("data")];
    let mut profile = profile(&project);
    profile.work_dir_read_only = true;
    let plan = Plan::new(&setup, &profile).unwrap();
    assert!(plan.mounts.contains(&Mount::Ro(project.clone())));
    assert!(!plan.writable.iter().any(|w| w.starts_with(&project)));
    assert!(!plan.mounts.iter().any(
        |m| matches!(m, Mount::Rw(p) | Mount::Private { at: p, .. } if p.starts_with(&project))
    ));
    // Ovra's data folder inside it stays hidden.
    assert!(plan.mounts.contains(&Mount::Tmpfs(project.join("data"))));
    // No protected folder or file was made for it.
    assert!(!project.join(".claude").exists());
    assert!(!project.join(".ovra").exists());
    assert!(!project.join(".mcp.json").exists());
    assert!(plan.temporary.is_empty());
}

#[test]
fn every_folder_made_is_on_the_protected_list() {
    for name in super::FOLDERS_MADE {
        assert!(
            ovra_core::protected::PROTECTED_IN_WORK_DIR.contains(&name),
            "{name} is made only when it is protected"
        );
    }
}

#[test]
fn git_status_leaves_out_exactly_the_files_made() {
    // The git panel hides these while they are empty; one made but not
    // listed would show up for "Commit all".
    assert_eq!(super::FILES_MADE, ovra_core::protected::PLACEHOLDERS);
}

fn config_sock(root: &Root) -> PathBuf {
    root.0.join("home/.layered/agent.sock")
}

/// An agent that writes lock files next to its state, as Gemini does.
const LAYERED: ProviderLists = ProviderLists {
    name: "Layered",
    home_dir: ".layered",
    state_file: ".layered.json",
    config_var: "LAYERED_HOME",
    private: &["tmp", "projects.json", "state.sqlite", "state.sqlite-wal"],
    in_place: &["tmp"],
    files: &["oauth_creds.json"],
    layered: true,
    files_made: &[],
    projects: "tmp",
    project_memory: "memory",
    env: &[],
    windows_env: &[],
    fixture_var: "OVRA_FAKE_LAYERED",
};

#[test]
fn a_layered_config_folder_is_private_with_its_entries_bound_back() {
    let root = Root::new("layered");
    let project = root.dir("projects/app");
    let config = root.dir("home/.layered");
    root.file("home/.layered/settings.json");
    root.file("home/.layered/oauth_creds.json");
    root.file("home/.layered/projects.json");
    root.file("home/.layered/credentials.json");
    // A database whose write-ahead log is gone stays out as a whole.
    root.file("home/.layered/state.sqlite");
    std::os::unix::net::UnixListener::bind(config_sock(&root)).unwrap();
    root.file("home/.layered/tmp/old-chat.json");
    root.dir("home/.layered/extensions");
    std::os::unix::fs::symlink(root.dir("home/.ssh"), config.join("keys")).unwrap();
    let setup = root.setup("/usr/bin");
    let mut profile = profile(&project);
    profile.agent_state = Some(AgentState {
        lists: &LAYERED,
        config_dir: None,
    });
    let plan = Plan::new(&setup, &profile).unwrap();
    let under: Vec<&Mount> = plan
        .mounts
        .iter()
        .filter(|m| match m {
            Mount::Ro(p) | Mount::Rw(p) | Mount::Tmpfs(p) | Mount::Null(p) => {
                p.starts_with(&config) && !p.starts_with(config.join("tmp"))
            }
            Mount::Private { .. } => false,
        })
        .collect();
    assert_eq!(
        under,
        [
            &Mount::Tmpfs(config.clone()),
            &Mount::Ro(config.join("extensions")),
            &Mount::Ro(config.join("settings.json")),
            &Mount::Rw(config.join("oauth_creds.json")),
        ]
    );
    assert!(plan.writable.contains(&config));
    // The chats of other folders stay out; this folder's own come back.
    assert!(!plan.mounts.contains(&Mount::Tmpfs(config.join("tmp"))));
    assert!(plan.mounts.iter().all(|m| match m {
        Mount::Ro(p) | Mount::Rw(p) => !p.starts_with(config.join("tmp/old-chat.json")),
        _ => true,
    }));
    plan.line(
        Path::new("/usr/bin/bwrap"),
        &setup.helper,
        Landlock::Try,
        line(Path::new("/bin/sh"), &project),
        &Root::outer_env(),
    )
    .unwrap();
}
