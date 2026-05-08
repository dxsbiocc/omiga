//! Autopilot mode orchestrator v1.
//!
//! This module centralizes the phase transitions and strategy decisions for the
//! Autopilot workflow so `commands/chat.rs` does not need to hardcode every
//! phase mutation inline.

use crate::domain::agents::scheduler::SchedulingStrategy;
use crate::domain::autopilot_state::{self, AutopilotPhase, AutopilotState};
use crate::domain::chat_state::SessionRuntimeState;
use crate::domain::orchestration::ExecutionLane;
use crate::domain::session::TodoItem;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AutopilotOrchestrator;

impl AutopilotOrchestrator {
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
    ) -> std::io::Result<AutopilotState> {
        let todos = Self::snapshot_todos(sessions, session_id).await;
        autopilot_state::begin_turn(project_root, session_id, goal, env_label, &todos).await
    }

    pub async fn set_phase(
        sessions: &Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
        project_root: &Path,
        session_id: &str,
        phase: AutopilotPhase,
        env_label: Option<String>,
    ) -> std::io::Result<Option<AutopilotState>> {
        let todos = Self::snapshot_todos(sessions, session_id).await;
        autopilot_state::update_phase(project_root, session_id, phase, env_label, &todos).await
    }

    pub async fn complete(
        sessions: &Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
        project_root: &Path,
        session_id: &str,
    ) -> std::io::Result<Option<AutopilotState>> {
        let todos = Self::snapshot_todos(sessions, session_id).await;
        autopilot_state::complete_turn(project_root, session_id, &todos).await
    }

    pub async fn fail(
        sessions: &Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
        project_root: &Path,
        session_id: &str,
        phase: AutopilotPhase,
        error: &str,
    ) -> std::io::Result<Option<AutopilotState>> {
        let todos = Self::snapshot_todos(sessions, session_id).await;
        autopilot_state::fail_turn(project_root, session_id, phase, &todos, error).await
    }

    pub async fn phase_for_scheduler_result(
        project_root: &Path,
        session_id: &str,
        scheduler_built_plan: bool,
    ) -> Option<AutopilotPhase> {
        let state = autopilot_state::read_state(project_root, session_id).await?;
        if matches!(state.phase, AutopilotPhase::Complete) {
            return None;
        }
        Some(if scheduler_built_plan {
            AutopilotPhase::Plan
        } else {
            AutopilotPhase::Design
        })
    }

    pub async fn suggested_strategy(
        project_root: &Path,
        session_id: &str,
    ) -> Option<SchedulingStrategy> {
        let state = autopilot_state::read_state(project_root, session_id).await?;
        let strategy = match state.phase {
            AutopilotPhase::Intake
            | AutopilotPhase::Interview
            | AutopilotPhase::Expansion
            | AutopilotPhase::Design
            | AutopilotPhase::Plan => SchedulingStrategy::Phased,
            AutopilotPhase::Implementation => SchedulingStrategy::Parallel,
            AutopilotPhase::Qa | AutopilotPhase::Validation => {
                SchedulingStrategy::VerificationFirst
            }
            AutopilotPhase::Complete => return None,
        };
        Some(strategy)
    }

    pub async fn current_execution_lane(
        project_root: &Path,
        session_id: &str,
    ) -> Option<ExecutionLane> {
        let state = autopilot_state::read_state(project_root, session_id).await?;
        let lane = match state.phase {
            AutopilotPhase::Intake | AutopilotPhase::Interview | AutopilotPhase::Expansion => {
                ExecutionLane {
                    lane_id: "autopilot-spec",
                    preferred_agent_type: Some("Plan"),
                    supplemental_agent_types: &["literature-search", "deep-research"],
                    instructions: "Spec lane: clarify the scientific question, data/literature scope, inclusion/exclusion criteria, and expected research deliverable before broad analysis.",
                }
            }
            AutopilotPhase::Design => ExecutionLane {
                lane_id: "autopilot-design",
                preferred_agent_type: Some("architect"),
                supplemental_agent_types: &["literature-search", "deep-research"],
                instructions: "Design lane: define the analysis strategy, evidence sources, database queries, data-processing boundaries, and citation/report structure.",
            },
            AutopilotPhase::Plan => ExecutionLane {
                lane_id: "autopilot-plan",
                preferred_agent_type: Some("Plan"),
                supplemental_agent_types: &["literature-search", "deep-research"],
                instructions: "Planning lane: decompose the research task into evidence collection, data/method analysis, synthesis, and quality-control subtasks.",
            },
            AutopilotPhase::Implementation => ExecutionLane {
                lane_id: "autopilot-implementation",
                preferred_agent_type: Some("deep-research"),
                supplemental_agent_types: &["literature-search", "verification"],
                instructions: "Analysis lane: execute the next pending literature/data analysis task, preserve collected evidence, and avoid reopening accepted scope unless evidence forces it.",
            },
            AutopilotPhase::Qa => ExecutionLane {
                lane_id: "autopilot-qa",
                preferred_agent_type: Some("verification"),
                supplemental_agent_types: &["critic", "deep-research"],
                instructions: "Argumentation lane: stress-test the analysis, identify potential problems or counter-evidence, examine whether claims are logically supported, and make narrow corrections without expanding scope.",
            },
            AutopilotPhase::Validation => ExecutionLane {
                lane_id: "autopilot-validation",
                preferred_agent_type: Some("verification"),
                supplemental_agent_types: &["critic", "deep-research", "literature-search"],
                instructions: "Review lane: audit the scientific soundness, completeness, citation/data traceability, limitations, and conclusion boundaries before preparing the final research summary.",
            },
            AutopilotPhase::Complete => return None,
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
        let mut session = Session::new("Autopilot".to_string(), ".".to_string());
        session.id = session_id.to_string();
        let todos = vec![TodoItem {
            content: "qa".to_string(),
            status: TodoStatus::InProgress,
            active_form: "running qa".to_string(),
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
    async fn orchestrator_updates_phase_and_qa_limit() {
        let dir = tempdir().unwrap();
        let sessions = sessions_with_todos("sess-orch");
        let _ =
            AutopilotOrchestrator::begin(&sessions, dir.path(), "sess-orch", "Build feature", None)
                .await
                .unwrap();
        let state = AutopilotOrchestrator::set_phase(
            &sessions,
            dir.path(),
            "sess-orch",
            AutopilotPhase::Qa,
            None,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(state.phase, AutopilotPhase::Qa);
        assert_eq!(state.qa_cycles, 1);
    }

    #[tokio::test]
    async fn orchestrator_strategy_tracks_phase() {
        let dir = tempdir().unwrap();
        let sessions = sessions_with_todos("sess-strat");
        let _ = AutopilotOrchestrator::begin(
            &sessions,
            dir.path(),
            "sess-strat",
            "Build feature",
            None,
        )
        .await
        .unwrap();
        let _ = AutopilotOrchestrator::set_phase(
            &sessions,
            dir.path(),
            "sess-strat",
            AutopilotPhase::Validation,
            None,
        )
        .await
        .unwrap();
        let strategy = AutopilotOrchestrator::suggested_strategy(dir.path(), "sess-strat")
            .await
            .unwrap();
        assert_eq!(strategy, SchedulingStrategy::VerificationFirst);
    }

    #[tokio::test]
    async fn orchestrator_exposes_lane_for_phase() {
        let dir = tempdir().unwrap();
        let sessions = sessions_with_todos("sess-lane");
        let _ =
            AutopilotOrchestrator::begin(&sessions, dir.path(), "sess-lane", "Build feature", None)
                .await
                .unwrap();
        let _ = AutopilotOrchestrator::set_phase(
            &sessions,
            dir.path(),
            "sess-lane",
            AutopilotPhase::Qa,
            None,
        )
        .await
        .unwrap();
        let lane = AutopilotOrchestrator::current_execution_lane(dir.path(), "sess-lane")
            .await
            .unwrap();
        assert_eq!(lane.lane_id, "autopilot-qa");
        assert_eq!(lane.preferred_agent_type, Some("verification"));
        assert!(lane.supplemental_agent_types.contains(&"critic"));
        assert!(lane.supplemental_agent_types.contains(&"deep-research"));
    }

    #[tokio::test]
    async fn validation_lane_includes_reviewer_family() {
        let dir = tempdir().unwrap();
        let sessions = sessions_with_todos("sess-validation");
        let _ = AutopilotOrchestrator::begin(
            &sessions,
            dir.path(),
            "sess-validation",
            "Build feature",
            None,
        )
        .await
        .unwrap();
        let _ = AutopilotOrchestrator::set_phase(
            &sessions,
            dir.path(),
            "sess-validation",
            AutopilotPhase::Validation,
            None,
        )
        .await
        .unwrap();
        let lane = AutopilotOrchestrator::current_execution_lane(dir.path(), "sess-validation")
            .await
            .unwrap();
        assert!(lane.supplemental_agent_types.contains(&"critic"));
        assert!(lane.supplemental_agent_types.contains(&"deep-research"));
        assert!(lane.supplemental_agent_types.contains(&"literature-search"));
    }
}
