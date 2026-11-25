//! The real sandbox around shell scripts that play a misbehaving agent.
//! Linux only; without a working bubblewrap the tests say so and pass,
//! except in CI or where bubblewrap is installed: there the sandbox must work.

#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use ovra_core::services::{CommandLine, ProfileNetwork, Sandbox, SandboxProfile, SandboxStatus};
use ovra_sandbox_linux::{Bubblewrap, HELPER_FLAG, Protected, Setup};

const HELPER: &str = env!("CARGO_BIN_EXE_ovra-sandbox-helper");

const PROTECTED: Protected = Protected {
    work_dir: &[".claude", "**/CLAUDE.md", ".envrc"],
};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("ovra-sandbox-linux-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(fs::canonicalize(dir).unwrap())
    }

    fn file(&self, path: &str, text: &str) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
        ])
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The sandbox with `home` as the home folder, or `None` (and why) where
/// bubblewrap cannot run.
fn sandbox(home: &Path, hidden: Vec<PathBuf>) -> Option<Bubblewrap> {
    sandbox_on(home, hidden, "/usr/local/bin:/usr/bin:/bin")
}

fn sandbox_on(home: &Path, hidden: Vec<PathBuf>, path: &str) -> Option<Bubblewrap> {
    let sandbox = Bubblewrap::detect(Setup {
        helper: HELPER.into(),
        home: home.to_path_buf(),
        path: path.into(),
        toolchains: vec![home.join(".cargo")],
        hidden,
        protected: PROTECTED,
    });
    match sandbox.status() {
        SandboxStatus::Available { .. } => Some(sandbox),
        SandboxStatus::Unavailable { reason } => {
            assert!(
                std::env::var_os("CI").is_none() && !Path::new("/usr/bin/bwrap").exists(),
                "no sandbox where bubblewrap is installed: {reason}"
            );
            eprintln!("skipped: {reason}");
            None
        }
    }
}

fn profile(work_dir: &Path, network: ProfileNetwork) -> SandboxProfile {
    SandboxProfile {
        work_dir: work_dir.to_path_buf(),
        read: Vec::new(),
        write: Vec::new(),
        network,
    }
}

/// Runs `script` with `sh` in `profile`; returns what it printed.
fn run(sandbox: &Bubblewrap, profile: &SandboxProfile, script: &str) -> String {
    let line = CommandLine {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        env: Vec::new(),
        cwd: profile.work_dir.clone(),
    };
    let line = sandbox.wrap(profile, line).unwrap();
    let out = spawn(&line);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn spawn(line: &CommandLine) -> Output {
    let mut command = Command::new(&line.program);
    command.args(&line.args).current_dir(&line.cwd);
    for (name, value) in &line.env {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    command.stdin(Stdio::null()).output().unwrap()
}

/// Each line of `checks` is `name: command`; prints `name: yes` or `name: no`.
fn script(checks: &str) -> String {
    checks
        .lines()
        .filter_map(|l| l.trim().split_once(": "))
        .map(|(name, command)| format!("if ({command}) >/dev/null 2>&1; then echo '{name}: yes'; else echo '{name}: no'; fi\n"))
        .collect()
}

#[test]
fn a_direct_session_writes_its_project_and_nothing_else() {
    let root = Dir::new("direct");
    let home = root.0.join("home");
    let project = root.0.join("project");
    fs::create_dir_all(&project).unwrap();
    root.file("home/.ssh/id_ed25519", "secret");
    root.file("home/.claude/settings.json", "{}");
    root.file("home/.cargo/credentials.toml", "token");
    root.file("home/.cargo/config.toml", "");
    root.file("home/.config/git/config", "");
    root.file(
        "home/.config/git/credentials",
        "https://u:token@example.com",
    );
    root.file("home/.docker/config.json", "token");
    root.file("home/.docker/bin/docker", "");
    root.file("project/.claude/settings.json", "{}");
    root.file("project/src/CLAUDE.md", "rules");
    root.file("project/README.md", "hello");
    git(&project, &["init", "-q"]);
    // A submodule that is not checked out, and config included from the tree.
    let blob = "0123456789012345678901234567890123456789";
    git(
        &project,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{blob},sub"),
        ],
    );
    fs::create_dir_all(project.join("sub")).unwrap();
    // One whose folder is gone, and one inside a checked-out submodule.
    git(
        &project,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{blob},deep/gone"),
        ],
    );
    let module = project.join("module");
    fs::create_dir_all(&module).unwrap();
    git(&module, &["init", "-q"]);
    git(
        &module,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{blob},inner"),
        ],
    );
    git(
        &project,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{blob},module"),
        ],
    );
    root.file("project/team.gitconfig", "");
    git(&project, &["config", "include.path", "../team.gitconfig"]);
    git(
        &project,
        &["config", "--add", "include.path", "../local.gitconfig"],
    );
    let data = root.0.join("data");
    root.file("data/store.db", "projects");
    let path = format!("{}/.docker/bin:/usr/bin:/bin", home.display());
    let Some(sandbox) = sandbox_on(&home, vec![data.clone()], &path) else {
        return;
    };
    let key: String = project
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let checks = format!(
        "write the project: echo x > new.txt
        read the project: cat README.md
        read ssh keys: cat {home}/.ssh/id_ed25519
        read cargo credentials: grep token {home}/.cargo/credentials.toml
        read cargo config: cat {home}/.cargo/config.toml
        read the data folder: cat {data}/store.db
        read claude settings: cat {home}/.claude/settings.json
        write claude settings: echo x > {home}/.claude/settings.json
        write this project transcripts: echo x > {home}/.claude/projects/{key}/s.jsonl
        write another project transcripts: mkdir -p {home}/.claude/projects/other
        write a shell snapshot: echo x > {home}/.claude/shell-snapshots/s.sh
        read git config: cat {home}/.config/git/config
        read git credentials: grep token {home}/.config/git/credentials
        read docker credentials: grep token {home}/.docker/config.json
        write a git hook: echo x > .git/hooks/pre-commit
        write git config: echo x >> .git/config
        plant a git commondir: echo evil > .git/commondir
        stage a change: git add README.md
        make a repository in a submodule folder: git init -q sub
        make a repository where a submodule folder is gone: mkdir -p deep/gone/.git
        make a repository in a nested submodule folder: git init -q module/inner
        write included git config: echo x >> team.gitconfig
        make missing included git config: echo x >> local.gitconfig
        write this project memory: echo x > {home}/.claude/projects/{key}/memory/m.md
        write project agent settings: echo x >> .claude/settings.json
        write a nested CLAUDE.md: echo x >> src/CLAUDE.md
        move the git folder away: mv .git .git-old
        move a CLAUDE.md folder away: mv src src-old
        write the private tmp: echo x > /tmp/x
        start a user namespace: unshare -U true",
        home = home.display(),
        data = data.display(),
    );
    let out = run(
        &sandbox,
        &profile(&project, ProfileNetwork::Open),
        &script(&checks),
    );
    insta::assert_snapshot!(out);
    assert_eq!(fs::read_to_string(project.join("new.txt")).unwrap(), "x\n");
    assert!(!project.join(".git/hooks/pre-commit").exists());
    // The shell snapshot went to a private folder, not the user's.
    assert!(!home.join(".claude/shell-snapshots/s.sh").exists());
}

#[test]
fn a_protected_name_that_is_a_link_is_refused() {
    let root = Dir::new("link");
    let home = root.0.join("home");
    fs::create_dir_all(&home).unwrap();
    let project = root.0.join("project");
    root.file("project/AGENTS.md", "rules");
    std::os::unix::fs::symlink("AGENTS.md", project.join("CLAUDE.md")).unwrap();
    let Some(sandbox) = sandbox(&home, Vec::new()) else {
        return;
    };
    let line = CommandLine {
        program: "/bin/true".into(),
        args: Vec::new(),
        env: Vec::new(),
        cwd: project.clone(),
    };
    let err = sandbox
        .wrap(&profile(&project, ProfileNetwork::Blocked), line)
        .unwrap_err();
    assert!(
        err.message.contains("CLAUDE.md is a link"),
        "{}",
        err.message
    );
}

#[test]
fn a_worker_reads_its_git_folder_and_writes_only_its_files() {
    let root = Dir::new("worker");
    let home = root.0.join("home");
    fs::create_dir_all(&home).unwrap();
    let main = root.0.join("main");
    fs::create_dir_all(&main).unwrap();
    root.file("main/a.txt", "a");
    git(&main, &["init", "-q"]);
    git(&main, &["add", "a.txt"]);
    git(&main, &["commit", "-qm", "a"]);
    let worktree = root.0.join("data/worktrees/w1");
    git(
        &main,
        &["worktree", "add", "-q", worktree.to_str().unwrap()],
    );
    let Some(sandbox) = sandbox(&home, vec![root.0.join("data")]) else {
        return;
    };
    let mut profile = profile(&worktree, ProfileNetwork::Blocked);
    profile.read.push(main.join(".git"));
    let own = main.join(".git/worktrees/w1");
    let checks = format!(
        "git status: git status --short
        stage a change: echo b > a.txt && git add a.txt
        write its own git folder: echo x > {own}/scratch
        plant a submodule in its index: git update-index --add --cacheinfo 160000,0123456789012345678901234567890123456789,sub
        write its commondir: echo x > {own}/commondir
        write config.worktree: echo x > {own}/config.worktree
        write its .git file: echo x >> .git
        commit: git commit -qm b --allow-empty
        change the main checkout: echo x > {main}/a.txt
        write the main objects: touch {main}/.git/objects/x",
        own = own.display(),
        main = main.display(),
    );
    let out = run(&sandbox, &profile, &script(&checks));
    insta::assert_snapshot!(out);
    // The main checkout is not in the sandbox: the write went to its
    // private `/tmp`.
    assert_eq!(fs::read_to_string(main.join("a.txt")).unwrap(), "a");
}

#[test]
fn a_forged_git_file_gets_no_write_access() {
    let root = Dir::new("forged");
    let home = root.0.join("home");
    fs::create_dir_all(&home).unwrap();
    let elsewhere = root.0.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let work = root.0.join("work");
    root.file("work/.git", &format!("gitdir: {}\n", elsewhere.display()));
    let Some(sandbox) = sandbox(&home, Vec::new()) else {
        return;
    };
    let mut profile = profile(&work, ProfileNetwork::Blocked);
    profile.read.push(elsewhere.clone());
    let checks = format!("write elsewhere: echo x > {}/x", elsewhere.display());
    assert_eq!(
        run(&sandbox, &profile, &script(&checks)),
        "write elsewhere: no\n"
    );
}

#[test]
fn a_blocked_network_reaches_nothing_and_a_proxy_only_its_socket() {
    let root = Dir::new("network");
    let home = root.0.join("home");
    let work = root.0.join("work");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&work).unwrap();
    let Some(sandbox) = sandbox(&home, Vec::new()) else {
        return;
    };
    let outside = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = outside.local_addr().unwrap().port();
    let reach = format!("reach the host: bash -c 'echo > /dev/tcp/127.0.0.1/{port}'");
    let blocked = profile(&work, ProfileNetwork::Blocked);
    assert_eq!(
        run(&sandbox, &blocked, &script(&reach)),
        "reach the host: no\n"
    );

    let socket = root.0.join("proxy.sock");
    let proxy = UnixListener::bind(&socket).unwrap();
    let echo = std::thread::spawn(move || {
        let (mut stream, _) = proxy.accept().unwrap();
        let mut buf = [0; 5];
        stream.read_exact(&mut buf).unwrap();
        stream.write_all(&buf).unwrap();
    });
    let proxied = profile(
        &work,
        ProfileNetwork::Proxy {
            address: socket.display().to_string(),
            ca_bundle: None,
        },
    );
    let out = run(
        &sandbox,
        &proxied,
        &format!(
            "echo \"$HTTPS_PROXY\"\n{}bash -c 'exec 3<>/dev/tcp/127.0.0.1/3128; printf \"ping\\n\" >&3; head -c 5 <&3'",
            script(&reach)
        ),
    );
    echo.join().unwrap();
    assert_eq!(out, "http://127.0.0.1:3128\nreach the host: no\nping\n");
}

#[test]
fn landlock_and_seccomp_hold_without_bubblewrap_too() {
    let root = Dir::new("helper");
    let allowed = root.0.join("allowed");
    let other = root.0.join("other");
    fs::create_dir_all(&allowed).unwrap();
    fs::create_dir_all(&other).unwrap();
    let checks = format!(
        "write the allowed folder: echo x > {}/x
        write another folder: echo x > {}/x
        start a user namespace: unshare -U true",
        allowed.display(),
        other.display()
    );
    let out = Command::new(HELPER)
        .args([
            HELPER_FLAG,
            "--landlock",
            "report",
            "--rw",
            "/dev/null",
            "--rw",
        ])
        .arg(&allowed)
        .args(["--", "/bin/sh", "-c", &script(&checks)])
        .output()
        .unwrap();
    let out = String::from_utf8(out.stdout).unwrap();
    let Some(out) = out.strip_prefix("ovra-sandbox: landlock enforced\n") else {
        assert!(std::env::var_os("CI").is_none(), "no Landlock in CI: {out}");
        eprintln!("skipped: this kernel has no Landlock");
        return;
    };
    insta::assert_snapshot!(out);
}

#[test]
fn starting_a_session_runs_nothing_a_nested_repository_names() {
    let root = Dir::new("fsmonitor");
    let home = root.0.join("home");
    fs::create_dir_all(&home).unwrap();
    let project = root.0.join("project");
    let nested = project.join("vendor/x");
    fs::create_dir_all(&nested).unwrap();
    git(&project, &["init", "-q"]);
    git(&nested, &["init", "-q"]);
    let marker = root.0.join("ran");
    git(
        &nested,
        &[
            "config",
            "core.fsmonitor",
            &format!("touch {}", marker.display()),
        ],
    );
    let Some(sandbox) = sandbox(&home, Vec::new()) else {
        return;
    };
    run(
        &sandbox,
        &profile(&project, ProfileNetwork::Blocked),
        "true",
    );
    assert!(!marker.exists());
}
