//! Tauri commands for the IDE Bridge.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::bridge::{
    auth,
    server::{start_bridge, stop_bridge},
    state::{BridgeState, CodeSelection},
};

/// Payload returned by [`bridge_status`].
#[derive(Serialize)]
pub struct BridgeStatusInfo {
    pub running: bool,
    pub port: u16,
    pub connection_count: usize,
    pub setup_key: Option<String>,
}

/// Payload returned by [`bridge_get_last_selection`] (mirrors `CodeSelection`
/// but derives `Serialize` for the frontend).
#[derive(Serialize)]
pub struct CodeSelectionPayload {
    pub file: String,
    pub selection: String,
    pub language: String,
    pub start_line: u32,
    pub end_line: u32,
}

impl From<CodeSelection> for CodeSelectionPayload {
    fn from(cs: CodeSelection) -> Self {
        Self {
            file: cs.file,
            selection: cs.selection,
            language: cs.language,
            start_line: cs.range.start_line,
            end_line: cs.range.end_line,
        }
    }
}

/// Start the IDE Bridge WebSocket server.
///
/// Returns the port number on success.
#[tauri::command]
pub async fn bridge_start(state: State<'_, Arc<BridgeState>>) -> Result<u16, String> {
    if state.is_running() {
        return Ok(state.port);
    }

    let state_arc = Arc::clone(&state);
    tokio::spawn(async move {
        start_bridge(state_arc).await;
    });

    // Give the listener a moment to bind before returning.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    tracing::info!("bridge_start: server spawned on port {}", state.port);
    Ok(state.port)
}

/// Stop the IDE Bridge WebSocket server.
#[tauri::command]
pub async fn bridge_stop(state: State<'_, Arc<BridgeState>>) -> Result<(), String> {
    stop_bridge(&state);
    Ok(())
}

/// Return current bridge status.
#[tauri::command]
pub async fn bridge_status(state: State<'_, Arc<BridgeState>>) -> Result<BridgeStatusInfo, String> {
    let connection_count = state.connections.lock().await.len();
    let setup_key = state.setup_key.lock().await.clone();

    Ok(BridgeStatusInfo {
        running: state.is_running(),
        port: state.port,
        connection_count,
        setup_key,
    })
}

/// Generate (and store) a one-time setup key for IDE extensions to use.
///
/// Format: `omiga://bridge?port=<port>&token=<jwt>`
#[tauri::command]
pub async fn bridge_generate_setup_key(
    state: State<'_, Arc<BridgeState>>,
) -> Result<String, String> {
    let token = auth::generate_token(&state.secret);
    let key = format!("omiga://bridge?port={}&token={}", state.port, token);

    *state.setup_key.lock().await = Some(key.clone());
    tracing::info!("bridge_generate_setup_key: new setup key generated");

    Ok(key)
}

/// Return the most-recent code selection sent by any connected IDE client.
#[tauri::command]
pub async fn bridge_get_last_selection(
    state: State<'_, Arc<BridgeState>>,
) -> Result<Option<CodeSelectionPayload>, String> {
    let guard = state.last_selection.lock().await;
    Ok(guard.clone().map(CodeSelectionPayload::from))
}
