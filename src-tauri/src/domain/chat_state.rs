//! In-memory chat runtime state (LLM config, session cache, rounds, pending tools).
//! Lives in `OmigaAppState` alongside the DB repo — backend analogue of chat runtime in AppStateStore.

use crate::domain::session::{AgentTask, Session, TodoItem};
use crate::domain::tools::ToolSchema;
use crate::domain::mcp::connection_manager::GlobalMcpManager;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock, oneshot};

/// Cached permission deny entries for one project root.
pub struct PermissionDenyCache {
    pub entries: Vec<crate::domain::tool_permission_rules::DenyRuleEntry>,
    pub cached_at: Instant,
}

/// TTL for permission deny rules. Short enough to pick up config edits quickly.
pub const PERMISSION_DENY_CACHE_TTL: Duration = Duration::from_secs(30);

/// Cached result of `discover_mcp_tool_schemas` for one project root.
pub struct McpToolCache {
    pub schemas: Vec<ToolSchema>,
    pub cached_at: Instant,
}

/// TTL for MCP tool schema cache. MCP server tool lists rarely change during a session.
pub const MCP_TOOL_CACHE_TTL: Duration = Duration::from_secs(300); // 5 minutes

/// Resolver for a blocked `ask_user_question` tool call (chat path only).
pub struct AskUserWaiter {
    pub tx: oneshot::Sender<Result<serde_json::Value, String>>,
}

/// Blocked until the user approves or denies in the chat UI (`permission_approve` / `permission_deny`).
/// Map key: permission `request_id` from the backend.
pub struct PermissionToolWaiter {
    pub tx: oneshot::Sender<Result<(), String>>,
    pub session_id: String,
    pub message_id: String,
}

/// Active chat state with optimized in-memory caching.
/// Database remains the single source of truth for persistence.
pub struct ChatState {
    /// LLM API configuration (includes provider, api_key, model, etc.)
    pub llm_config: Mutex<Option<crate::llm::LlmConfig>>,
    /// `omiga.yaml` `providers` map key for the entry currently driving [`Self::llm_config`].
    /// Used so Settings only marks one row as "In use" when multiple entries share the same provider+model.
    pub active_provider_entry_name: Mutex<Option<String>>,
    /// User-configured Brave Search API key (Settings); overrides env when set.
    pub brave_search_api_key: Mutex<Option<String>>,
    /// In-memory session cache for O(1) lookup by session_id
    pub sessions: Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    /// Active conversation rounds for cancellation tracking
    pub active_rounds: Arc<Mutex<HashMap<String, RoundCancellationState>>>,
    pub pending_tools: Arc<Mutex<HashMap<String, PendingToolCall>>>,
    /// Key: `session_id\\x1fmessage_id\\x1ftool_use_id` — blocked until user submits or cancels.
    pub ask_user_waiters: Arc<Mutex<HashMap<String, AskUserWaiter>>>,
    /// Key: permission `request_id` — blocked until user approves/denies or cancels the round.
    pub permission_tool_waiters: Arc<Mutex<HashMap<String, PermissionToolWaiter>>>,
    /// MCP tool schema cache keyed by project root. Avoids re-spawning MCP server
    /// processes on every `send_message` call (primary cause of slow first response).
    pub mcp_tool_cache: Arc<Mutex<HashMap<PathBuf, McpToolCache>>>,
    /// Managed MCP connection pool with session boundaries and lifecycle management.
    /// 
    /// Features:
    /// - Session tracking: connections are tagged with session ID, stdio connections
    ///   are refreshed on session change to avoid zombie processes
    /// - Idle cleanup: connections idle > 5 minutes are automatically closed
    /// - Config reload: new sessions pick up configuration changes
    /// - Health checking: stdio process liveness is monitored
    pub mcp_manager: Arc<GlobalMcpManager>,
    /// Cached permission deny rules keyed by project root. Avoids re-reading 4 settings
    /// files synchronously on every `send_message` call.
    pub permission_deny_cache: Arc<Mutex<HashMap<PathBuf, PermissionDenyCache>>>,
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
            active_provider_entry_name: Mutex::new(None),
            brave_search_api_key: Mutex::new(None),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            active_rounds: Arc::new(Mutex::new(HashMap::new())),
            pending_tools: Arc::new(Mutex::new(HashMap::new())),
            ask_user_waiters: Arc::new(Mutex::new(HashMap::new())),
            permission_tool_waiters: Arc::new(Mutex::new(HashMap::new())),
            mcp_tool_cache: Arc::new(Mutex::new(HashMap::new())),
            mcp_manager: Arc::new(GlobalMcpManager::new()),
            permission_deny_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
