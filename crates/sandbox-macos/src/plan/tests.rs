#![allow(clippy::unwrap_used)]

use std::ffi::OsStr;

use ovra_core::services::{AgentState, WorkEnv};

use super::*;

/// The Claude runtime's own lists, compiled here so the plans below are
/// those of `claude`'s real state. Read as source: this crate cannot depend
/// on the runtime, so the crate graph does not see this.
#[path = "../../../agent-runtime/src/claude_lists.rs"]
mod claude_lists;

fn claude_state(config_dir: Option<PathBuf>) -> Option<AgentState> {
    Some(AgentState {
        lists: &claude_lists::CLAUDE_LISTS,
        config_dir,
    })
}

const PROTECTED: &[&str] = &[".claude", ".vscode", "**/CLAUDE.md"];

struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ovra-mac-plan-{}-{name}", std::process::id()));
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

    fn setup(&self) -> Setup {
        let path = ["home/.cargo/bin", "home/.docker/bin", "usr/bin", "home"].map(|p| self.dir(p));
        Setup {
            helper: self.file("bin/ovra"),
            home: self.dir("home"),
            path: std::env::join_paths(path).unwrap(),
            toolchains: vec![self.dir("home/.rustup")],
            data_dir: self.dir("data"),
            caches: self.0.join("data/sandbox-caches"),
            temp: self.0.join("data/sandbox-temp"),
            hidden: vec![self.dir("tmp")],
            protected: PROTECTED,
        }
    }

    fn line(&self) -> CommandLine {
        CommandLine {
            program: self.file("home/.local/share/claude/versions/2.0"),
            args: vec!["-p".into()],
            env: vec![("CLAUDECODE".into(), None)],
            cwd: self.dir("projects/app"),
        }
    }

    fn profile(&self) -> SandboxProfile {
        SandboxProfile {
            kind: ProfileKind::Agent,
            agent_state: claude_state(None),
            project: Some(ProjectId("p1".into())),
            work_env: None,
            work_dir: self.dir("projects/app"),
            work_dir_read_only: false,
            read: vec![self.dir("projects/main.git")],
            write: Vec::new(),
            expose: Vec::new(),
            forward: Vec::new(),
            network: ProfileNetwork::Blocked,
        }
    }

    fn clean(&self, text: &str) -> String {
        let key = project_key(&self.0);
        let folded = crate::sbpl::fold_for_tests(&self.0.display().to_string());
        let temp = cache_key(None, &self.0.join("projects/app"));
        let folded_key = crate::sbpl::fold_for_tests(&key);
        text.replace(&temp, "WORK-DIR-KEY")
            .replace(&folded, "ROOT")
            .replace(&folded_key, "-ROOT")
            .replace(&key, "-ROOT")
            .replace(&self.0.display().to_string(), "ROOT")
    }

    /// The helper's line without the profile, one flag and its value per
    /// line, then the variables it sets or removes.
    fn show(&self, line: &CommandLine) -> String {
        let clean = |s: &OsStr| self.clean(&s.to_string_lossy());
        let mut out = String::new();
        let mut args = line.args.iter();
        while let Some(arg) = args.next() {
            if arg == "--profile" {
                args.next();
                out.push_str("--profile <profile>\n");
                continue;
            }
            out.push_str(&clean(arg));
            if arg == "--env" || arg == "--bridge" {
                out.push(' ');
                out.push_str(&clean(args.next().unwrap()));
            }
            out.push('\n');
        }
        out.push_str("env:\n");
        for (name, value) in &line.env {
            match value {
                Some(value) => out.push_str(&format!("  {}={}\n", clean(name), clean(value))),
                None => out.push_str(&format!("  -{}\n", clean(name))),
            }
        }
        out.push_str(&format!("cwd: {}\n", clean(line.cwd.as_os_str())));
        out
    }

    fn profile_text(&self, plan: &Plan) -> String {
        self.clean(&crate::sbpl::profile(plan).unwrap())
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn outer() -> Vec<OsString> {
    [
        "HOME",
        "GITHUB_TOKEN",
        "SSH_AUTH_SOCK",
        "ANTHROPIC_API_KEY",
        "NO_PROXY",
        "CLAUDE_CODE_OAUTH_TOKEN",
    ]
    .map(OsString::from)
    .to_vec()
}

#[test]
fn an_agent_session() {
    let root = Root::new("agent");
    root.file("home/.gitconfig");
    root.file("home/.cargo/credentials.toml");
    let setup = root.setup();
    let mut profile = root.profile();
    let bundle = root.file("tmp/ca.pem");
    let socket = root.file("tmp/net/p.sock");
    profile.network = ProfileNetwork::Proxy {
        address: socket.display().to_string(),
        ca_bundle: Some(bundle),
    };
    profile.work_env = Some(WorkEnv {
        port: 41000,
        worktree_root: root.dir("projects/app"),
        main_root: root.dir("projects/main"),
    });
    let plan = Plan::agent(&setup, &profile, &root.line()).unwrap();
    insta::assert_snapshot!("agent_profile", root.profile_text(&plan));
    let line = plan.line(&setup, root.line(), &outer()).unwrap();
    insta::assert_snapshot!("agent_line", root.show(&line));
}

#[test]
fn a_research_session_writes_nothing_in_its_folder() {
    let root = Root::new("research");
    let setup = root.setup();
    let mut profile = root.profile();
    profile.work_dir_read_only = true;
    let plan = Plan::agent(&setup, &profile, &root.line()).unwrap();
    let work = root.dir("projects/app");
    assert!(plan.read.contains(&work));
    assert!(!plan.write.iter().any(|w| w.starts_with(&work)));
}

#[test]
fn a_protected_name_that_is_a_link_keeps_its_target_read_only() {
    let root = Root::new("link");
    let setup = root.setup();
    let target = root.dir("projects/app/notes");
    std::os::unix::fs::symlink(&target, root.0.join("projects/app/.claude")).unwrap();
    let plan = Plan::agent(&setup, &root.profile(), &root.line()).unwrap();
    assert!(
        plan.protected.contains(&Pattern::Under(target)),
        "{:?}",
        plan.protected
    );
}

#[test]
fn an_accounts_own_folder_replaces_the_default_one() {
    let root = Root::new("account");
    let setup = root.setup();
    let work = root.dir("home/.claude-work");
    let mut profile = root.profile();
    profile.agent_state = claude_state(Some(work.clone()));
    let plan = Plan::agent(&setup, &profile, &root.line()).unwrap();
    assert!(plan.read.contains(&work));
    assert!(!plan.read.iter().any(|r| r.ends_with(".claude")));
    assert!(
        plan.set
            .contains(&("CLAUDE_CONFIG_DIR".into(), Some(work.into())))
    );
    // Any other folder is refused.
    profile.agent_state = claude_state(Some(root.dir("home/elsewhere")));
    assert!(Plan::agent(&setup, &profile, &root.line()).is_err());
}

#[test]
fn what_no_session_gets() {
    let root = Root::new("refused");
    let setup = root.setup();
    let line = root.line();
    let refused = |change: &dyn Fn(&mut SandboxProfile)| {
        let mut profile = root.profile();
        change(&mut profile);
        Plan::agent(&setup, &profile, &line)
            .and_then(|plan| plan.line(&setup, line.clone(), &[]))
            .err()
            .map(|e| e.message)
    };
    let pod = refused(&|p| {
        p.kind = ProfileKind::Pod;
        p.expose = vec![3000];
    });
    assert!(pod.unwrap().contains("server pods"));
    assert!(refused(&|p| p.kind = ProfileKind::Browser).is_some());
    assert!(refused(&|p| p.work_dir = root.dir("home")).is_some());
    assert!(refused(&|p| p.read = vec![root.0.clone()]).is_some());
    assert!(refused(&|p| p.write = vec![root.dir("home/.claude/agents")]).is_some());
    assert!(refused(&|p| p.write = vec![root.dir("home/.Claude-x")]).is_some());
    assert!(refused(&|p| p.work_dir = root.dir("projects/a\"b")).is_some());
}

#[test]
fn a_language_server() {
    let root = Root::new("server");
    let setup = root.setup();
    let profile = ServerProfile {
        project: root.dir("projects/app"),
        cache_dir: root.dir("data/lsp/p1/rust"),
        read: Vec::new(),
        network: ProfileNetwork::Blocked,
    };
    let mut line = root.line();
    line.env = vec![(
        "CARGO_TARGET_DIR".into(),
        Some(root.dir("data/lsp/p1/rust/target").into()),
    )];
    let plan = Plan::server(&setup, &profile, &line).unwrap();
    assert_eq!(plan.write, vec![root.dir("data/lsp/p1/rust")]);
    assert!(plan.read.contains(&root.dir("projects/app")));
    assert_eq!(plan.net, Net::Blocked);
    assert_eq!(plan.sockets, None);
    assert!(
        plan.never
            .iter()
            .all(|n| !matches!(n, Pattern::Under(p) if p.ends_with(".claude")))
    );
    let shown = root.show(&plan.line(&setup, line.clone(), &outer()).unwrap());
    assert!(
        shown.contains("CARGO_TARGET_DIR=ROOT/data/lsp/p1/rust/target"),
        "{shown}"
    );
    assert!(!shown.contains("--env ANTHROPIC_API_KEY"), "{shown}");
    let mut outside = profile.clone();
    outside.cache_dir = root.dir("data/elsewhere");
    assert!(Plan::server(&setup, &outside, &line).is_err());
    let mut networked = profile;
    networked.network = ProfileNetwork::Open;
    assert!(Plan::server(&setup, &networked, &line).is_err());
}
