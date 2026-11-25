#![allow(clippy::unwrap_used)]

use std::ffi::OsStr;

use super::*;

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
            caches: self.0.join("data/sandbox-caches"),
            env: [
                "HOME",
                "LANG",
                "GITHUB_TOKEN",
                "SSH_AUTH_SOCK",
                "CLAUDE_CODE_OAUTH_TOKEN",
                "CLAUDECODE",
            ]
            .map(Into::into)
            .to_vec(),
            protected: PROTECTED,
        }
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
        out
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn profile(work_dir: &Path) -> SandboxProfile {
    SandboxProfile {
        work_dir: work_dir.to_path_buf(),
        read: Vec::new(),
        write: Vec::new(),
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
    );
    let line = line.unwrap();
    assert_eq!(line.program, Path::new("/usr/bin/bwrap"));
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
    let setup = root.setup("/usr/bin");
    let mut profile = profile(&worktree);
    profile.read.push(common);
    profile.network = ProfileNetwork::Proxy {
        address: socket.display().to_string(),
        ca_bundle: None,
    };
    let plan = Plan::new(&setup, &profile).unwrap();
    let line = plan.line(
        Path::new("/usr/bin/bwrap"),
        &setup.helper,
        Landlock::Try,
        line(Path::new("/bin/sh"), &worktree),
    );
    insta::assert_snapshot!(root.show(&line.unwrap()));
    // Its git folder stays read-only with the rest of the repository's.
    assert!(!own.join("config.worktree").exists());
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
    );
    assert!(line.is_ok(), "{:?}", line.err());
    assert!(
        plan.mounts
            .contains(&Mount::Ro(project.join("vendor/lib/.git")))
    );
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
    );
    assert!(line.is_ok(), "{:?}", line.err());
}
