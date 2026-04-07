//! In-memory chat runtime state (LLM config, session cache, rounds, pending tools).
//! Lives in `OmigaAppState` alongside the DB repo — backend analogue of chat runtime in AppStateStore.

use crate::domain::session::{AgentTask, Session, TodoItem};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

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
        }
    }
}
