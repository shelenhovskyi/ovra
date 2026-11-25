//! Integration tests for the WebSocket endpoint.
#![cfg(feature = "ws")]
#![allow(clippy::unwrap_used)]

use futures_util::{SinkExt, StreamExt};
use ovra_core::Core;
use ovra_transport::ws::{Access, router};
use tokio_tungstenite::tungstenite::{self, Message, client::IntoClientRequest};

async fn spawn_server() -> std::net::SocketAddr {
    serve(Core::new("test")).await
}

async fn serve(core: Core) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let access = Access {
        token: "secret".into(),
        allowed_origins: vec![format!("http://{addr}")],
    };
    tokio::spawn(async move {
        axum::serve(listener, router(core, access)).await.unwrap();
    });
    addr
}

#[tokio::test]
async fn ready_then_get_info() {
    let addr = spawn_server().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws?token=secret"))
        .await
        .unwrap();

    let ready = ws.next().await.unwrap().unwrap();
    assert_eq!(
        ready.to_text().unwrap(),
        r#"{"event":{"type":"info.ready"}}"#
    );

    ws.send(Message::text(r#"{"id":1,"method":"info.get"}"#))
        .await
        .unwrap();
    let reply: serde_json::Value =
        serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["result"]["data"]["mode"], "test");
}

#[tokio::test]
async fn wrong_token_is_rejected() {
    let addr = spawn_server().await;
    let err = tokio_tungstenite::connect_async(format!("ws://{addr}/ws?token=nope"))
        .await
        .unwrap_err();
    assert!(matches!(err, tungstenite::Error::Http(r) if r.status() == 401));
}

#[tokio::test]
async fn foreign_origin_is_rejected() {
    let addr = spawn_server().await;
    let mut request = format!("ws://{addr}/ws?token=secret")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Origin", "http://evil.example".parse().unwrap());
    let err = tokio_tungstenite::connect_async(request).await.unwrap_err();
    assert!(matches!(err, tungstenite::Error::Http(r) if r.status() == 403));
}

/// The next frame that `pick` takes, skipping the others.
async fn next_where<S>(ws: &mut S, pick: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value
where
    S: futures_util::Stream<Item = Result<Message, tungstenite::Error>> + Unpin,
{
    loop {
        let frame = ws.next().await.unwrap().unwrap();
        let frame: serde_json::Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
        if pick(&frame) {
            return frame;
        }
    }
}

#[tokio::test]
async fn a_request_that_waits_holds_up_neither_events_nor_other_requests() {
    use ovra_core::fakes::{FakeBrowser, FakeNetProxy, MemoryProjects, ScriptedAgent, Step};
    use ovra_core::protocol::agents::{SandboxNetwork, SandboxReport};
    use std::sync::Arc;

    let agent = ScriptedAgent::new(vec![vec![Step::Say("on it")]]);
    *agent.sandbox.lock().unwrap() = Some(SandboxReport {
        name: "fake".into(),
        network: SandboxNetwork::Proxy,
        gaps: Vec::new(),
    });
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(agent))
        .netproxy(Arc::new(FakeNetProxy::default()))
        .browser(Arc::new(FakeBrowser::default()))
        .build();
    for request in [
        serde_json::json!({"method": "projects.add", "params": {"root": ovra_core::test_path!("/work/a")}}),
        serde_json::json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "look"}}),
    ] {
        core.handle(serde_json::from_value(request).unwrap())
            .await
            .unwrap();
    }
    let addr = serve(core).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws?token=secret"))
        .await
        .unwrap();
    let open = r#"{"id":1,"method":"browser.open","params":{"session":"s1","url":"https://example.com/"}}"#;
    ws.send(Message::text(open)).await.unwrap();
    // The open waits for the person, who hears of it and answers.
    next_where(&mut ws, |f| {
        f["event"]["type"] == "browser.navigation_asked"
    })
    .await;
    let answer = r#"{"id":2,"method":"browser.answer_navigation","params":{"page":"pg1","origin":"https://example.com","allow":true}}"#;
    ws.send(Message::text(answer)).await.unwrap();
    let answered = next_where(&mut ws, |f| f["id"] == 2).await;
    assert_eq!(answered["result"]["type"], "ok");
    let opened = next_where(&mut ws, |f| f["id"] == 1).await;
    assert_eq!(opened["result"]["data"]["url"], "https://example.com/");
}
