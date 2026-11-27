//! e2e only (`fake-agent` builds, `--fake-ssh`): the bridge a remote window
//! uses, served at `/ws/ssh/<id>` with the token and `Origin` checks of
//! `/ws`. Instead of `ssh` it runs this daemon as
//! `--stdio --fake-agent` on a data folder of the machine's own
//! (`docs/specs/ssh-remotes.md`, "e2e"). Release builds have no such
//! endpoint.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use ovra_core::ssh::HostId;
use ovra_transport::ws::Access;

#[derive(Clone)]
struct Bridge {
    access: Arc<Access>,
    /// `<data>/fake-ssh`: each machine's own data folder is in it.
    machines: PathBuf,
}

#[derive(serde::Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

/// `/ws/ssh/{id}`.
pub(crate) fn routes(access: Access, data_dir: &std::path::Path) -> Router {
    Router::new()
        .route("/ws/ssh/{id}", get(upgrade))
        .with_state(Bridge {
            access: Arc::new(access),
            machines: data_dir.join("fake-ssh"),
        })
}

async fn upgrade(
    State(bridge): State<Bridge>,
    Path(id): Path<String>,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if let Err(refused) = bridge.access.check(query.token.as_deref(), &headers) {
        return refused.into_response();
    }
    let Ok(id) = HostId::parse(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let data = bridge.machines.join(id.as_str());
    ws.on_upgrade(move |socket| run(socket, data))
}

/// Bridges `socket` to a fake remote core until either side ends. A
/// failure closes the socket with its reason as the close reason, never
/// as a frame.
async fn run(socket: WebSocket, data: PathBuf) {
    let mut socket = socket;
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut command = tokio::process::Command::new(exe);
    command
        .args(["--stdio", "--fake-agent", "--data-dir"])
        .arg(&data);
    let mut link = match ovra_ssh::fake(command).await {
        Ok(link) => link,
        Err(failure) => return close(&mut socket, failure.reason).await,
    };
    let sender = link.sender();
    let (to_window, mut from_window) = socket.split();
    let to_window = tokio::sync::Mutex::new(to_window);
    // Each way in its own future: a remote that fills its queue never
    // stops the window's frames from going out, nor the other way.
    let outgoing = async {
        while let Some(Ok(message)) = from_window.next().await {
            match message {
                Message::Text(text) => {
                    if let Err(failure) = sender.send(text.as_str()).await {
                        return Some(failure.reason);
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        None
    };
    let incoming = async {
        loop {
            match link.recv().await {
                Ok(frame) => {
                    let sent = to_window
                        .lock()
                        .await
                        .send(Message::Text(frame.into()))
                        .await;
                    if sent.is_err() {
                        return;
                    }
                }
                Err(failure) => {
                    let _ = to_window
                        .lock()
                        .await
                        .send(close_message(failure.reason))
                        .await;
                    return;
                }
            }
        }
    };
    let refused = tokio::select! {
        refused = outgoing => refused,
        () = incoming => None,
    };
    if let Some(reason) = refused {
        let _ = to_window.lock().await.send(close_message(reason)).await;
    }
}

fn close_message(reason: ovra_ssh::Reason) -> Message {
    Message::Close(Some(CloseFrame {
        code: 4000,
        reason: reason.as_str().into(),
    }))
}

async fn close(socket: &mut WebSocket, reason: ovra_ssh::Reason) {
    let _ = socket.send(close_message(reason)).await;
}
