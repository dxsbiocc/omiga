//! In-memory chat runtime state (LLM config, session cache, rounds, pending tools).
//! Lives in `OmigaAppState` alongside the DB repo — backend analogue of chat runtime in AppStateStore.

use crate::domain::mcp::connection_manager::GlobalMcpManager;
use crate::domain::session::{AgentTask, Session, TodoItem};
use crate::domain::tools::{env_store::EnvStore, ToolSchema, WebSearchApiKeys};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

/// Cached `omiga.yaml` content.  Loaded once on first use, invalidated whenever
/// the config file is written so callers never see stale data.
pub type CachedConfigFile = Arc<Mutex<Option<Arc<crate::llm::config::LlmConfigFile>>>>;

/// Cached permission deny entries for one project root.
pub struct PermissionDenyCache {
    pub entries: Vec<crate::domain::permissions::DenyRuleEntry>,
    pub cached_at: Instant,
}

/// TTL for permission deny rules. Short enough to pick up config edits quickly.
pub const PERMISSION_DENY_CACHE_TTL: Duration = Duration::from_secs(30);

/// Cached result of `discover_mcp_tool_schemas` for one project root.
pub struct McpToolCache {
    pub schemas: Vec<ToolSchema>,
    pub cached_at: Instant,
    pub config_signature: String,
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
    pub context: crate::domain::permissions::PermissionContext,
}

/// Active chat state with optimized in-memory caching.
/// Database remains the single source of truth for persistence.
pub struct ChatState {
    /// LLM API configuration (includes provider, api_key, model, etc.)
    pub llm_config: Mutex<Option<crate::llm::LlmConfig>>,
    /// `omiga.yaml` `providers` map key for the entry currently driving [`Self::llm_config`].
    /// Used so Settings only marks one row as "In use" when multiple entries share the same provider+model.
    pub active_provider_entry_name: Mutex<Option<String>>,
    /// User-configured API keys for built-in `search` (Settings override env when set).
    pub web_search_api_keys: Mutex<WebSearchApiKeys>,
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
    /// In-memory cache for the parsed `omiga.yaml` config file.
    /// Eliminates 12+ blocking stat() calls + file read + YAML parse on every session
    /// switch that hits the provider-restore code path.  Cleared whenever the config
    /// file is written so callers never see stale data.
    pub cached_config_file: CachedConfigFile,
    /// Active agent orchestrations keyed by session_id → map of (orch_id → CancellationToken).
    /// One session may have multiple concurrent orchestrations; each gets its own entry.
    /// Populated by `run_agent_schedule`, consumed by `cancel_agent_schedule`.
    pub active_orchestrations:
        Arc<Mutex<HashMap<String, HashMap<String, tokio_util::sync::CancellationToken>>>>,
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
    /// `local` | `ssh` | `sandbox` — matches composer [`executionEnvironment`](SendMessageRequest) from the UI.
    pub execution_environment: String,
    /// Selected SSH server name; used when `execution_environment == "ssh"`.
    pub ssh_server: Option<String>,
    /// `modal` | `daytona` | `docker` | `singularity` — composer sandbox backend; used when `execution_environment == "sandbox"`.
    pub sandbox_backend: String,
    /// `"none"` | `"conda"` | `"venv"` | `"pyenv"` — local virtual env type.
    pub local_venv_type: String,
    /// Conda env name, venv directory path, or pyenv version string.
    pub local_venv_name: String,
    /// Session-scoped environment cache — shared across all tool calls in this session.
    /// Created once per session; shutdown on session teardown.
    pub env_store: EnvStore,
    /// File artifacts written or edited by AI tools during this session.
    pub artifact_registry: crate::domain::session::artifacts::ArtifactRegistry,
}

/// Cancellation state for an active conversation round
#[derive(Debug, Clone)]
pub struct RoundCancellationState {
    pub round_id: String,
    pub message_id: String,
    pub session_id: String,
    pub cancelled: Arc<RwLock<bool>>,
    /// Cancels in-flight tools (foreground/background bash, fetch, …) when the user stops the round.
    pub round_cancel: CancellationToken,
}

/// A pending tool call being collected from the stream
#[derive(Debug)]
pub struct PendingToolCall {
    pub id: String,
    pub original_name: String,
    pub name: String,
    pub arguments: Vec<String>,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            llm_config: Mutex::new(None),
            active_provider_entry_name: Mutex::new(None),
            web_search_api_keys: Mutex::new(WebSearchApiKeys::default()),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            active_rounds: Arc::new(Mutex::new(HashMap::new())),
            pending_tools: Arc::new(Mutex::new(HashMap::new())),
            ask_user_waiters: Arc::new(Mutex::new(HashMap::new())),
            permission_tool_waiters: Arc::new(Mutex::new(HashMap::new())),
            mcp_tool_cache: Arc::new(Mutex::new(HashMap::new())),
            mcp_manager: Arc::new(GlobalMcpManager::new()),
            permission_deny_cache: Arc::new(Mutex::new(HashMap::new())),
            cached_config_file: Arc::new(Mutex::new(None)),
            active_orchestrations: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
