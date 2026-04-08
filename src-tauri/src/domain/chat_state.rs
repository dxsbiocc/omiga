//! In-memory chat runtime state (LLM config, session cache, rounds, pending tools).
//! Lives in `OmigaAppState` alongside the DB repo — backend analogue of chat runtime in AppStateStore.

use crate::domain::mcp_client::McpLiveConnection;
use crate::domain::session::{AgentTask, Session, TodoItem};
use crate::domain::tools::ToolSchema;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

/// Cached result of `discover_mcp_tool_schemas` for one project root.
pub struct McpToolCache {
    pub schemas: Vec<ToolSchema>,
    pub cached_at: Instant,
}

/// TTL for MCP tool schema cache. MCP server tool lists rarely change during a session.
pub const MCP_TOOL_CACHE_TTL: Duration = Duration::from_secs(300); // 5 minutes

/// Active chat state with optimized in-memory caching.
/// Database remains the single source of truth for persistence.
pub struct ChatState {
    /// LLM API configuration (includes provider, api_key, model, etc.)
    pub llm_config: Mutex<Option<crate::llm::LlmConfig>>,
    /// User-configured Brave Search API key (Settings); overrides env when set.
    pub brave_search_api_key: Mutex<Option<String>>,
    /// In-memory session cache for O(1) lookup by session_id
    pub sessions: Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    /// Active conversation rounds for cancellation tracking
    pub active_rounds: Arc<Mutex<HashMap<String, RoundCancellationState>>>,
    pub pending_tools: Arc<Mutex<HashMap<String, PendingToolCall>>>,
    /// MCP tool schema cache keyed by project root. Avoids re-spawning MCP server
    /// processes on every `send_message` call (primary cause of slow first response).
    pub mcp_tool_cache: Arc<Mutex<HashMap<PathBuf, McpToolCache>>>,
    /// Persistent MCP connections keyed by `"<project_root>/<server_name>"`.
    /// Each entry keeps a live stdio process / HTTP session alive so tool calls
    /// skip the spawn+handshake overhead (~0.5–3 s per server per call).
    pub mcp_connections: Arc<Mutex<HashMap<String, McpLiveConnection>>>,
}

/// Runtime state for an active session. Chat transcript is persisted via messages;
/// `todos` / `agent_tasks` are mirrored to `session_tool_state` (SQLite) on each tool round.
#[derive(Debug)]
pub struct SessionRuntimeState {
    pub session: Session,
    pub active_round_ids: Vec<String>,
    /// Session-scoped todo list for `todo_write` (persisted per turn in `session_tool_state`).
    pub todos: Arc<tokio::sync::Mutex<Vec<TodoItem>>>,
    /// Session-scoped V2 tasks (persisted per turn in `session_tool_state`).
    pub agent_tasks: Arc<tokio::sync::Mutex<Vec<AgentTask>>>,
    /// `true` after `EnterPlanMode` until `ExitPlanMode` — aligns with TS `permissionMode === 'plan'`
    /// (sub-agents may keep `ExitPlanMode` in the tool list when this is true).
    pub plan_mode: Arc<tokio::sync::Mutex<bool>>,
}

/// Cancellation state for an active conversation round
#[derive(Debug, Clone)]
pub struct RoundCancellationState {
    pub round_id: String,
    pub message_id: String,
    pub session_id: String,
    pub cancelled: Arc<RwLock<bool>>,
}

/// A pending tool call being collected from the stream
#[derive(Debug)]
pub struct PendingToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Vec<String>,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            llm_config: Mutex::new(None),
            brave_search_api_key: Mutex::new(None),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            active_rounds: Arc::new(Mutex::new(HashMap::new())),
            pending_tools: Arc::new(Mutex::new(HashMap::new())),
            mcp_tool_cache: Arc::new(Mutex::new(HashMap::new())),
            mcp_connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
