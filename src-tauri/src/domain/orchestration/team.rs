//! Team mode orchestrator helpers.
//!
//! Centralizes Team-phase strategy and execution-lane decisions so prompt
//! assembly can stay consistent with persisted Team state.

use crate::domain::agents::scheduler::SchedulingStrategy;
use crate::domain::orchestration::ExecutionLane;
use crate::domain::team_state::{self, TeamPhase};
use std::path::Path;

pub struct TeamOrchestrator;

impl TeamOrchestrator {
    pub async fn begin(
        project_root: &Path,
        session_id: &str,
        goal: &str,
    ) -> std::io::Result<team_state::TeamState> {
        let mut state = team_state::read_state(project_root, session_id)
            .await
            .unwrap_or_else(|| {
                team_state::TeamState::new(
                    session_id.to_string(),
                    goal.to_string(),
                    project_root.to_string_lossy().to_string(),
                )
            });
        state.goal = goal.to_string();
        state.project_root = project_root.to_string_lossy().to_string();
        state.phase = TeamPhase::Planning;
        state.touch();
        team_state::write_state(project_root, &state).await?;
        Ok(state)
    }

    pub async fn complete(
        project_root: &Path,
        session_id: &str,
    ) -> std::io::Result<Option<team_state::TeamState>> {
        let Some(mut state) = team_state::read_state(project_root, session_id).await else {
            return Ok(None);
        };
        state.phase = TeamPhase::Complete;
        state.touch();
        team_state::write_state(project_root, &state).await?;
        Ok(Some(state))
    }

    pub async fn fail(
        project_root: &Path,
        session_id: &str,
        error: &str,
    ) -> std::io::Result<Option<team_state::TeamState>> {
        let Some(mut state) = team_state::read_state(project_root, session_id).await else {
            return Ok(None);
        };
        state.phase = TeamPhase::Failed;
        if let Some(subtask) = state.subtasks.iter_mut().find(|s| s.status == "running") {
            subtask.status = "failed".to_string();
            subtask.error = Some(error.chars().take(500).collect());
        }
        state.touch();
        team_state::write_state(project_root, &state).await?;
        Ok(Some(state))
    }

    pub async fn suggested_strategy(
        project_root: &Path,
        session_id: &str,
    ) -> Option<SchedulingStrategy> {
        let state = team_state::read_state(project_root, session_id).await?;
        let strategy = match state.phase {
            TeamPhase::Planning | TeamPhase::Executing | TeamPhase::Synthesizing => {
                SchedulingStrategy::Team
            }
            TeamPhase::Verifying | TeamPhase::Fixing => SchedulingStrategy::VerificationFirst,
            TeamPhase::Complete | TeamPhase::Failed => return None,
        };
        Some(strategy)
    }

    pub async fn current_execution_lane(
        project_root: &Path,
        session_id: &str,
    ) -> Option<ExecutionLane> {
        let state = team_state::read_state(project_root, session_id).await?;
        let lane = match state.phase {
            TeamPhase::Planning => ExecutionLane {
                lane_id: "team-planning",
                preferred_agent_type: Some("Plan"),
                supplemental_agent_types: &["literature-search", "deep-research"],
                instructions: "Team planning lane: decompose the scientific question into parallel evidence/data-analysis subtasks with explicit sources, ownership, dependencies, and success criteria.",
            },
            TeamPhase::Executing => ExecutionLane {
                lane_id: "team-execution",
                preferred_agent_type: Some("deep-research"),
                supplemental_agent_types: &["literature-search", "verification"],
                instructions: "Team execution lane: drive the next pending research-analysis worker task, preserve completed evidence, and coordinate through the shared blackboard.",
            },
            TeamPhase::Verifying => ExecutionLane {
                lane_id: "team-verification",
                preferred_agent_type: Some("verification"),
                supplemental_agent_types: &["critic", "deep-research", "literature-search"],
                instructions: "Team verification lane: compare all worker outputs against the original scientific question, evidence quality, citation traceability, and acceptance criteria before declaring success.",
            },
            TeamPhase::Fixing => ExecutionLane {
                lane_id: "team-fixing",
                preferred_agent_type: Some("debugger"),
                supplemental_agent_types: &["verification", "critic"],
                instructions: "Team fixing lane: focus on verification-reported evidence gaps, citation errors, data inconsistencies, and unsupported claims; apply the narrowest corrective analysis.",
            },
            TeamPhase::Synthesizing => ExecutionLane {
                lane_id: "team-synthesis",
                preferred_agent_type: Some("architect"),
                supplemental_agent_types: &["critic", "verification", "deep-research"],
                instructions: "Team synthesis lane: aggregate verified worker outputs into a coherent research report with traceable citations, limitations, and next-step analysis suggestions.",
            },
            TeamPhase::Complete | TeamPhase::Failed => return None,
        };
        Some(lane)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::tempdir;

    #[tokio::test]
    async fn team_orchestrator_begin_and_complete() {
        let dir = tempdir().unwrap();
        let begun = TeamOrchestrator::begin(dir.path(), "team-begin", "Parallelize feature")
            .await
            .unwrap();
        assert_eq!(begun.phase, TeamPhase::Planning);

        let completed = TeamOrchestrator::complete(dir.path(), "team-begin")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(completed.phase, TeamPhase::Complete);
    }

    #[tokio::test]
    async fn team_orchestrator_strategy_tracks_phase() {
        let dir = tempdir().unwrap();
        let state = team_state::TeamState {
            version: 1,
            session_id: "team-s".to_string(),
            goal: "Parallelize feature".to_string(),
            phase: TeamPhase::Verifying,
            project_root: dir.path().display().to_string(),
            subtasks: vec![],
            started_at: Utc::now(),
            updated_at: Utc::now(),
        };
        team_state::write_state(dir.path(), &state).await.unwrap();
        let strategy = TeamOrchestrator::suggested_strategy(dir.path(), "team-s")
            .await
            .unwrap();
        assert_eq!(strategy, SchedulingStrategy::VerificationFirst);
    }

    #[tokio::test]
    async fn team_orchestrator_exposes_lane_for_phase() {
        let dir = tempdir().unwrap();
        let state = team_state::TeamState {
            version: 1,
            session_id: "team-l".to_string(),
            goal: "Parallelize feature".to_string(),
            phase: TeamPhase::Fixing,
            project_root: dir.path().display().to_string(),
            subtasks: vec![],
            started_at: Utc::now(),
            updated_at: Utc::now(),
        };
        team_state::write_state(dir.path(), &state).await.unwrap();
        let lane = TeamOrchestrator::current_execution_lane(dir.path(), "team-l")
            .await
            .unwrap();
        assert_eq!(lane.lane_id, "team-fixing");
        assert_eq!(lane.preferred_agent_type, Some("debugger"));
        assert!(lane.supplemental_agent_types.contains(&"verification"));
        assert!(lane.supplemental_agent_types.contains(&"critic"));
    }

    #[tokio::test]
    async fn team_verification_lane_includes_research_reviewers() {
        let dir = tempdir().unwrap();
        let state = team_state::TeamState {
            version: 1,
            session_id: "team-v".to_string(),
            goal: "Parallelize feature".to_string(),
            phase: TeamPhase::Verifying,
            project_root: dir.path().display().to_string(),
            subtasks: vec![],
            started_at: Utc::now(),
            updated_at: Utc::now(),
        };
        team_state::write_state(dir.path(), &state).await.unwrap();
        let lane = TeamOrchestrator::current_execution_lane(dir.path(), "team-v")
            .await
            .unwrap();
        assert!(lane.supplemental_agent_types.contains(&"critic"));
        assert!(lane.supplemental_agent_types.contains(&"deep-research"));
        assert!(lane.supplemental_agent_types.contains(&"literature-search"));
    }
}
