#![allow(clippy::unwrap_used)]

use super::*;

const PROTECTED: &[&str] = &[".claude", ".vscode", "**/CLAUDE.md"];

struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ovra-win-plan-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(real(&dir).unwrap())
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

    fn setup(&self, path: &[&str]) -> Setup {
        let path: Vec<PathBuf> = path.iter().map(|p| self.0.join(p)).collect();
        Setup {
            helper: self.file("bin/ovra"),
            home: self.dir("home"),
            path: std::env::join_paths(path).unwrap(),
            data_dir: self.dir("data"),
            caches: self.0.join("data/sandbox-caches"),
            temp: self.0.join("data/sandbox-temp"),
            protected: PROTECTED,
        }
    }

    /// The helper's line, one argument per line (a flag with its value),
    /// then the variables it sets or removes, with this root as `ROOT`.
    fn show(&self, line: &CommandLine) -> String {
        let root = self.0.display().to_string();
        let key = project_key(&self.0);
        let app = real(&self.0.join("projects").join("app")).unwrap();
        let (dir, sid) = (project_dir(&app), project_sid(&app));
        let clean = |s: &OsStr| {
            s.to_string_lossy()
                .replace(&sid, "SID")
                .replace(&dir, "PROJECT")
                .replace(&key, "-ROOT")
                .replace(&root, "ROOT")
                .replace(':', ";")
                .replace('\\', "/")
        };
        let mut out = String::new();
        let mut args = line.args.iter();
        while let Some(arg) = args.next() {
            out.push_str(&clean(arg));
            if arg.to_string_lossy().starts_with("--") && arg != "--" && arg != HELPER_FLAG {
                out.push(' ');
                out.push_str(&clean(args.next().unwrap()));
            }
            out.push('\n');
        }
        out.push_str("environment:\n");
        for (name, value) in &line.env {
            let value = value.as_deref().map_or("(removed)".into(), clean);
            out.push_str(&format!("  {} = {value}\n", name.to_string_lossy()));
        }
        out
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn line(program: PathBuf, cwd: &Path) -> CommandLine {
    CommandLine {
        program,
        args: vec!["-p".into()],
        env: vec![("CLAUDECODE".into(), None)],
        cwd: cwd.to_path_buf(),
    }
}

fn outer_env() -> Vec<OsString> {
    [
        "Path",
        "USERPROFILE",
        "LANG",
        "GITHUB_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDECODE",
    ]
    .map(Into::into)
    .to_vec()
}

#[test]
fn a_direct_session_behind_a_proxy() {
    let root = Root::new("direct");
    let work = root.dir("projects/app");
    root.dir("projects/app/.git/hooks");
    root.file("projects/app/docs/CLAUDE.md");
    root.file("home/.claude.json");
    root.file("home/.claude/.credentials.json");
    root.file("home/.gitconfig");
    root.file("home/.ssh/id_ed25519");
    let claude = root.file("home/.local/bin/claude.exe");
    root.dir("home/.cargo/bin");
    root.dir("home/.ssh/bin");
    let bundle = root.file("data/net/ca.pem");
    let setup = root.setup(&["home/.cargo/bin", "home/.ssh/bin", "home", "sys/bin"]);
    root.dir("sys/bin");
    let profile = SandboxProfile {
        kind: ovra_core::services::ProfileKind::Agent,
        claude_dir: None,
        work_dir: work.clone(),
        read: Vec::new(),
        write: Vec::new(),
        expose: Vec::new(),
        forward: Vec::new(),
        network: ProfileNetwork::Proxy {
            address: "127.0.0.1:49152".into(),
            ca_bundle: Some(bundle),
        },
    };
    let plan = Plan::new(&setup, &profile, &claude).unwrap();
    let line = plan.line(&setup, line(claude, &work), &outer_env());
    insta::assert_snapshot!(root.show(&line));
    // Missing protected folders are made, so the agent cannot make them.
    assert!(work.join(".claude").is_dir() && work.join(".vscode").is_dir());
}

#[test]
fn a_worker_reads_its_git_folder() {
    let root = Root::new("worker");
    let main_git = root.dir("projects/app/.git");
    let work = root.dir("data/worktrees/w1");
    root.file("data/worktrees/w1/.git");
    let claude = root.file("sys/bin/claude.exe");
    let setup = root.setup(&["sys/bin"]);
    let profile = SandboxProfile {
        kind: ovra_core::services::ProfileKind::Agent,
        claude_dir: None,
        work_dir: work.clone(),
        read: vec![main_git],
        write: Vec::new(),
        expose: Vec::new(),
        forward: Vec::new(),
        network: ProfileNetwork::Blocked,
    };
    let plan = Plan::new(&setup, &profile, &claude).unwrap();
    assert!(plan.grants.contains(&(work.clone(), Right::Modify)));
    assert!(plan.denied.contains(&work.join(".git")));
    assert!(plan.grants.iter().all(|(p, _)| *p != root.0.join("data")));
}

#[test]
fn folders_holding_the_home_folder_or_ovras_data_are_refused() {
    let root = Root::new("refused");
    let claude = root.file("sys/bin/claude.exe");
    let setup = root.setup(&["sys/bin"]);
    for work in [root.0.clone(), root.dir("home"), root.dir("home/.claude/x")] {
        let profile = SandboxProfile {
            kind: ovra_core::services::ProfileKind::Agent,
            claude_dir: None,
            work_dir: work.clone(),
            read: Vec::new(),
            write: Vec::new(),
            expose: Vec::new(),
            forward: Vec::new(),
            network: ProfileNetwork::Blocked,
        };
        assert!(
            Plan::new(&setup, &profile, &claude).is_err(),
            "{}",
            work.display()
        );
    }
}

#[cfg(unix)]
#[test]
fn a_protected_path_that_is_a_link_refuses_the_session() {
    let root = Root::new("link");
    let work = root.dir("projects/app");
    let elsewhere = root.dir("elsewhere");
    std::os::unix::fs::symlink(&elsewhere, work.join(".claude")).unwrap();
    let claude = root.file("sys/bin/claude.exe");
    let setup = root.setup(&["sys/bin"]);
    let profile = SandboxProfile {
        kind: ovra_core::services::ProfileKind::Agent,
        claude_dir: None,
        work_dir: work,
        read: Vec::new(),
        write: Vec::new(),
        expose: Vec::new(),
        forward: Vec::new(),
        network: ProfileNetwork::Blocked,
    };
    let error = Plan::new(&setup, &profile, &claude).unwrap_err();
    assert!(error.message.contains("is a link"), "{}", error.message);
}

#[test]
fn variable_names_match_without_case() {
    let root = Root::new("case");
    let work = root.dir("projects/app");
    let claude = root.file("sys/bin/claude.exe");
    let setup = root.setup(&["sys/bin"]);
    let profile = SandboxProfile {
        kind: ovra_core::services::ProfileKind::Agent,
        claude_dir: None,
        work_dir: work.clone(),
        read: Vec::new(),
        write: Vec::new(),
        expose: Vec::new(),
        forward: Vec::new(),
        network: ProfileNetwork::Blocked,
    };
    let plan = Plan::new(&setup, &profile, &claude).unwrap();
    let outer = ["Path", "userprofile", "github_token"].map(OsString::from);
    let line = plan.line(&setup, line(claude, &work), &outer);
    let removed: Vec<_> = line
        .env
        .iter()
        .filter(|(_, v)| v.is_none())
        .map(|(n, _)| n.clone())
        .collect();
    assert!(removed.contains(&"github_token".into()));
    assert!(!removed.contains(&"userprofile".into()) && !removed.contains(&"Path".into()));
}

#[test]
fn each_folder_has_its_own_sid_and_keeps_it() {
    let a = project_sid(Path::new("C:/work/a"));
    assert_eq!(a, project_sid(Path::new("C:/work/a")));
    assert_ne!(a, project_sid(Path::new("C:/work/b")));
    assert_eq!(a, project_sid(Path::new("C:/WORK/A")));
    // One key, three folders: each its own SID and folders.
    let names = ["C:/w/a-b", "C:/w/a b", "C:/w/a/b"].map(Path::new);
    assert_eq!(project_key(names[0]), project_key(names[1]));
    for (i, x) in names.iter().enumerate() {
        for y in &names[i + 1..] {
            assert_ne!(project_sid(x), project_sid(y));
            assert_ne!(project_dir(x), project_dir(y));
        }
    }
    assert!(
        a.starts_with("S-1-5-21-") && a.split('-').count() == 8,
        "{a}"
    );
}

#[test]
fn a_helper_installed_in_the_home_folder_can_be_started() {
    let root = Root::new("helper");
    let work = root.dir("projects/app");
    let claude = root.file("sys/bin/claude.exe");
    let mut setup = root.setup(&["sys/bin"]);
    setup.helper = root.file("home/AppData/Local/Programs/Ovra/ovra.exe");
    let profile = SandboxProfile {
        kind: ovra_core::services::ProfileKind::Agent,
        claude_dir: None,
        work_dir: work,
        read: Vec::new(),
        write: Vec::new(),
        expose: Vec::new(),
        forward: Vec::new(),
        network: ProfileNetwork::Blocked,
    };
    let plan = Plan::new(&setup, &profile, &claude).unwrap();
    let folder = root.0.join("home/AppData/Local/Programs/Ovra");
    assert!(plan.grants.contains(&(folder, Right::ReadExecute)));
}

#[test]
fn secret_folders_stay_hidden_whatever_the_case_of_their_name() {
    let root = Root::new("case-secret");
    let work = root.dir("projects/app");
    let claude = root.file("sys/bin/claude.exe");
    let tools = root.dir("home/.SSH/bin");
    let setup = root.setup(&["home/.SSH/bin", "sys/bin"]);
    let profile = SandboxProfile {
        kind: ovra_core::services::ProfileKind::Agent,
        claude_dir: None,
        work_dir: work,
        read: Vec::new(),
        write: Vec::new(),
        expose: Vec::new(),
        forward: Vec::new(),
        network: ProfileNetwork::Blocked,
    };
    let plan = Plan::new(&setup, &profile, &claude).unwrap();
    assert!(plan.grants.iter().all(|(p, _)| !p.starts_with(&tools)));
    let home_claude = root.dir("home/.Claude/x");
    let profile = SandboxProfile {
        kind: ovra_core::services::ProfileKind::Agent,
        claude_dir: None,
        work_dir: home_claude,
        read: Vec::new(),
        write: Vec::new(),
        expose: Vec::new(),
        forward: Vec::new(),
        network: ProfileNetwork::Blocked,
    };
    assert!(Plan::new(&setup, &profile, &claude).is_err());
}

#[test]
fn server_pods_are_refused_and_forwards_left_out() {
    let root = Root::new("pods");
    let work = root.dir("projects/app");
    let claude = root.file("sys/bin/claude.exe");
    let setup = root.setup(&["sys/bin"]);
    let mut profile = SandboxProfile {
        kind: ovra_core::services::ProfileKind::Agent,
        claude_dir: None,
        work_dir: work,
        read: Vec::new(),
        write: Vec::new(),
        expose: Vec::new(),
        forward: vec![5173],
        network: ProfileNetwork::Blocked,
    };
    assert!(Plan::new(&setup, &profile, &claude).is_ok());
    profile.expose = vec![5173];
    let err = Plan::new(&setup, &profile, &claude).unwrap_err();
    assert!(err.to_string().contains("server pods"), "{err}");
}

#[test]
fn an_account_with_its_own_folder_is_refused() {
    let root = Root::new("account");
    let work = root.dir("projects/app");
    let claude = root.file("sys/bin/claude.exe");
    let setup = root.setup(&["sys/bin"]);
    let profile = SandboxProfile {
        kind: ovra_core::services::ProfileKind::Agent,
        claude_dir: Some(root.dir("home/.claude-work")),
        work_dir: work,
        read: Vec::new(),
        write: Vec::new(),
        expose: Vec::new(),
        forward: Vec::new(),
        network: ProfileNetwork::Blocked,
    };
    let err = Plan::new(&setup, &profile, &claude).unwrap_err();
    assert!(err.to_string().contains("account"), "{err}");
}
