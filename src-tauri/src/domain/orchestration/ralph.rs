//! Ralph mode orchestrator v1.
//!
//! Centralizes Ralph lifecycle operations so the chat command path can delegate
//! phase transitions and future resume logic to a dedicated runtime surface.

use crate::domain::agents::scheduler::SchedulingStrategy;
use crate::domain::chat_state::SessionRuntimeState;
use crate::domain::orchestration::ExecutionLane;
use crate::domain::ralph_state::{self, RalphPhase, RalphState};
use crate::domain::session::TodoItem;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct RalphOrchestrator;

impl RalphOrchestrator {
    async fn snapshot_todos(
        sessions: &Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
        session_id: &str,
    ) -> Vec<TodoItem> {
        let todos_arc = {
            let guard = sessions.read().await;
            guard.get(session_id).map(|runtime| runtime.todos.clone())
        };
        match todos_arc {
            Some(todos) => todos.lock().await.clone(),
            None => vec![],
        }
    }

    pub async fn begin(
        sessions: &Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
        project_root: &Path,
        session_id: &str,
        goal: &str,
        env_label: Option<String>,
    ) -> std::io::Result<RalphState> {
        let todos = Self::snapshot_todos(sessions, session_id).await;
        ralph_state::begin_turn(project_root, session_id, goal, env_label, &todos).await
    }

    pub async fn set_phase(
        sessions: &Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
        project_root: &Path,
        session_id: &str,
        phase: RalphPhase,
        env_label: Option<String>,
    ) -> std::io::Result<Option<RalphState>> {
        let todos = Self::snapshot_todos(sessions, session_id).await;
        ralph_state::update_phase(project_root, session_id, phase, env_label, &todos).await
    }

    pub async fn complete(
        sessions: &Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
        project_root: &Path,
        session_id: &str,
    ) -> std::io::Result<Option<RalphState>> {
        let todos = Self::snapshot_todos(sessions, session_id).await;
        ralph_state::complete_turn(project_root, session_id, &todos).await
    }

    pub async fn fail(
        sessions: &Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
        project_root: &Path,
        session_id: &str,
        phase: RalphPhase,
        error: &str,
    ) -> std::io::Result<Option<RalphState>> {
        let todos = Self::snapshot_todos(sessions, session_id).await;
        ralph_state::fail_turn(project_root, session_id, phase, &todos, error).await
    }

    pub async fn suggested_strategy(
        project_root: &Path,
        session_id: &str,
    ) -> Option<SchedulingStrategy> {
        let state = ralph_state::read_state(project_root, session_id).await?;
        let strategy = match state.phase {
            RalphPhase::Planning | RalphPhase::EnvCheck | RalphPhase::Executing => {
                SchedulingStrategy::Sequential
            }
            RalphPhase::QualityCheck | RalphPhase::Verifying => {
                SchedulingStrategy::VerificationFirst
            }
            RalphPhase::Complete => return None,
        };
        Some(strategy)
    }

    pub async fn current_execution_lane(
        project_root: &Path,
        session_id: &str,
    ) -> Option<ExecutionLane> {
        let state = ralph_state::read_state(project_root, session_id).await?;
        let lane = match state.phase {
            RalphPhase::Planning => ExecutionLane {
                lane_id: "ralph-plan",
                preferred_agent_type: Some("Plan"),
                supplemental_agent_types: &["test-engineer"],
                instructions: "Planning lane: refine the execution plan, preserve completed work, and make the next actionable todo explicit before broad execution.",
            },
            RalphPhase::EnvCheck => ExecutionLane {
                lane_id: "ralph-env-check",
                preferred_agent_type: Some("debugger"),
                supplemental_agent_types: &[],
                instructions: "Environment lane: verify dependencies, credentials, paths, and runtime prerequisites before attempting the next step.",
            },
            RalphPhase::Executing => ExecutionLane {
                lane_id: "ralph-execution",
                preferred_agent_type: Some("executor"),
                supplemental_agent_types: &["test-engineer"],
                instructions: "Execution lane: continue the next pending task directly and keep momentum without restarting completed steps.",
            },
            RalphPhase::QualityCheck => ExecutionLane {
                lane_id: "ralph-quality",
                preferred_agent_type: Some("verification"),
                supplemental_agent_types: &["performance-reviewer"],
                instructions: "Quality lane: inspect produced outputs, compare against expected artifacts, and apply only the smallest corrective fixes needed.",
            },
            RalphPhase::Verifying => ExecutionLane {
                lane_id: "ralph-verify",
                preferred_agent_type: Some("verification"),
                supplemental_agent_types: &[
                    "code-reviewer",
                    "quality-reviewer",
                    "test-engineer",
                    "critic",
                ],
                instructions: "Verification lane: gather evidence, run acceptance checks, and summarize whether the original goal is satisfied.",
            },
            RalphPhase::Complete => return None,
        };
        Some(lane)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chat_state::SessionRuntimeState;
    use crate::domain::session::{Session, TodoStatus};
    use crate::domain::tools::env_store::EnvStore;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::{Mutex, RwLock};

    fn sessions_with_todos(session_id: &str) -> Arc<RwLock<HashMap<String, SessionRuntimeState>>> {
        let mut session = Session::new("Ralph".to_string(), ".".to_string());
        session.id = session_id.to_string();
        let todos = vec![TodoItem {
            content: "run task".to_string(),
            status: TodoStatus::InProgress,
            active_form: "running".to_string(),
        }];
        let runtime = SessionRuntimeState {
            session,
            active_round_ids: vec![],
            todos: Arc::new(Mutex::new(todos)),
            agent_tasks: Arc::new(Mutex::new(vec![])),
            plan_mode: Arc::new(Mutex::new(false)),
            execution_environment: "local".to_string(),
            ssh_server: None,
            sandbox_backend: "docker".to_string(),
            local_venv_type: "".to_string(),
            local_venv_name: "".to_string(),
            env_store: EnvStore::new(),
        };
        let mut map = HashMap::new();
        map.insert(session_id.to_string(), runtime);
        Arc::new(RwLock::new(map))
    }

    #[tokio::test]
    async fn orchestrator_updates_phase_and_error_count() {
        let dir = tempdir().unwrap();
        let sessions = sessions_with_todos("sess-r-orch");
        let _ = RalphOrchestrator::begin(
            &sessions,
            dir.path(),
            "sess-r-orch",
            "Continue analysis",
            None,
        )
        .await
        .unwrap();
        let state = RalphOrchestrator::set_phase(
            &sessions,
            dir.path(),
            "sess-r-orch",
            RalphPhase::Executing,
            None,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(state.phase, RalphPhase::Executing);

        let failed = RalphOrchestrator::fail(
            &sessions,
            dir.path(),
            "sess-r-orch",
            RalphPhase::Executing,
            "tool crashed",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(failed.consecutive_errors, 1);
    }

    #[tokio::test]
    async fn orchestrator_strategy_tracks_phase() {
        let dir = tempdir().unwrap();
        let sessions = sessions_with_todos("sess-r-strat");
        let _ = RalphOrchestrator::begin(
            &sessions,
            dir.path(),
            "sess-r-strat",
            "Continue analysis",
            None,
        )
        .await
        .unwrap();
        let _ = RalphOrchestrator::set_phase(
            &sessions,
            dir.path(),
            "sess-r-strat",
            RalphPhase::Verifying,
            None,
        )
        .await
        .unwrap();
        let strategy = RalphOrchestrator::suggested_strategy(dir.path(), "sess-r-strat")
            .await
            .unwrap();
        assert_eq!(strategy, SchedulingStrategy::VerificationFirst);
    }

    #[tokio::test]
    async fn orchestrator_exposes_lane_for_phase() {
        let dir = tempdir().unwrap();
        let sessions = sessions_with_todos("sess-r-lane");
        let _ = RalphOrchestrator::begin(
            &sessions,
            dir.path(),
            "sess-r-lane",
            "Continue analysis",
            None,
        )
        .await
        .unwrap();
        let _ = RalphOrchestrator::set_phase(
            &sessions,
            dir.path(),
            "sess-r-lane",
            RalphPhase::QualityCheck,
            None,
        )
        .await
        .unwrap();
        let lane = RalphOrchestrator::current_execution_lane(dir.path(), "sess-r-lane")
            .await
            .unwrap();
        assert_eq!(lane.lane_id, "ralph-quality");
        assert_eq!(lane.preferred_agent_type, Some("verification"));
        assert!(lane
            .supplemental_agent_types
            .contains(&"performance-reviewer"));
    }

    #[tokio::test]
    async fn verify_lane_includes_critic_and_quality() {
        let dir = tempdir().unwrap();
        let sessions = sessions_with_todos("sess-r-verify");
        let _ = RalphOrchestrator::begin(
            &sessions,
            dir.path(),
            "sess-r-verify",
            "Continue analysis",
            None,
        )
        .await
        .unwrap();
        let _ = RalphOrchestrator::set_phase(
            &sessions,
            dir.path(),
            "sess-r-verify",
            RalphPhase::Verifying,
            None,
        )
        .await
        .unwrap();
        let lane = RalphOrchestrator::current_execution_lane(dir.path(), "sess-r-verify")
            .await
            .unwrap();
        assert!(lane.supplemental_agent_types.contains(&"quality-reviewer"));
        assert!(lane.supplemental_agent_types.contains(&"critic"));
    }
}
