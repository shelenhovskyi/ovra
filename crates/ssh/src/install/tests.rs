//! The install commands, run under `sh` on a temporary `HOME`, and the
//! install against a fake `ssh`.

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Stdio;
use std::sync::Mutex;

use tokio::io::AsyncWriteExt as _;

use super::{Arch, Build, INSTALLED, Sha256, UPLOAD_MAX, command};
use crate::link::tests::{abc, fake, ok};
use crate::{Reason, RemoteHosts as _};

const BUILD: &[u8] = b"#!/bin/sh\necho new\n";

fn sha(bytes: &[u8]) -> Sha256 {
    Sha256::of(bytes)
}

#[test]
fn the_remote_commands_are_fixed() {
    let command = command(&sha(BUILD));
    assert!(!command.contains('\n'));
    insta::assert_snapshot!(command);
    assert_eq!(super::UNAME, "uname -sm");
}

#[test]
fn only_the_two_linux_answers_name_a_build() {
    assert_eq!(Arch::from_uname(b"Linux x86_64\n"), Some(Arch::X86_64));
    assert_eq!(Arch::from_uname(b"Linux aarch64"), Some(Arch::Aarch64));
    for other in [
        &b"Darwin arm64\n"[..],
        b"Linux armv7l\n",
        b"Linux x86_64\n\n",
        b" Linux x86_64",
        b"Linux x86_64; rm -rf ~",
        b"",
    ] {
        assert_eq!(Arch::from_uname(other), None, "{other:?}");
    }
    assert_eq!(
        Arch::Aarch64.file_name(),
        "ovra-daemon-aarch64-unknown-linux-musl"
    );
}

#[test]
fn only_a_hash_goes_into_the_command_and_the_build_must_match_it() {
    for bad in [
        "",
        &"A".repeat(64),
        &"a".repeat(63),
        &format!("{}'", "a".repeat(63)),
    ] {
        assert!(Sha256::parse(bad).is_none(), "{bad}");
    }
    assert!(Sha256::parse(&"0f".repeat(32)).is_some());
    assert!(Build::checked(BUILD.to_vec(), &sha(BUILD)).is_ok());
    assert!(Build::checked(b"other".to_vec(), &sha(BUILD)).is_err());
    let huge = vec![0; UPLOAD_MAX + 1];
    assert!(Build::checked(huge, &sha(BUILD)).is_err());
}

/// Runs the install command as a login shell would, on `home`, with
/// `upload` on stdin; true when it succeeded.
async fn upload(home: &Path, expected: &Sha256, upload: &[u8]) -> bool {
    let mut child = ok(tokio::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(command(expected))
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn());
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(upload).await;
    }
    ok(child.wait().await).success()
}

fn bin(home: &Path) -> std::path::PathBuf {
    home.join(".local/share/ovra/bin")
}

#[tokio::test]
async fn the_command_installs_only_a_whole_build_into_folders_of_its_own() {
    let home = ok(tempfile::tempdir());
    let home = home.path();
    assert!(upload(home, &sha(BUILD), BUILD).await);
    let installed = bin(home).join("ovra-daemon");
    assert_eq!(ok(std::fs::read(&installed)), BUILD);
    let mode = |p: &Path| ok(std::fs::metadata(p)).permissions().mode() & 0o777;
    assert_eq!(mode(&installed), 0o700);
    assert_eq!(mode(&bin(home)), 0o700);

    // Short: the old build stays and no `.new` is left behind.
    let next = b"#!/bin/sh\necho next\n";
    assert!(!upload(home, &sha(next), &next[..5]).await);
    assert_eq!(ok(std::fs::read(&installed)), BUILD);
    assert!(!bin(home).join("ovra-daemon.new").exists());

    // A planted `.new` link is removed, never written through.
    let target = home.join("target");
    ok(std::fs::write(&target, "mine"));
    ok(std::os::unix::fs::symlink(
        &target,
        bin(home).join("ovra-daemon.new"),
    ));
    assert!(upload(home, &sha(next), next).await);
    assert_eq!(ok(std::fs::read_to_string(&target)), "mine");
    assert_eq!(ok(std::fs::read(&installed)), next);
}

#[tokio::test]
async fn the_command_refuses_a_bin_folder_that_is_a_link() {
    let home = ok(tempfile::tempdir());
    let home = home.path();
    let elsewhere = home.join("elsewhere");
    ok(std::fs::create_dir_all(&elsewhere));
    ok(std::fs::create_dir_all(home.join(".local/share/ovra")));
    ok(std::os::unix::fs::symlink(&elsewhere, bin(home)));
    assert!(!upload(home, &sha(BUILD), BUILD).await);
    assert_eq!(ok(std::fs::read_dir(&elsewhere)).count(), 0);
}

/// A fake `ssh` that answers `uname -sm` with `arch` and keeps the upload
/// in `got`.
fn machine(dir: &Path, arch: &str) -> crate::OpenSsh {
    let got = dir.join("got");
    let body = format!(
        "case \"$*\" in *'uname -sm') echo '{arch}' ;; *) cat > '{}' ;; esac",
        got.display()
    );
    fake(dir, None, &body)
}

#[tokio::test]
async fn installing_uploads_the_build_for_the_machine_and_points_the_record_at_it() {
    let dir = ok(tempfile::tempdir());
    let ssh = machine(dir.path(), "Linux aarch64");
    let asked = Mutex::new(None);
    let sent = Mutex::new(Vec::new());
    let host = ssh
        .install(
            &abc(),
            |arch| {
                *ok(asked.lock()) = Some(arch);
                Build::checked(BUILD.to_vec(), &sha(BUILD))
            },
            |done, total| ok(sent.lock()).push((done, total)),
        )
        .await;
    assert_eq!(ok(host).daemon, INSTALLED);
    assert_eq!(*ok(asked.lock()), Some(Arch::Aarch64));
    assert_eq!(ok(std::fs::read(dir.path().join("got"))), BUILD);
    let len = BUILD.len() as u64;
    assert_eq!(ok(sent.lock()).last(), Some(&(len, len)));
    assert_eq!(ok(ssh.list())[0].daemon, INSTALLED);
}

#[tokio::test]
async fn another_system_or_a_missing_build_uploads_nothing() {
    let dir = ok(tempfile::tempdir());
    let ssh = machine(dir.path(), "Darwin arm64");
    let refused = ssh
        .install(&abc(), |_| panic!("no build for it"), |_, _| {})
        .await
        .err();
    assert!(refused.is_some_and(|f| f.details.contains("only on Linux")));

    let ssh = machine(dir.path(), "Linux x86_64");
    let missing = ssh
        .install(&abc(), |_| Err("no build here".to_owned()), |_, _| {})
        .await
        .err();
    assert!(missing.is_some_and(|f| f.reason == Reason::Exited && f.details == "no build here"));
    assert!(!dir.path().join("got").exists());
    assert_eq!(ok(ssh.list())[0].daemon, "ovra-daemon");
}

#[tokio::test]
async fn the_command_refuses_a_linked_data_folder_or_a_folder_in_the_binarys_place() {
    let home = ok(tempfile::tempdir());
    let home = home.path();
    let elsewhere = home.join("elsewhere");
    ok(std::fs::create_dir_all(&elsewhere));
    ok(std::fs::create_dir_all(home.join(".local/share")));
    ok(std::os::unix::fs::symlink(
        &elsewhere,
        home.join(".local/share/ovra"),
    ));
    assert!(!upload(home, &sha(BUILD), BUILD).await);
    assert!(!elsewhere.join("bin/ovra-daemon").exists());

    let other = ok(tempfile::tempdir());
    let other = other.path();
    ok(std::fs::create_dir_all(bin(other).join("ovra-daemon")));
    assert!(!upload(other, &sha(BUILD), BUILD).await);
    assert!(!bin(other).join("ovra-daemon.new").exists());
}
