//! Shared state for the IDE Bridge.  Pure Rust — no Tauri dependency.

use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Mutex;

use crate::bridge::protocol::SelectionRange;

/// Information about a single IDE client connection.
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// "vscode" | "jetbrains" | "unknown"
    pub client_type: String,
    pub connected_at: chrono::DateTime<chrono::Utc>,
}

/// A code selection forwarded from the IDE.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodeSelection {
    pub file: String,
    pub selection: String,
    pub language: String,
    pub range: SelectionRange,
}

/// All mutable state owned by the bridge, safe to share across async tasks.
pub struct BridgeState {
    /// Port the WebSocket server listens on.
    pub port: u16,
    /// HS256 secret used to sign / verify JWTs.
    pub secret: String,
    /// Set to `false` to signal the server task to stop.
    pub running: Arc<AtomicBool>,
    /// Currently authenticated connections keyed by a UUID.
    pub connections: Arc<Mutex<HashMap<String, ConnectionInfo>>>,
    /// Most-recent code selection sent by any IDE client.
    pub last_selection: Arc<Mutex<Option<CodeSelection>>>,
    /// One-time setup key (cleared after first use or after the server stops).
    pub setup_key: Arc<Mutex<Option<String>>>,
}

impl BridgeState {
    /// Create a new `BridgeState` with a random 32-byte hex secret.
    pub fn new() -> Self {
        use rand::Rng;
        let secret: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(64)
            .map(char::from)
            .collect();

        Self {
            port: 7777,
            secret,
            running: Arc::new(AtomicBool::new(false)),
            connections: Arc::new(Mutex::new(HashMap::new())),
            last_selection: Arc::new(Mutex::new(None)),
            setup_key: Arc::new(Mutex::new(None)),
        }
    }

    /// Convenience: is the server currently running?
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Default for BridgeState {
    fn default() -> Self {
        Self::new()
    }
}
