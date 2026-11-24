use std::net::IpAddr;
use std::sync::Arc;

use serde_json::json;

use super::{is_broad, is_reachable, parse_host};
use crate::Core;
use crate::fakes::{FakeNetProxy, MemoryProjects, ScriptedAgent, Step};
use crate::protocol::agents::{AgentStatus, AgentsEvent, SessionId};
use crate::protocol::netproxy::{AllowedHost, Allowlist, NetproxyEvent, NetproxyResponse};
use crate::protocol::{ErrorCode, Event, Request, Response};
use crate::services::ProfileNetwork;

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap_or_else(|e| panic!("{text}: {e}"))
}

#[test]
fn only_public_unicast_addresses_are_reachable() {
    for refused in [
        "0.0.0.0",
        "10.1.2.3",
        "100.64.0.1",
        "127.0.0.1",
        "127.9.9.9",
        "169.254.169.254",
        "172.16.0.1",
        "172.31.255.255",
        "192.0.0.8",
        "192.0.2.1",
        "192.168.1.1",
        "198.18.0.1",
        "224.0.0.1",
        "255.255.255.255",
        "::",
        "::1",
        "::ffff:127.0.0.1",
        "::ffff:169.254.169.254",
        "64:ff9b::7f00:1",
        "fe80::1",
        "fc00::1",
        "fd12:3456::1",
        "ff02::1",
        "2001::1",
        "2001:db8::1",
        "2002:7f00:1::",
        "2002:a9fe:a9fe::",
        "3fff::1",
    ] {
        assert!(!is_reachable(ip(refused)), "{refused} should be refused");
    }
    for allowed in [
        "1.1.1.1",
        "8.8.8.8",
        "172.32.0.1",
        "100.128.0.1",
        "::ffff:8.8.8.8",
        "2606:4700::1111",
        "2002:0808:0808::",
    ] {
        assert!(is_reachable(ip(allowed)), "{allowed} should be reachable");
    }
}

#[test]
fn hosts_are_normalized_and_odd_ones_refused() {
    assert_eq!(
        parse_host(" Registry.NPMjs.org. "),
        Ok("registry.npmjs.org".into())
    );
    assert_eq!(
        parse_host("[2606:4700::1111]"),
        Ok("2606:4700::1111".into())
    );
    assert_eq!(parse_host("8.8.8.8"), Ok("8.8.8.8".into()));
    for bad in [
        "",
        "*.github.com",
        "a..b",
        "-a.com",
        "a_b.com",
        "a.com/path",
        "user@a.com",
        "a.com:443",
        "127.0.0.1",
        "[::1]",
        "169.254.169.254",
        "127.1",
        "0x7f.1",
        "exämple.com",
    ] {
        assert!(parse_host(bad).is_err(), "{bad:?} should be refused");
    }
}

#[test]
fn hosts_anyone_can_receive_data_on_are_broad() {
    assert!(is_broad("github.com"));
    assert!(is_broad("raw.githubusercontent.com"));
    assert!(is_broad("my-bucket.s3.amazonaws.com"));
    assert!(!is_broad("api.github.com.example.org"));
    assert!(!is_broad("notgithub.com"));
    assert!(!is_broad("registry.npmjs.org"));
}

fn host(name: &str, port: Option<u16>) -> AllowedHost {
    AllowedHost {
        host: name.into(),
        port,
    }
}

fn fake_net() -> Arc<FakeNetProxy> {
    Arc::new(FakeNetProxy {
        base: vec![host("api.anthropic.com", None)],
        ..FakeNetProxy::default()
    })
}

async fn core_with(net: Arc<FakeNetProxy>, agent: ScriptedAgent) -> Core {
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(agent))
        .netproxy(net)
        .build();
    call(
        &core,
        json!({"method": "projects.add", "params": {"root": crate::test_path!("/work/a")}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    core
}

async fn call(
    core: &Core,
    json: serde_json::Value,
) -> Result<Response, crate::protocol::ProtocolError> {
    let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
    core.handle(request).await
}

fn set(hosts: serde_json::Value) -> serde_json::Value {
    json!({"method": "netproxy.set_allowlist", "params": {
        "project": "p1", "allowlist": {"hosts": hosts}}})
}

#[tokio::test]
async fn a_projects_allowlist_is_checked_normalized_and_kept() {
    let core = core_with(fake_net(), ScriptedAgent::default()).await;
    let mut rx = core.subscribe();
    let kept = call(
        &core,
        set(json!([
            {"host": "GitHub.com."},
            {"host": "registry.npmjs.org", "port": 8443},
            {"host": "github.com"}
        ])),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    insta::assert_json_snapshot!(kept, @r#"
    {
      "type": "netproxy.allowlist",
      "data": {
        "base": {
          "hosts": [
            {
              "host": "api.anthropic.com"
            }
          ]
        },
        "project": {
          "hosts": [
            {
              "host": "github.com"
            },
            {
              "host": "registry.npmjs.org",
              "port": 8443
            }
          ]
        },
        "broad": [
          "github.com"
        ]
      }
    }
    "#);
    let got = call(
        &core,
        json!({"method": "netproxy.get_allowlist", "params": {"project": "p1"}}),
    )
    .await;
    assert_eq!(got.as_ref().ok(), Some(&kept));
    assert!(matches!(
        rx.recv().await,
        Ok(Event::Netproxy(NetproxyEvent::Changed { .. }))
    ));
    for bad in [
        json!([{"host": "*.github.com"}]),
        json!([{"host": "10.0.0.1"}]),
        json!([{"host": "a.com", "port": 0}]),
    ] {
        let err = call(&core, set(bad)).await.err().map(|e| e.code);
        assert_eq!(err, Some(ErrorCode::BadRequest));
    }
    let Ok(Response::Netproxy(NetproxyResponse::Allowlist(still))) = call(
        &core,
        json!({"method": "netproxy.get_allowlist", "params": {"project": "p1"}}),
    )
    .await
    else {
        panic!("no allowlist");
    };
    assert_eq!(still.project.hosts.len(), 2);
}

fn waits() -> ScriptedAgent {
    ScriptedAgent::new(vec![vec![Step::Say("working")]])
}

async fn start(core: &Core) {
    call(
        core,
        json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "go"}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
}

#[tokio::test]
async fn each_session_runs_behind_its_own_proxy_until_it_ends() {
    let net = fake_net();
    let agent = Arc::new(waits());
    let audit = Arc::new(crate::fakes::MemoryAudit::default());
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(agent.clone())
        .netproxy(net.clone())
        .audit(audit.clone())
        .build();
    call(
        &core,
        json!({"method": "projects.add", "params": {"root": crate::test_path!("/work/a")}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    call(&core, set(json!([{"host": "registry.npmjs.org"}])))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    start(&core).await;
    let s1 = SessionId("s1".into());
    let spec = agent.started.lock().unwrap_or_else(|e| e.into_inner())[0].clone();
    assert_eq!(
        spec.sandbox.network,
        ProfileNetwork::Proxy {
            address: "fake-proxy:s1".into()
        }
    );
    let allowed = |net: &FakeNetProxy| {
        net.running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&s1)
            .map(|(allow, _)| allow.clone())
    };
    assert_eq!(
        allowed(&net),
        Some(Allowlist {
            hosts: vec![host("registry.npmjs.org", None)]
        })
    );

    // A changed allowlist reaches the running proxy at once.
    call(&core, set(json!([{"host": "crates.io"}])))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        allowed(&net),
        Some(Allowlist {
            hosts: vec![host("crates.io", None)]
        })
    );

    // Refusals reach every front end.
    let mut rx = core.subscribe();
    let reporter = net
        .running
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&s1)
        .map(|(_, r)| r.clone());
    reporter
        .unwrap_or_else(|| panic!("no reporter"))
        .refused("evil.example:443", "not in the allowlist");
    let p1 = crate::protocol::projects::ProjectId("p1".into());
    let logged = crate::services::AuditLog::list(audit.as_ref(), &p1, None, 100)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| {
            matches!(
                e.event,
                crate::protocol::audit::AuditEvent::NetworkRefused { .. }
            )
        })
        .count();
    assert_eq!(logged, 1);
    let refused = loop {
        match rx.recv().await {
            Ok(event @ Event::Netproxy(_)) => break Some(event),
            Ok(_) => {}
            Err(_) => break None,
        }
    };
    assert_eq!(
        refused,
        Some(Event::Netproxy(NetproxyEvent::Refused {
            project: crate::protocol::projects::ProjectId("p1".into()),
            session: s1.clone(),
            host: "evil.example:443".into(),
            reason: "not in the allowlist".into(),
        }))
    );

    call(
        &core,
        json!({"method": "agents.stop", "params": {"session": "s1"}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    loop {
        if let Ok(Event::Agents(AgentsEvent::Status { status, .. })) = rx.recv().await
            && status.is_ended()
        {
            break;
        }
    }
    assert_eq!(allowed(&net), None);

    // A resumed session gets a proxy of its own again, with today's list.
    call(
        &core,
        json!({"method": "agents.resume", "params": {"session": "s1", "prompt": "again"}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        allowed(&net),
        Some(Allowlist {
            hosts: vec![host("crates.io", None)]
        })
    );
    let specs = agent
        .started
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .len();
    assert_eq!(specs, 2);
}

#[tokio::test]
async fn a_session_whose_proxy_cannot_start_fails_instead_of_running_open() {
    let net = Arc::new(FakeNetProxy {
        fails: true,
        ..FakeNetProxy::default()
    });
    let agent = Arc::new(waits());
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(agent.clone())
        .netproxy(net)
        .build();
    call(
        &core,
        json!({"method": "projects.add", "params": {"root": crate::test_path!("/work/a")}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    let err = call(
        &core,
        json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "go"}}),
    )
    .await
    .err()
    .map(|e| e.code);
    assert_eq!(err, Some(ErrorCode::Unavailable));
    assert!(
        agent
            .started
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    );
    let Ok(Response::Agents(crate::protocol::agents::AgentsResponse::List(list))) = call(
        &core,
        json!({"method": "agents.list", "params": {"project": "p1"}}),
    )
    .await
    else {
        panic!("no list");
    };
    assert!(matches!(list[0].status, AgentStatus::Failed { .. }));
}

#[tokio::test]
async fn without_a_netproxy_sessions_run_open_and_the_base_is_empty() {
    let agent = Arc::new(waits());
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(agent.clone())
        .build();
    call(
        &core,
        json!({"method": "projects.add", "params": {"root": crate::test_path!("/work/a")}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    start(&core).await;
    let spec = agent.started.lock().unwrap_or_else(|e| e.into_inner())[0].clone();
    assert_eq!(spec.sandbox.network, ProfileNetwork::Open);
    let Ok(Response::Netproxy(NetproxyResponse::Allowlist(list))) = call(
        &core,
        json!({"method": "netproxy.get_allowlist", "params": {"project": "p1"}}),
    )
    .await
    else {
        panic!("no allowlist");
    };
    assert!(list.base.hosts.is_empty());
}
