//! Integration tests for the WebSocket endpoint.
#![cfg(feature = "ws")]
#![allow(clippy::unwrap_used)]

use futures_util::{SinkExt, StreamExt};
use ovra_core::Core;
use ovra_transport::ws::{Access, router};
use tokio_tungstenite::tungstenite::{self, Message, client::IntoClientRequest};

async fn spawn_server() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let access = Access {
        token: "secret".into(),
        allowed_origins: vec![format!("http://{addr}")],
    };
    tokio::spawn(async move {
        axum::serve(listener, router(Core::new("test"), access))
            .await
            .unwrap();
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
    assert_eq!(ready.to_text().unwrap(), r#"{"event":{"type":"ready"}}"#);

    ws.send(Message::text(r#"{"id":1,"method":"get_info"}"#))
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
