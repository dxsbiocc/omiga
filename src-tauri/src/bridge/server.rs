//! WebSocket server for the IDE Bridge.
//!
//! Listens on `127.0.0.1:<port>`, performs a JWT handshake on every new
//! connection, then dispatches `IdeMessage` frames from connected IDE clients.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use futures::SinkExt;
use futures::StreamExt;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use uuid::Uuid;

use crate::bridge::{
    auth,
    protocol::IdeMessage,
    state::{BridgeState, CodeSelection, ConnectionInfo},
};

/// Internal auth frame sent by the IDE as the very first message.
#[derive(serde::Deserialize)]
struct AuthFrame {
    #[serde(rename = "type")]
    msg_type: String,
    token: String,
}

/// Start the bridge server and loop accepting connections until `running` is set to `false`.
///
/// This function is designed to be spawned with `tokio::spawn`.
pub async fn start_bridge(state: Arc<BridgeState>) {
    let addr = format!("127.0.0.1:{}", state.port);

    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => {
            tracing::info!("IDE Bridge listening on ws://{}", addr);
            state.running.store(true, Ordering::SeqCst);
            l
        }
        Err(e) => {
            tracing::error!("IDE Bridge failed to bind {}: {}", addr, e);
            return;
        }
    };

    while state.running.load(Ordering::SeqCst) {
        // Use a short timeout so we can check `running` frequently.
        let accept =
            tokio::time::timeout(std::time::Duration::from_millis(500), listener.accept()).await;

        match accept {
            Ok(Ok((stream, peer))) => {
                tracing::info!("IDE Bridge: new TCP connection from {}", peer);
                let state_clone = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, state_clone).await {
                        tracing::warn!("IDE Bridge connection error: {}", e);
                    }
                });
            }
            Ok(Err(e)) => {
                tracing::error!("IDE Bridge accept error: {}", e);
            }
            Err(_timeout) => {
                // No new connection within the poll window — loop to re-check `running`.
            }
        }
    }

    tracing::info!("IDE Bridge server stopped");
}

/// Signal the server to stop accepting new connections.
pub fn stop_bridge(state: &Arc<BridgeState>) {
    state.running.store(false, Ordering::SeqCst);
    tracing::info!("IDE Bridge stop requested");
}

/// Handle a single WebSocket connection: authenticate, then process messages.
async fn handle_connection(
    stream: tokio::net::TcpStream,
    state: Arc<BridgeState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_stream = accept_async(stream).await?;
    let (mut writer, mut reader) = ws_stream.split();

    // ── Authentication handshake ────────────────────────────────────────────
    let first_msg = match reader.next().await {
        Some(Ok(msg)) => msg,
        _ => {
            send_error(&mut writer, "Expected auth frame").await;
            return Ok(());
        }
    };

    let client_type = match authenticate(&first_msg, &state.secret) {
        Some(ct) => ct,
        None => {
            send_error(&mut writer, "Authentication failed").await;
            return Ok(());
        }
    };

    // Register connection.
    let conn_id = Uuid::new_v4().to_string();
    {
        let mut conns = state.connections.lock().await;
        conns.insert(
            conn_id.clone(),
            ConnectionInfo {
                client_type: client_type.clone(),
                connected_at: chrono::Utc::now(),
            },
        );
    }
    tracing::info!(
        "IDE Bridge: authenticated client '{}' ({})",
        client_type,
        conn_id
    );

    // ── Message loop ────────────────────────────────────────────────────────
    while let Some(raw) = reader.next().await {
        match raw {
            Ok(Message::Text(text)) => {
                if let Err(e) = process_message(&text, &state, &mut writer).await {
                    tracing::warn!("IDE Bridge message error: {}", e);
                    break;
                }
            }
            Ok(Message::Close(_)) => {
                tracing::info!("IDE Bridge: client {} closed connection", conn_id);
                break;
            }
            Ok(_) => {} // binary / ping frames — ignore
            Err(e) => {
                tracing::warn!("IDE Bridge WS error: {}", e);
                break;
            }
        }
    }

    // Deregister.
    state.connections.lock().await.remove(&conn_id);
    tracing::info!("IDE Bridge: connection {} removed", conn_id);
    Ok(())
}

/// Verify the auth frame and return the detected client type, or `None` on failure.
fn authenticate(msg: &Message, secret: &str) -> Option<String> {
    let text = match msg {
        Message::Text(t) => t,
        _ => return None,
    };

    let frame: AuthFrame = serde_json::from_str(text).ok()?;
    if frame.msg_type != "auth" {
        return None;
    }
    if !auth::verify_token(&frame.token, secret) {
        return None;
    }

    // Best-effort client detection from a custom "client" field embedded by
    // the IDE extension — fall back to "unknown".
    let value: serde_json::Value = serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
    let client_type = value
        .get("client")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    Some(client_type)
}

/// Dispatch a single text frame from an authenticated IDE client.
async fn process_message(
    text: &str,
    state: &Arc<BridgeState>,
    writer: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let msg: IdeMessage = serde_json::from_str(text)?;

    match msg {
        IdeMessage::CodeSelection {
            file,
            selection,
            language,
            range,
        } => {
            tracing::info!("IDE Bridge: code selection from '{}'", file);
            let mut guard = state.last_selection.lock().await;
            *guard = Some(CodeSelection {
                file,
                selection,
                language,
                range,
            });
        }

        IdeMessage::RequestDiff { session_id } => {
            tracing::info!("IDE Bridge: diff requested for session '{}'", session_id);
            // Placeholder: a real implementation would look up the session diff.
            let response = IdeMessage::Error {
                message: "Diff generation not yet implemented".to_string(),
            };
            send_message(writer, &response).await;
        }

        IdeMessage::Ping => {
            send_message(writer, &IdeMessage::Pong).await;
        }

        other => {
            tracing::warn!("IDE Bridge: unexpected message from IDE: {:?}", other);
        }
    }

    Ok(())
}

/// Serialise and send an `IdeMessage` text frame.
async fn send_message(
    writer: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    msg: &IdeMessage,
) {
    if let Ok(json) = serde_json::to_string(msg) {
        let _ = writer.send(Message::Text(json.into())).await;
    }
}

/// Send a plain error text frame.
async fn send_error(
    writer: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    message: &str,
) {
    send_message(
        writer,
        &IdeMessage::Error {
            message: message.to_string(),
        },
    )
    .await;
}
