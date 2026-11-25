//! The whole chain on a real Windows machine, as an administrator: the
//! one-time setup creates the sandbox user and its firewall rule, the
//! helper runs the probe as that user under the token, and removal undoes
//! the setup. It changes the machine, so it is ignored unless asked for:
//! the Windows CI job runs it with `cargo test -- --ignored`.

#![cfg(windows)]
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PROBE: &str = env!("CARGO_BIN_EXE_ovra-sandbox-probe");

fn probe(args: &[&str]) -> std::process::Output {
    Command::new(PROBE).args(args).output().unwrap()
}

fn text(path: &Path) -> String {
    path.display().to_string()
}

#[test]
#[ignore = "creates a Windows account; needs an administrator"]
fn the_sandbox_user_writes_its_project_only_and_reaches_nothing() {
    let root = std::env::temp_dir().join(format!("ovra-win-setup-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let (data, work) = (root.join("data"), root.join("work"));
    fs::create_dir_all(work.join(".git")).unwrap();
    fs::create_dir_all(&data).unwrap();
    let home = PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
    let secret = home.join(format!("ovra-secret-{}.txt", std::process::id()));
    fs::write(&secret, "secret").unwrap();

    // Already elevated, so `runas` asks nothing.
    let out = probe(&["--ovra-sandbox-setup", &text(&data)]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(data.join("sandbox-user").is_file());

    // Another project a session ran in before: the sandbox user keeps
    // its grant there.
    let other = root.join("other");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("kept.txt"), "kept").unwrap();
    let other_sid = "S-1-5-21-1000000031-1000000032-1000000033-1000000034";
    let out = Command::new(PROBE)
        .args([
            "--ovra-sandbox-helper",
            "--sid",
            other_sid,
            "--data",
            &text(&data),
        ])
        .args(["--m", &text(&other)])
        .args(["--rx", &text(Path::new(PROBE).parent().unwrap())])
        .args(["--env", "SystemRoot", "--", PROBE])
        .current_dir(&other)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let sid = "S-1-5-21-1000000011-1000000012-1000000013-1000000014";
    let probe_dir = text(Path::new(PROBE).parent().unwrap());
    let out = Command::new(PROBE)
        .args([
            "--ovra-sandbox-helper",
            "--sid",
            sid,
            "--data",
            &text(&data),
        ])
        .args(["--m", &text(&work), "--deny", &text(&work.join(".git"))])
        .args(["--rx", &probe_dir, "--env", "SystemRoot", "--", PROBE])
        .args(["write", &text(&work.join("a.txt"))])
        .args(["write", &text(&work.join(".git").join("config"))])
        .args(["read", &text(&secret)])
        .args(["write", &text(&home.join("planted.txt"))])
        .args(["connect", "1.1.1.1:443"])
        .args(["remove", &text(&work.join(".git"))])
        .args(["write", &text(&other.join("kept.txt"))])
        .args(["remove", &text(&other.join("kept.txt"))])
        .current_dir(&work)
        .output()
        .unwrap();
    let removed = probe(&["--ovra-sandbox-remove"]);
    let _ = fs::remove_file(&secret);
    let _ = fs::remove_dir_all(&root);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    // The last line is a known limit (docs/specs/sandbox-windows.md):
    // Windows does not count DELETE as a write, so the token's project SID
    // does not stop it, and the sandbox user may delete in every project it
    // was ever granted.
    let shown = String::from_utf8_lossy(&out.stdout)
        .replace(&text(&work), "WORK")
        .replace(&text(&other), "OTHER")
        .replace(&text(&home), "HOME")
        .replace(&std::process::id().to_string(), "PID");
    insta::assert_snapshot!(shown, @r"
    write WORK\a.txt: yes
    write WORK\.git\config: no
    read HOME\ovra-secret-PID.txt: no
    write HOME\planted.txt: no
    connect 1.1.1.1:443: no
    remove WORK\.git: no
    write OTHER\kept.txt: no
    remove OTHER\kept.txt: yes
    ");
}
