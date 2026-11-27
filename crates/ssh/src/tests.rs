use std::path::Path;

use ovra_core::ssh::{DaemonPath, Destination, HostId};

use super::arguments;

fn destination() -> Destination {
    Destination::parse("dev@build").unwrap_or_else(|e| panic!("{e:?}"))
}

fn id() -> HostId {
    HostId::parse("h1").unwrap_or_else(|e| panic!("{e:?}"))
}

fn daemon() -> DaemonPath {
    DaemonPath::parse(Some("~/bin/ovra-daemon")).unwrap_or_else(|e| panic!("{e:?}"))
}

#[test]
#[cfg(unix)]
fn the_command_line_is_fixed() {
    let pins = Path::new("/Users/me/Library/Application Support/ovra/ssh/h1/known_hosts");
    let args = arguments(pins, &id(), &destination(), &daemon());
    insta::assert_json_snapshot!(args.unwrap_or_else(|e| panic!("{e:?}")));
}

#[test]
#[cfg(unix)]
fn the_pin_path_cannot_break_out_of_its_option() {
    let args = arguments(
        Path::new("/data/100%/known_hosts"),
        &id(),
        &destination(),
        &daemon(),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(args.contains(&"UserKnownHostsFile=\"/data/100%%/known_hosts\"".to_owned()));
    let args = arguments(
        Path::new("/data/a\\b/known_hosts"),
        &id(),
        &destination(),
        &daemon(),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(args.contains(&"UserKnownHostsFile=\"/data/a\\\\b/known_hosts\"".to_owned()));
    for bad in [
        "/data/a\"b/known_hosts",
        "/data/a\nb/known_hosts",
        "/data/${HOME}/known_hosts",
        "relative/known_hosts",
    ] {
        assert!(
            arguments(Path::new(bad), &id(), &destination(), &daemon()).is_err(),
            "{bad:?}"
        );
    }
}

/// Hosts and pins against the real `ssh -G` and `ssh-keygen`, with an SSH
/// config and `known_hosts` of the test's own. Nothing connects anywhere.
#[cfg(unix)]
mod hosts {
    use std::path::Path;

    use ovra_core::protocol::ErrorCode;
    use ovra_core::services::RemoteHosts;
    use ovra_core::ssh::{DaemonPath, Destination, HostId, MachineName};

    use crate::OpenSsh;

    const KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIOVwBLUvHwz0UtEMJKsEMKbeFJm0OWj04597pZvkljeA";
    const OTHER: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIEVC1APpZoUbZvkLfQkQInTKwcijoxXkTLJqIijqKUZk";

    fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        r.unwrap_or_else(|e| panic!("{e:?}"))
    }

    /// An `OpenSsh` whose config sends `build` to `build.example` with
    /// `known` as the person's known_hosts.
    fn setup(dir: &Path, known: &str, port: u16) -> OpenSsh {
        let known_file = dir.join("known_hosts");
        ok(std::fs::write(&known_file, known));
        let config = dir.join("config");
        ok(std::fs::write(
            &config,
            format!(
                "Host build\n  HostName build.example\n  User dev\n  Port {port}\n  UserKnownHostsFile {}\n  GlobalKnownHostsFile none\n",
                known_file.display()
            ),
        ));
        OpenSsh::with_config(&dir.join("data"), config)
    }

    fn build() -> Destination {
        ok(Destination::parse("build"))
    }

    fn name(text: &str) -> MachineName {
        ok(MachineName::parse(text))
    }

    #[test]
    fn keys_the_person_trusts_are_shown_pinned_and_forgotten() {
        let dir = ok(tempfile::tempdir());
        let ssh = setup(
            dir.path(),
            &format!("build.example ssh-ed25519 {KEY}\n"),
            22,
        );
        let prepared = ok(ssh.prepare(&build()));
        assert_eq!(
            (
                prepared.host_name.as_str(),
                prepared.user.as_str(),
                prepared.port
            ),
            ("build.example", "dev", 22)
        );
        assert_eq!(prepared.keys.len(), 1);
        let print = prepared.keys[0].fingerprint.clone();
        assert!(print.starts_with("SHA256:"), "{print}");

        let host = ok(ssh.add(
            &name("Build"),
            &build(),
            &ok(DaemonPath::parse(None)),
            std::slice::from_ref(&print),
        ));
        let pins = dir
            .path()
            .join("data/ssh")
            .join(&host.id)
            .join("known_hosts");
        assert_eq!(
            ok(std::fs::read_to_string(&pins)),
            format!("ovra-{} ssh-ed25519 {KEY}\n", host.id)
        );
        assert_eq!(ok(ssh.list()), vec![host.clone()]);
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| ok(std::fs::metadata(p)).permissions().mode() & 0o777;
            assert_eq!(mode(&pins), 0o600);
            assert_eq!(mode(&dir.path().join("data/ssh/hosts.json")), 0o600);
            assert_eq!(mode(&dir.path().join("data/ssh")), 0o700);
        }

        let id = ok(HostId::parse(&host.id));
        assert_eq!(ok(ssh.rename(&id, &name("Box"))).name, "Box");
        ok(ssh.remove(&id));
        assert!(ok(ssh.list()).is_empty());
        assert!(!pins.exists());
        assert_eq!(
            ssh.remove(&id).err().map(|e| e.code),
            Some(ErrorCode::NotFound)
        );
    }

    #[test]
    fn hashed_ported_revoked_and_unknown_keys() {
        let dir = ok(tempfile::tempdir());
        let known = format!(
            "[build.example]:2222 ssh-ed25519 {KEY}\n[build.example]:2222 ssh-ed25519 {OTHER}\n@revoked [build.example]:2222 ssh-ed25519 {OTHER}\nbuild.example ssh-ed25519 {OTHER}\n"
        );
        let ssh = setup(dir.path(), &known, 2222);
        // Hashed the way Debian and Ubuntu keep it.
        let hashed = std::process::Command::new("ssh-keygen")
            .args(["-q", "-H", "-f"])
            .arg(dir.path().join("known_hosts"))
            .output();
        assert!(hashed.is_ok_and(|o| o.status.success()));
        let prepared = ok(ssh.prepare(&build()));
        assert_eq!(prepared.key_name, "[build.example]:2222");
        assert_eq!(prepared.keys.len(), 1, "{:?}", prepared.keys);

        // A key the person's SSH does not trust is never pinned.
        let other = "SHA256:AAAAQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU".to_owned();
        let refused = ssh.add(&name("B"), &build(), &ok(DaemonPath::parse(None)), &[other]);
        assert_eq!(refused.err().map(|e| e.code), Some(ErrorCode::Conflict));
        assert!(ok(ssh.list()).is_empty());

        let empty = setup(dir.path(), "", 22);
        assert!(ok(empty.prepare(&build())).keys.is_empty());
    }

    #[test]
    fn a_host_known_only_through_a_certificate_authority_is_refused() {
        let dir = ok(tempfile::tempdir());
        let ssh = setup(
            dir.path(),
            &format!("@cert-authority *.example ssh-ed25519 {KEY}\n"),
            22,
        );
        assert_eq!(
            ssh.prepare(&build()).err().map(|e| e.code),
            Some(ErrorCode::Unavailable)
        );
    }
}
