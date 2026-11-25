//! The helper on a real Windows machine: the plan's ACEs and the
//! write-restricted token around the probe binary, never a real agent.

#![cfg(windows)]
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PROBE: &str = env!("CARGO_BIN_EXE_ovra-sandbox-probe");

struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        // Under the person's profile, whose ACL grants no one else: a
        // folder in `C:\` may grant every user write.
        let dir =
            std::env::temp_dir().join(format!("ovra-win-sandbox-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(work: &Path, plan: &[&str], probes: &[(&str, PathBuf)]) -> String {
    let mut command = Command::new(PROBE);
    command
        .arg("--ovra-sandbox-helper")
        .args(plan)
        .current_dir(work);
    command.args([
        "--env",
        "SystemRoot",
        "--env",
        "OVRA_PROBE_KEPT",
        "--",
        PROBE,
    ]);
    for (what, path) in probes {
        command.arg(what).arg(path);
    }
    command
        .env("OVRA_PROBE_KEPT", "1")
        .env("OVRA_PROBE_DROPPED", "1");
    let out = command.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).replace(&root_text(work), "WORK")
}

fn root_text(work: &Path) -> String {
    work.display().to_string()
}

#[test]
fn the_token_writes_only_what_the_plan_grants() {
    let root = Root::new("writes");
    let work = root.0.join("work");
    fs::create_dir_all(work.join(".git/hooks")).unwrap();
    fs::write(work.join(".git/config"), "").unwrap();
    let outside = root.0.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let sid = "S-1-5-21-1000000001-1000000002-1000000003-1000000004";
    let work_text = work.display().to_string();
    let git = work.join(".git").display().to_string();
    let plan = ["--sid", sid, "--m", &work_text, "--deny", &git];
    let out = run(
        &work,
        &plan,
        &[
            ("write", work.join("a.txt")),
            ("write", work.join("src.txt")),
            ("write", work.join(".git/hooks/pre-commit")),
            ("write", work.join(".git/config")),
            ("remove", work.join(".git")),
            ("write", outside.join("x.txt")),
            ("read", outside.join("..").join("work").join("a.txt")),
            ("env", "OVRA_PROBE_KEPT".into()),
            ("env", "OVRA_PROBE_DROPPED".into()),
        ],
    );
    let out = out.replace(&root.0.display().to_string(), "ROOT");
    insta::assert_snapshot!(out, @r"
    write WORK\a.txt: yes
    write WORK\src.txt: yes
    write WORK\.git\hooks\pre-commit: no
    write WORK\.git\config: no
    remove WORK\.git: no
    write ROOT\outside\x.txt: no
    read ROOT\outside\..\work\a.txt: yes
    env OVRA_PROBE_KEPT: yes
    env OVRA_PROBE_DROPPED: no
    ");
    assert!(work.join(".git/config").exists());
}

#[test]
fn grants_never_follow_a_junction_out_of_the_project() {
    let root = Root::new("junction");
    let work = root.0.join("work");
    let outside = root.0.join("outside");
    fs::create_dir_all(&work).unwrap();
    fs::create_dir_all(&outside).unwrap();
    // What an agent leaves behind for the next session.
    let made = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(work.join("door"))
        .arg(&outside)
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let sid = "S-1-5-21-1000000021-1000000022-1000000023-1000000024";
    let work_text = work.display().to_string();
    let out = run(
        &work,
        &["--sid", sid, "--m", &work_text],
        &[
            ("write", work.join("a.txt")),
            ("write", outside.join("through.txt")),
            ("write", work.join("door").join("through.txt")),
        ],
    );
    let out = out.replace(&root.0.display().to_string(), "ROOT");
    insta::assert_snapshot!(out, @r"
    write WORK\a.txt: yes
    write ROOT\outside\through.txt: no
    write WORK\door\through.txt: no
    ");
}
