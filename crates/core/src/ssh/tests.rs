use std::sync::Arc;

use serde_json::json;

use super::{DaemonPath, Destination, HostId, MachineName};
use crate::Core;
use crate::fakes::MemoryRemoteHosts;
use crate::protocol::ssh::{SshEvent, SshResponse};
use crate::protocol::{ErrorCode, Event, Request, Response};

const KEY: &str = "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU";

async fn call(core: &Core, value: serde_json::Value) -> Result<Response, ErrorCode> {
    let request: Request = serde_json::from_value(value).unwrap_or_else(|e| panic!("{e}"));
    core.handle(request).await.map_err(|e| e.code)
}

#[test]
fn destinations_cannot_carry_options_ports_or_shell() {
    for good in ["build", "dev@build.example.org", "a_b-c.d", "u-1@10.0.0.2"] {
        assert!(Destination::parse(good).is_ok(), "{good}");
    }
    for bad in [
        "",
        "-oProxyCommand=sh",
        "dev@-oProxyCommand=sh",
        "-dev@build",
        ".hidden",
        "dev@.build",
        "build:2222",
        "ssh://build",
        "dev@build@x",
        "build box",
        "build;id",
        "build\nx",
        "@build",
        "dev@",
        &"a".repeat(65),
    ] {
        assert_eq!(
            Destination::parse(bad).map_err(|e| e.code),
            Err(ErrorCode::BadRequest),
            "{bad:?}"
        );
    }
}

#[test]
fn daemon_paths_are_safe_for_the_remote_shell() {
    assert_eq!(
        DaemonPath::parse(None).map(|d| d.0),
        Ok("ovra-daemon".into())
    );
    for good in [
        "ovra-daemon",
        "/opt/ovra/bin/ovra-daemon",
        "~/bin/ovra-daemon",
        "bin/ovra",
        "./ovra",
        ".ovra",
    ] {
        assert!(DaemonPath::parse(Some(good)).is_ok(), "{good}");
    }
    for bad in [
        "",
        ".",
        "./",
        "./.",
        "bin/./ovra",
        "~",
        "~/",
        "~user/bin/ovra",
        "-x",
        "~/-x",
        "/opt/../bin/ovra",
        "a/../../b",
        "..",
        "/opt//ovra",
        "/opt/ovra/",
        "ovra-daemon --serve",
        "ovra;id",
        "$(id)",
        "`id`",
        "a'b",
        "a\"b",
        "a\\b",
        "a\nb",
        "ovra*",
    ] {
        assert!(DaemonPath::parse(Some(bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn names_are_short_and_visible() {
    assert_eq!(
        MachineName::parse("  Build box ").map(|n| n.0),
        Ok("Build box".into())
    );
    for bad in [
        "",
        "   ",
        "a\nb",
        "a\u{202E}b",
        "a\u{200B}b",
        "a\u{7}b",
        "a\u{E0041}b",
        "a\u{FE0F}",
        "a\u{034F}b",
        "a\u{E000}",
        "\u{3164}\u{3164}",
        "\u{2800}",
        &"x".repeat(41),
    ] {
        assert!(MachineName::parse(bad).is_err(), "{bad:?}");
    }
    assert!(MachineName::parse(&"\u{e9}".repeat(40)).is_ok());
    assert!(MachineName::parse("box\u{2800}1").is_ok());
}

#[test]
fn host_ids_name_a_folder_and_nothing_else() {
    assert!(HostId::parse("h1").is_ok());
    for bad in ["", "../x", "H1", "a b", "a/b", ".", &"a".repeat(33)] {
        assert!(HostId::parse(bad).is_err(), "{bad:?}");
    }
}

#[tokio::test]
async fn a_machine_is_prepared_added_renamed_and_removed() {
    let hosts = Arc::new(MemoryRemoteHosts::knowing(
        "dev@build",
        &[("ssh-ed25519", KEY)],
    ));
    let core = Core::builder("test").ssh(hosts).build();
    let mut events = core.subscribe();

    let Ok(Response::Ssh(SshResponse::Prepared(prepared))) = call(
        &core,
        json!({"method": "ssh.prepare", "params": {"destination": "dev@build"}}),
    )
    .await
    else {
        panic!("not prepared")
    };
    assert_eq!(prepared.keys[0].fingerprint, KEY);

    let added = call(
        &core,
        json!({"method": "ssh.add", "params": {
            "name": "Build", "destination": "dev@build", "fingerprints": [KEY]}}),
    )
    .await;
    let Ok(Response::Ssh(SshResponse::Host(host))) = added else {
        panic!("{added:?}")
    };
    assert_eq!(
        (host.daemon.as_str(), host.fingerprints.len()),
        ("ovra-daemon", 1)
    );
    assert!(
        matches!(events.try_recv(), Ok(Event::Ssh(SshEvent::Changed(list))) if list.len() == 1)
    );

    let renamed = call(
        &core,
        json!({"method": "ssh.rename", "params": {"id": host.id, "name": "Box"}}),
    )
    .await;
    assert!(matches!(renamed, Ok(Response::Ssh(SshResponse::Host(h))) if h.name == "Box"));

    assert!(
        call(
            &core,
            json!({"method": "ssh.remove", "params": {"id": host.id}})
        )
        .await
        .is_ok()
    );
    assert!(matches!(
        call(&core, json!({"method": "ssh.list"})).await,
        Ok(Response::Ssh(SshResponse::Hosts(list))) if list.is_empty()
    ));
}

#[tokio::test]
async fn only_keys_the_persons_ssh_trusts_are_pinned() {
    let hosts = Arc::new(MemoryRemoteHosts::knowing("build", &[("ssh-ed25519", KEY)]));
    let core = Core::builder("test").ssh(hosts.clone()).build();
    let other = "SHA256:AAAAQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU";
    let add = |fingerprints: serde_json::Value| {
        json!({"method": "ssh.add", "params": {
            "name": "Build", "destination": "build", "fingerprints": fingerprints}})
    };
    assert_eq!(
        call(&core, add(json!([other]))).await.err(),
        Some(ErrorCode::Conflict)
    );
    assert_eq!(
        call(&core, add(json!([]))).await.err(),
        Some(ErrorCode::BadRequest)
    );
    assert_eq!(
        call(&core, add(json!(["md5:aa"]))).await.err(),
        Some(ErrorCode::BadRequest)
    );
    assert_eq!(
        call(&core, add(json!([KEY, KEY]))).await.err(),
        Some(ErrorCode::BadRequest)
    );
    // The person's known_hosts changed between prepare and add.
    hosts
        .known
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    assert_eq!(
        call(&core, add(json!([KEY]))).await.err(),
        Some(ErrorCode::Conflict)
    );
}

#[tokio::test]
async fn bad_values_never_reach_the_service() {
    let core = Core::builder("test")
        .ssh(Arc::new(MemoryRemoteHosts::default()))
        .build();
    let add = json!({"method": "ssh.add", "params": {
        "name": "x", "destination": "build", "daemon": "ovra;id", "fingerprints": [KEY]}});
    assert_eq!(call(&core, add).await.err(), Some(ErrorCode::BadRequest));
    let prepare = json!({"method": "ssh.prepare", "params": {"destination": "-oProxyCommand=id"}});
    assert_eq!(
        call(&core, prepare).await.err(),
        Some(ErrorCode::BadRequest)
    );
    let remove = json!({"method": "ssh.remove", "params": {"id": "../x"}});
    assert_eq!(call(&core, remove).await.err(), Some(ErrorCode::BadRequest));
}

#[tokio::test]
async fn without_the_service_remote_machines_are_unavailable() {
    let core = Core::builder("test").build();
    assert_eq!(
        call(&core, json!({"method": "ssh.list"})).await.err(),
        Some(ErrorCode::Unavailable)
    );
}
