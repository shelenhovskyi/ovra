//! The link against a fake `ssh`: a shell script that prints a safe
//! `ssh -G` and then plays the remote machine.

use std::path::Path;
use std::time::Duration;

use ovra_core::ssh::HostId;

use super::Reason;
use crate::OpenSsh;
use crate::tools::Tools;

pub(crate) fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
    r.unwrap_or_else(|e| panic!("{e:?}"))
}

pub(crate) fn hello(version: &str) -> String {
    format!(r#"echo '{{"hello":{{"version":"{version}","protocol":1,"os":"linux"}}}}'"#)
}

pub(crate) fn ours() -> String {
    hello(env!("CARGO_PKG_VERSION"))
}

pub(crate) fn safe_config(pins: &Path) -> String {
    format!(
        "user dev\nhostname build.example\nport 22\nstricthostkeychecking true\n\
         updatehostkeys false\nbatchmode yes\ncheckhostip no\nrequesttty false\n\
         globalknownhostsfile none\nuserknownhostsfile {}\nverifyhostkeydns false\n\
         gssapidelegatecredentials no\nforwardagent no\nforwardx11 no\n\
         forwardx11trusted no\nclearallforwardings yes\ncontrolmaster false\n\
         permitlocalcommand no\ntunnel false\nsessiontype default\nstdinnull no\n\
         forkafterauthentication no\nhostkeyalias ovra-abc\nsendenv LANG\n",
        pins.display()
    )
}

/// An `OpenSsh` with the machine `abc` whose `ssh` prints `config` for
/// `-G` and otherwise runs `body`.
pub(crate) fn fake(dir: &Path, config: Option<&str>, body: &str) -> OpenSsh {
    use std::os::unix::fs::PermissionsExt;
    let data = dir.join("data");
    let pins = data.join("ssh/abc/known_hosts");
    ok(std::fs::create_dir_all(data.join("ssh/abc")));
    ok(std::fs::write(&pins, "ovra-abc ssh-ed25519 AAAA\n"));
    ok(std::fs::write(
        data.join("ssh/hosts.json"),
        format!(
            r#"[{{"id":"abc","name":"Build","destination":"build","daemon":"ovra-daemon","fingerprints":["SHA256:{}"]}}]"#,
            "A".repeat(43)
        ),
    ));
    let g = dir.join("g");
    let config = config.map_or_else(|| safe_config(&pins), str::to_owned);
    ok(std::fs::write(&g, config));
    let script = dir.join("ssh");
    ok(std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do if [ \"$a\" = -G ]; then cat '{}'; exit 0; fi; done\n{body}\n",
            g.display()
        ),
    ));
    ok(std::fs::set_permissions(
        &script,
        std::fs::Permissions::from_mode(0o755),
    ));
    let ssh = OpenSsh::with_config(&data, dir.join("unused-config"));
    *crate::lock(&ssh.tools) = Some(Tools {
        ssh: script,
        keygen: "/bin/false".into(),
        limit: Duration::from_secs(2),
    });
    ssh
}

pub(crate) fn abc() -> HostId {
    ok(HostId::parse("abc"))
}

#[tokio::test]
async fn frames_pass_both_ways_and_the_machine_cannot_speak_for_the_laptop() {
    let dir = ok(tempfile::tempdir());
    let body = format!(
        r#"echo 'Welcome to build'
{}
while read -r line; do
  case "$line" in
    *ping*) ;;
    *) echo '{{"event":{{"type":"ssh.changed","hosts":[]}}}}'
       echo '{{"event":{{"type":"info.ready"}}}}'
       echo '{{"id":1,"result":{{"ok":true}}}}' ;;
  esac
done"#,
        ours()
    );
    let ssh = fake(dir.path(), None, &body);
    let mut link = ok(ssh.connect(&abc()).await);
    assert_eq!(link.hello().os, "linux");
    ok(link.send(r#"{"id":1,"method":"info.get"}"#).await);
    assert_eq!(ok(link.recv().await), r#"{"event":{"type":"info.ready"}}"#);
    assert_eq!(ok(link.recv().await), r#"{"id":1,"result":{"ok":true}}"#);
    let split = link.send("{\"id\":2,\n\"method\":\"info.get\"}").await;
    assert!(split.is_err());
}

#[tokio::test]
async fn each_way_of_failing_before_the_hello_has_its_reason() {
    let noise = "i=0; while [ $i -lt 2000 ]; do echo 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'; i=$((i+1)); done";
    let cases = [
        (r#"echo '{"busy":{}}'"#.to_owned(), Reason::Busy),
        (hello("0.0.1"), Reason::VersionMismatch),
        (
            "echo '@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@' >&2; exit 255".into(),
            Reason::HostKeyChanged,
        ),
        (
            "echo 'dev@build: Permission denied (publickey).' >&2; exit 255".into(),
            Reason::AuthFailed,
        ),
        (
            "echo 'bash: ovra-daemon: command not found' >&2; exit 127".into(),
            Reason::NoDaemon,
        ),
        (noise.into(), Reason::NoDaemon),
        (
            r"printf 'Connection closed \033[31mred\n' >&2; exit 255".into(),
            Reason::Exited,
        ),
    ];
    for (body, reason) in cases {
        let dir = ok(tempfile::tempdir());
        let failed = fake(dir.path(), None, &body).connect(&abc()).await.err();
        assert_eq!(failed.as_ref().map(|f| f.reason), Some(reason), "{body}");
        if reason == Reason::Exited {
            let details = failed.map(|f| f.details).unwrap_or_default();
            assert_eq!(details, "Connection closed [31mred");
        }
    }
}

#[tokio::test]
async fn a_frame_out_of_shape_ends_the_link() {
    let dir = ok(tempfile::tempdir());
    let body = format!(
        r#"{}
echo '{{"event":{{"type":"info.ready"}}}}'
echo '{{"id":1,"result":{{}},"more":1}}'
sleep 5"#,
        ours()
    );
    let mut link = ok(fake(dir.path(), None, &body).connect(&abc()).await);
    assert!(link.recv().await.is_ok());
    let ended = link.recv().await.err();
    assert!(
        ended.is_some_and(|f| f.reason == Reason::Exited && f.details.contains("does not accept"))
    );
}

#[tokio::test]
async fn an_unsafe_config_or_missing_pins_never_reach_ssh() {
    let dir = ok(tempfile::tempdir());
    let pins = dir.path().join("data/ssh/abc/known_hosts");
    let forwarding = safe_config(&pins).replace("forwardagent no", "forwardagent yes");
    // The body would say the machine is busy; it must not run.
    let ssh = fake(dir.path(), Some(&forwarding), r#"echo '{"busy":{}}'"#);
    let refused = ssh.connect(&abc()).await.err();
    assert!(refused.is_some_and(|f| f.reason == Reason::UnsafeConfig && f.details.contains("forwardagent")));

    let ssh = fake(dir.path(), None, r#"echo '{"busy":{}}'"#);
    ok(std::fs::remove_file(&pins));
    let gone = ssh.connect(&abc()).await.err();
    assert_eq!(gone.map(|f| f.reason), Some(Reason::NoPin));
    let other = ssh.connect(&ok(HostId::parse("zzz"))).await.err();
    assert_eq!(other.map(|f| f.reason), Some(Reason::NoPin));
}

#[tokio::test]
async fn dropping_the_link_ends_ssh() {
    let dir = ok(tempfile::tempdir());
    let pid = dir.path().join("pid");
    let body = format!("echo $$ > '{}'\n{}\nsleep 30", pid.display(), ours());
    let link = ok(fake(dir.path(), None, &body).connect(&abc()).await);
    let pid = ok(ok(std::fs::read_to_string(&pid)).trim().parse::<i32>());
    drop(link);
    let alive = |pid: i32| {
        rustix::process::Pid::from_raw(pid)
            .is_some_and(|p| rustix::process::test_kill_process(p).is_ok())
    };
    let mut waited = 0;
    while alive(pid) && waited < 200 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        waited += 1;
    }
    assert!(!alive(pid));
}

#[tokio::test]
async fn what_the_hello_says_is_shown_as_plain_text() {
    let dir = ok(tempfile::tempdir());
    let body = format!(
        r#"printf '{{"hello":{{"version":"{}","protocol":1,"os":"lin\342\200\256ux","no_sandbox":"no \\u001b[1mbwrap"}}}}\n'
sleep 5"#,
        env!("CARGO_PKG_VERSION")
    );
    let link = ok(fake(dir.path(), None, &body).connect(&abc()).await);
    assert_eq!(link.hello().os, "linux");
    assert_eq!(link.hello().no_sandbox.as_deref(), Some("no [1mbwrap"));
}

#[tokio::test]
async fn hearing_another_client_took_over_ends_the_link_as_replaced() {
    let dir = ok(tempfile::tempdir());
    let body = format!(
        r#"{}
echo '{{"replaced":{{}}}}'
sleep 5"#,
        ours()
    );
    let mut link = ok(fake(dir.path(), None, &body).connect(&abc()).await);
    let ended = link.recv().await.err();
    assert_eq!(ended.map(|f| f.reason), Some(Reason::Replaced));
}

#[tokio::test]
async fn stopping_sends_the_exact_quit_line() {
    let dir = ok(tempfile::tempdir());
    let got = dir.path().join("got");
    let body = format!(
        "{}\nread -r line; printf '%s' \"$line\" > '{}'\nsleep 5",
        ours(),
        got.display()
    );
    let link = ok(fake(dir.path(), None, &body).connect(&abc()).await);
    ok(link.sender().quit().await);
    let mut waited = 0;
    while std::fs::read_to_string(&got).unwrap_or_default().is_empty() && waited < 200 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        waited += 1;
    }
    assert_eq!(ok(std::fs::read_to_string(&got)), r#"{"quit":{}}"#);
}
