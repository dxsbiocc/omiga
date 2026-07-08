use super::*;

/// Max tool rounds inside one `Agent` sub-session (nested Agent calls are blocked separately).
pub(super) const MAX_SUBAGENT_TOOL_ROUNDS: usize = 50;

/// Max `execute_tool_calls` depth for nested `Agent` (main session = 0). TS allows deep nesting when `USER_TYPE=ant`.
pub(super) const MAX_SUBAGENT_EXECUTE_DEPTH: u8 = 8;

/// LLM + stream state needed for the `Agent` tool to run an isolated sub-session (same API key as main chat).
#[derive(Clone)]
pub(crate) struct AgentLlmRuntime {
    pub(super) llm_config: LlmConfig,
    pub(super) round_id: String,
    pub(super) cancel_flag: Arc<RwLock<bool>>,
    pub(super) pending_tools: Arc<Mutex<HashMap<String, PendingToolCall>>>,
    pub(super) repo: Arc<crate::domain::persistence::SessionRepository>,
    /// Same `Arc` as [`SessionRuntimeState::plan_mode`] — sub-agent filter reads plan mode for `ExitPlanMode` parity.
    pub(super) plan_mode_flag: Option<Arc<Mutex<bool>>>,
    /// `USER_TYPE=ant` — nested `Agent` allowed (`ALL_AGENT_DISALLOWED_TOOLS` omits Agent).
    pub(super) allow_nested_agent: bool,
    /// Same token as [`RoundCancellationState::round_cancel`] for main chat; stops foreground/background bash on cancel.
    pub(super) round_cancel: tokio_util::sync::CancellationToken,
    /// `local` | `ssh` | `sandbox` — from [`SessionRuntimeState::execution_environment`].
    pub(super) execution_environment: String,
    /// Selected SSH server name; from [`SessionRuntimeState::ssh_server`].
    pub(super) ssh_server: Option<String>,
    /// `modal` | `daytona` | `docker` | `singularity` — from [`SessionRuntimeState::sandbox_backend`].
    pub(super) sandbox_backend: String,
    /// Local virtual env type: `"none"` | `"conda"` | `"venv"` | `"pyenv"`.
    pub(super) local_venv_type: String,
    /// Conda env name, venv directory path, or pyenv version string.
    pub(super) local_venv_name: String,
    /// Session-scoped environment cache — shared across all tool calls in this round.
    pub(super) env_store: crate::domain::tools::env_store::EnvStore,
    /// Resolved runtime constraint configuration (project + session overrides).
    pub(super) runtime_constraints_config:
        crate::domain::runtime_constraints::ResolvedRuntimeConstraintConfig,
}

impl AgentLlmRuntime {
    pub(crate) fn round_id(&self) -> &str {
        &self.round_id
    }

    pub(crate) fn repo(&self) -> &Arc<crate::domain::persistence::SessionRepository> {
        &self.repo
    }

    /// Build a runtime from app state, optionally inheriting execution environment from a parent
    /// session. If `session_id` is `Some`, the session's `execution_environment`, `ssh_server`,
    /// `sandbox_backend`, `local_venv_*`, and `env_store` are copied; otherwise defaults apply.
    pub(crate) async fn from_app(
        app: &tauri::AppHandle,
        session_id: Option<&str>,
    ) -> Result<Self, String> {
        use crate::app_state::OmigaAppState;
        use tauri::Manager;
        let state = app
            .try_state::<OmigaAppState>()
            .ok_or("OmigaAppState not available")?;
        let llm_config = {
            let guard = state.chat.llm_config.lock().await;
            guard
                .clone()
                .ok_or("LLM not configured — set an API key first")?
        };
        if llm_config.api_key.is_empty() {
            return Err("API key is empty".to_string());
        }

        let (
            execution_environment,
            ssh_server,
            sandbox_backend,
            local_venv_type,
            local_venv_name,
            env_store,
        ) = {
            let sessions = state.chat.sessions.read().await;
            let s = session_id.and_then(|id| sessions.get(id));
            (
                s.map(|x| x.execution_environment.clone())
                    .unwrap_or_else(|| "local".to_string()),
                s.and_then(|x| x.ssh_server.clone()),
                s.map(|x| x.sandbox_backend.clone())
                    .unwrap_or_else(|| "docker".to_string()),
                s.map(|x| x.local_venv_type.clone()).unwrap_or_default(),
                s.map(|x| x.local_venv_name.clone()).unwrap_or_default(),
                s.map(|x| x.env_store.clone()).unwrap_or_default(),
            )
        };

        Ok(Self {
            llm_config,
            round_id: uuid::Uuid::new_v4().to_string(),
            cancel_flag: Arc::new(RwLock::new(false)),
            pending_tools: state.chat.pending_tools.clone(),
            repo: state.repo.clone(),
            plan_mode_flag: None,
            allow_nested_agent: false,
            round_cancel: tokio_util::sync::CancellationToken::new(),
            execution_environment,
            ssh_server,
            sandbox_backend,
            local_venv_type,
            local_venv_name,
            env_store,
            runtime_constraints_config:
                crate::domain::runtime_constraints::ResolvedRuntimeConstraintConfig::default(),
        })
    }

    /// Load runtime constraint config from the project's omiga.yaml and apply it.
    /// Call this after `from_app()` when the project root and session ID are known.
    pub(crate) fn with_runtime_context(
        mut self,
        project_root: &std::path::Path,
        session_id: &str,
    ) -> Self {
        let session_cfg = crate::domain::session::load_session_config(session_id);
        self.runtime_constraints_config =
            crate::domain::runtime_constraints::resolve_runtime_constraint_config(
                project_root,
                session_cfg.runtime_constraints.as_ref(),
            );
        self
    }
}

pub(super) struct ActiveRoundCleanup {
    active_rounds: Arc<Mutex<HashMap<String, RoundCancellationState>>>,
    message_id: String,
}

impl ActiveRoundCleanup {
    pub(super) fn new(
        active_rounds: Arc<Mutex<HashMap<String, RoundCancellationState>>>,
        message_id: String,
    ) -> Self {
        Self {
            active_rounds,
            message_id,
        }
    }
}

impl Drop for ActiveRoundCleanup {
    fn drop(&mut self) {
        let active_rounds = self.active_rounds.clone();
        let message_id = self.message_id.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut active_rounds = active_rounds.lock().await;
                active_rounds.remove(&message_id);
            });
        }
    }
}
