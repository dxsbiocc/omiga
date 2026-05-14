//! Wire protocol types for the IDE Bridge WebSocket connection.

use serde::{Deserialize, Serialize};

/// A range within a source file identified by (inclusive) line numbers.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SelectionRange {
    pub start_line: u32,
    pub end_line: u32,
}

/// All messages that flow over the IDE Bridge WebSocket.
///
/// The `type` field is the JSON discriminant (`serde(tag = "type")`).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IdeMessage {
    // ── IDE → Omiga ──────────────────────────────────────────────────────────

    /// IDE sends a code selection for Omiga to operate on.
    CodeSelection {
        file: String,
        selection: String,
        language: String,
        range: SelectionRange,
    },

    /// IDE requests a diff for an ongoing Omiga session.
    RequestDiff { session_id: String },

    /// Keep-alive ping.
    Ping,

    // ── Omiga → IDE ──────────────────────────────────────────────────────────

    /// Omiga returns a suggested diff for a file.
    DiffResult {
        file: String,
        original: String,
        modified: String,
    },

    /// Omiga asks the IDE to display a permission prompt.
    PermissionRequest {
        tool: String,
        description: String,
        request_id: String,
    },

    /// Response to a `Ping`.
    Pong,

    /// Error message.
    Error { message: String },
}
