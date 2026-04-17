//! Global application state — **single backend source of truth** for Omiga.
//!
//! Analogous to `src/state/AppStateStore.ts` + provider in Claude Code: persistence
//! (`SessionRepository`) and chat/runtime (`ChatState`) live together so Tauri commands,
//! logging, and future observability read one managed struct.

use crate::commands::CommandResult;
use crate::domain::chat_state::ChatState;
use crate::domain::integrations_catalog::IntegrationsCatalog;
use crate::domain::integrations_config::IntegrationsConfig;
use crate::domain::permissions::PermissionManager;
use crate::domain::persistence::SessionRepository;
use crate::domain::skills::SkillCacheMap;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};
use tauri::State;

/// TTL for the integrations config cache. Short enough to pick up edits quickly.
pub const INTEGRATIONS_CONFIG_CACHE_TTL: Duration = Duration::from_secs(30);

pub struct IntegrationsConfigCacheSlot {
    pub config: IntegrationsConfig,
    pub cached_at: Instant,
}

/// Process-wide state (managed once in `lib.rs`).
pub struct OmigaAppState {
    /// `SessionRepository` wraps a `SqlitePool` which is already `Send + Sync` and manages
    /// concurrent access internally (WAL mode).  No Mutex needed here.
    pub repo: Arc<SessionRepository>,
    pub chat: ChatState,
    /// Process start time for uptime in snapshots.
    pub started_at: Instant,
    /// Warm cache for [`crate::commands::integrations_settings::get_integrations_catalog`]:
    /// keyed by resolved project root (see `resolve_project_root` there).
    pub integrations_catalog_cache: Arc<StdMutex<HashMap<PathBuf, IntegrationsCatalog>>>,
    /// Process-level skill cache: keyed by resolved project root.
    /// Invalidated automatically via directory mtime stamps — no explicit flush needed.
    pub skill_cache: Arc<StdMutex<SkillCacheMap>>,
    /// Process-level integrations config cache: keyed by resolved project root.
    /// Short TTL (30 s) to pick up config edits promptly.
    pub integrations_config_cache: Arc<StdMutex<HashMap<PathBuf, IntegrationsConfigCacheSlot>>>,
    /// Permission manager for tool execution security.
    pub permission_manager: Arc<PermissionManager>,
}

impl OmigaAppState {
    pub fn new(repo: SessionRepository) -> Self {
        Self {
            repo: Arc::new(repo),
            chat: ChatState::default(),
            started_at: Instant::now(),
            integrations_catalog_cache: Arc::new(StdMutex::new(HashMap::new())),
            skill_cache: Arc::new(StdMutex::new(SkillCacheMap::default())),
            integrations_config_cache: Arc::new(StdMutex::new(HashMap::new())),
            permission_manager: Arc::new(PermissionManager::new()),
        }
    }
}

/// Serializable backend health snapshot for debugging / future UI “status” panels.
#[derive(Debug, Serialize)]
pub struct AppStateSnapshot {
    pub uptime_ms: u64,
    pub cached_sessions: usize,
    pub active_rounds: usize,
    pub pending_tool_calls: usize,
    pub llm_configured: bool,
    pub llm_provider: Option<String>,
}

/// Return current backend counters (global monitor hook).
#[tauri::command]
pub async fn get_app_state_snapshot(
    state: State<'_, OmigaAppState>,
) -> CommandResult<AppStateSnapshot> {
    let cached = state.chat.sessions.read().await.len();
    let active_rounds = state.chat.active_rounds.lock().await.len();
    let pending_tool_calls = state.chat.pending_tools.lock().await.len();
    let llm = state.chat.llm_config.lock().await;
    let llm_configured = llm.as_ref().map(|c| !c.api_key.is_empty()).unwrap_or(false);
    let llm_provider = llm.as_ref().map(|c| format!("{:?}", c.provider));
    drop(llm);

    Ok(AppStateSnapshot {
        uptime_ms: state.started_at.elapsed().as_millis() as u64,
        cached_sessions: cached,
        active_rounds,
        pending_tool_calls,
        llm_configured,
        llm_provider,
    })
}
