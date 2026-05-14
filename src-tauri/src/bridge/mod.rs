//! IDE Bridge — local WebSocket server for VS Code / JetBrains extensions.
//!
//! IDE extensions connect on `127.0.0.1:<port>` (default 7777), authenticate with a JWT,
//! then exchange `IdeMessage` JSON frames to send code selections and receive diffs.

pub mod auth;
pub mod protocol;
pub mod server;
pub mod state;
