//! Resume context builders for long-running orchestration modes.
//!
//! These helpers turn persisted Ralph / Autopilot state into compact system-prompt
//! sections so a follow-up "continue" turn can genuinely resume work rather than
//! restarting from scratch.

use std::path::Path;

fn join_preview(items: &[String], max_items: usize) -> String {
    if items.is_empty() {
        return "none".to_string();
    }
    let mut preview = items.iter().take(max_items).cloned().collect::<Vec<_>>();
    if items.len() > max_items {
        preview.push(format!("… (+{} more)", items.len() - max_items));
    }
    preview.join("; ")
}

pub async fn build_ralph_resume_context(project_root: &Path, session_id: &str) -> Option<String> {
    let state = crate::domain::ralph_state::read_state(project_root, session_id).await?;
    if matches!(
        state.phase,
        crate::domain::ralph_state::RalphPhase::Complete
    ) {
        return None;
    }

    Some(format!(
        "## Resume Context: Ralph\n\
         Continue from the persisted Ralph state below unless the user explicitly asked to restart.\n\
         Do not reset the plan or redo already-completed work without cause.\n\n\
         - session_id: `{}`\n\
         - goal: {}\n\
         - phase: `{}`\n\
         - iteration: {}\n\
         - consecutive_errors: {}\n\
         - completed_todos: {}\n\
         - pending_todos: {}\n\
         - last_error: {}\n",
        state.session_id,
        state.goal,
        state.phase,
        state.iteration,
        state.consecutive_errors,
        join_preview(&state.todos_completed, 5),
        join_preview(&state.todos_pending, 5),
        state.last_error.as_deref().unwrap_or("none"),
    ))
}

pub async fn build_autopilot_resume_context(
    project_root: &Path,
    session_id: &str,
) -> Option<String> {
    let state = crate::domain::autopilot_state::read_state(project_root, session_id).await?;
    if matches!(
        state.phase,
        crate::domain::autopilot_state::AutopilotPhase::Complete
    ) {
        return None;
    }

    Some(format!(
        "## Resume Context: Autopilot\n\
         Continue from the persisted Autopilot state below unless the user explicitly asked to restart.\n\
         Preserve completed work and continue from the current stage of the pipeline.\n\n\
         - session_id: `{}`\n\
         - goal: {}\n\
         - phase: `{}`\n\
         - qa_cycles: {}/{}\n\
         - completed_todos: {}\n\
         - pending_todos: {}\n\
         - last_error: {}\n",
        state.session_id,
        state.goal,
        state.phase,
        state.qa_cycles,
        state.max_qa_cycles,
        join_preview(&state.todos_completed, 5),
        join_preview(&state.todos_pending, 5),
        state.last_error.as_deref().unwrap_or("none"),
    ))
}

pub async fn build_ralph_phase_guidance(project_root: &Path, session_id: &str) -> Option<String> {
    let state = crate::domain::ralph_state::read_state(project_root, session_id).await?;
    if matches!(
        state.phase,
        crate::domain::ralph_state::RalphPhase::Complete
    ) {
        return None;
    }

    let guidance = match state.phase {
        crate::domain::ralph_state::RalphPhase::Planning => {
            "Continue the Ralph run by refining the plan, preserving completed work, and updating todos before broad execution."
        }
        crate::domain::ralph_state::RalphPhase::EnvCheck => {
            "Resume with environment verification first: confirm dependencies, paths, credentials, and execution prerequisites before taking broad action."
        }
        crate::domain::ralph_state::RalphPhase::Executing => {
            "Resume execution directly. Do not restart completed steps; focus on the next pending todo and keep momentum."
        }
        crate::domain::ralph_state::RalphPhase::QualityCheck => {
            "Prioritize quality checks on produced outputs, inspect failures, and make the smallest fixes needed to proceed."
        }
        crate::domain::ralph_state::RalphPhase::Verifying => {
            "Prioritize verification evidence and acceptance checks before adding new implementation work."
        }
        crate::domain::ralph_state::RalphPhase::Complete => return None,
    };

    Some(format!(
        "## Phase Control: Ralph\nCurrent Ralph phase is `{}`.\n{}",
        state.phase, guidance
    ))
}

pub async fn build_autopilot_phase_guidance(
    project_root: &Path,
    session_id: &str,
) -> Option<String> {
    let state = crate::domain::autopilot_state::read_state(project_root, session_id).await?;
    if matches!(
        state.phase,
        crate::domain::autopilot_state::AutopilotPhase::Complete
    ) {
        return None;
    }

    let guidance = match state.phase {
        crate::domain::autopilot_state::AutopilotPhase::Intake => {
            "Stabilize the task statement, constraints, and desired outcome before expanding work."
        }
        crate::domain::autopilot_state::AutopilotPhase::Interview => {
            "Resolve missing requirements first; prefer clarification/spec artifacts over implementation."
        }
        crate::domain::autopilot_state::AutopilotPhase::Expansion => {
            "Expand the brief into a concrete spec and acceptance criteria before implementation."
        }
        crate::domain::autopilot_state::AutopilotPhase::Design => {
            "Focus on architecture, interfaces, and test strategy before broad code changes."
        }
        crate::domain::autopilot_state::AutopilotPhase::Plan => {
            "Decompose into ordered tasks with dependencies and parallelism boundaries before executing."
        }
        crate::domain::autopilot_state::AutopilotPhase::Implementation => {
            "Prioritize implementation of pending tasks and tests; avoid reopening already accepted planning work."
        }
        crate::domain::autopilot_state::AutopilotPhase::Qa => {
            "You are in the QA cycle. Prioritize tests, build, lint, and narrow fixes. Do not expand scope. If repeated failures persist, summarize the root blocker."
        }
        crate::domain::autopilot_state::AutopilotPhase::Validation => {
            "Prioritize final validation against acceptance criteria and prepare a concise evidence-backed completion summary."
        }
        crate::domain::autopilot_state::AutopilotPhase::Complete => return None,
    };

    Some(format!(
        "## Phase Control: Autopilot\nCurrent Autopilot phase is `{}` (qa_cycles {}/{}).\n{}",
        state.phase, state.qa_cycles, state.max_qa_cycles, guidance
    ))
}

pub async fn suggested_mode_strategy(
    project_root: &Path,
    session_id: &str,
    skill_name: &str,
) -> Option<crate::domain::agents::scheduler::SchedulingStrategy> {
    match skill_name {
        "ralph" => {
            crate::domain::orchestration::ralph::RalphOrchestrator::suggested_strategy(
                project_root,
                session_id,
            )
            .await
        }
        "autopilot" => {
            crate::domain::orchestration::autopilot::AutopilotOrchestrator::suggested_strategy(
                project_root,
                session_id,
            )
            .await
        }
        "team" => {
            crate::domain::orchestration::team::TeamOrchestrator::suggested_strategy(
                project_root,
                session_id,
            )
            .await
        }
        _ => None,
    }
}

pub async fn build_team_resume_context(project_root: &Path, session_id: &str) -> Option<String> {
    let state = crate::domain::team_state::read_state(project_root, session_id).await?;
    if matches!(
        state.phase,
        crate::domain::team_state::TeamPhase::Complete
            | crate::domain::team_state::TeamPhase::Failed
    ) {
        return None;
    }

    let completed = state.completed_count();
    let total = state.subtasks.len();
    let failed = state.failed_count();
    let running = state.running_count();

    Some(format!(
        "## Resume Context: Team\n\
         Continue from the persisted Team state below unless the user explicitly asked to restart.\n\
         Do not re-run subtasks that have already completed.\n\n\
         - session_id: `{}`\n\
         - goal: {}\n\
         - phase: `{}`\n\
         - subtasks: {}/{} completed, {} running, {} failed\n",
        state.session_id, state.goal, state.phase, completed, total, running, failed,
    ))
}

pub async fn build_team_phase_guidance(project_root: &Path, session_id: &str) -> Option<String> {
    let state = crate::domain::team_state::read_state(project_root, session_id).await?;
    let guidance = match state.phase {
        crate::domain::team_state::TeamPhase::Planning => {
            "You are in the **Planning** phase. Decompose the goal into parallel subtasks and assign agents."
        }
        crate::domain::team_state::TeamPhase::Executing => {
            "You are in the **Executing** phase. Workers are running in parallel. Coordinate output via the shared blackboard."
        }
        crate::domain::team_state::TeamPhase::Verifying => {
            "You are in the **Verifying** phase. Review all worker outputs against the original goal and acceptance criteria."
        }
        crate::domain::team_state::TeamPhase::Fixing => {
            "You are in the **Fixing** phase. Address issues found during verification. Focus on root causes, not symptoms."
        }
        crate::domain::team_state::TeamPhase::Synthesizing => {
            "You are in the **Synthesizing** phase. Aggregate all worker outputs into a coherent final response for the user."
        }
        crate::domain::team_state::TeamPhase::Complete
        | crate::domain::team_state::TeamPhase::Failed => return None,
    };
    Some(format!("## Team Phase Guidance\n\n{}\n", guidance))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::tempdir;

    #[tokio::test]
    async fn builds_ralph_resume_context_for_active_state() {
        let dir = tempdir().unwrap();
        let state = crate::domain::ralph_state::RalphState {
            version: 1,
            session_id: "sess-r".to_string(),
            goal: "Continue differential analysis".to_string(),
            phase: crate::domain::ralph_state::RalphPhase::Executing,
            iteration: 3,
            consecutive_errors: 1,
            project_root: dir.path().display().to_string(),
            env: None,
            todos_completed: vec!["env check".to_string()],
            todos_pending: vec!["run tool".to_string()],
            last_error: Some("temporary timeout".to_string()),
            started_at: Utc::now(),
            updated_at: Utc::now(),
        };
        crate::domain::ralph_state::write_state(dir.path(), &state)
            .await
            .unwrap();

        let ctx = build_ralph_resume_context(dir.path(), "sess-r")
            .await
            .unwrap();
        assert!(ctx.contains("Resume Context: Ralph"));
        assert!(ctx.contains("run tool"));
        assert!(ctx.contains("temporary timeout"));
    }

    #[tokio::test]
    async fn builds_autopilot_resume_context_for_active_state() {
        let dir = tempdir().unwrap();
        let state = crate::domain::autopilot_state::AutopilotState {
            version: 1,
            session_id: "sess-a".to_string(),
            goal: "Ship validated feature".to_string(),
            phase: crate::domain::autopilot_state::AutopilotPhase::Qa,
            project_root: dir.path().display().to_string(),
            qa_cycles: 2,
            max_qa_cycles: 5,
            env: None,
            todos_completed: vec!["spec".to_string()],
            todos_pending: vec!["qa pass".to_string()],
            last_error: Some("test failure".to_string()),
            started_at: Utc::now(),
            updated_at: Utc::now(),
        };
        crate::domain::autopilot_state::write_state(dir.path(), &state)
            .await
            .unwrap();

        let ctx = build_autopilot_resume_context(dir.path(), "sess-a")
            .await
            .unwrap();
        assert!(ctx.contains("Resume Context: Autopilot"));
        assert!(ctx.contains("2/5"));
        assert!(ctx.contains("test failure"));
    }

    #[tokio::test]
    async fn builds_phase_guidance_and_strategy_for_autopilot() {
        let dir = tempdir().unwrap();
        let state = crate::domain::autopilot_state::AutopilotState {
            version: 1,
            session_id: "sess-strategy".to_string(),
            goal: "Ship validated feature".to_string(),
            phase: crate::domain::autopilot_state::AutopilotPhase::Qa,
            project_root: dir.path().display().to_string(),
            qa_cycles: 3,
            max_qa_cycles: 5,
            env: None,
            todos_completed: vec![],
            todos_pending: vec!["qa".to_string()],
            last_error: None,
            started_at: Utc::now(),
            updated_at: Utc::now(),
        };
        crate::domain::autopilot_state::write_state(dir.path(), &state)
            .await
            .unwrap();

        let guidance = build_autopilot_phase_guidance(dir.path(), "sess-strategy")
            .await
            .unwrap();
        assert!(guidance.contains("Phase Control: Autopilot"));
        assert!(guidance.contains("QA cycle"));

        let strategy = suggested_mode_strategy(dir.path(), "sess-strategy", "autopilot")
            .await
            .unwrap();
        assert_eq!(
            strategy,
            crate::domain::agents::scheduler::SchedulingStrategy::VerificationFirst
        );
    }

    #[tokio::test]
    async fn suggested_strategy_supports_ralph() {
        let dir = tempdir().unwrap();
        let state = crate::domain::ralph_state::RalphState {
            version: 1,
            session_id: "sess-r-strategy".to_string(),
            goal: "Continue analysis".to_string(),
            phase: crate::domain::ralph_state::RalphPhase::Verifying,
            iteration: 2,
            consecutive_errors: 0,
            project_root: dir.path().display().to_string(),
            env: None,
            todos_completed: vec![],
            todos_pending: vec!["verify".to_string()],
            last_error: None,
            started_at: Utc::now(),
            updated_at: Utc::now(),
        };
        crate::domain::ralph_state::write_state(dir.path(), &state)
            .await
            .unwrap();

        let strategy = suggested_mode_strategy(dir.path(), "sess-r-strategy", "ralph")
            .await
            .unwrap();
        assert_eq!(
            strategy,
            crate::domain::agents::scheduler::SchedulingStrategy::VerificationFirst
        );
    }

    #[tokio::test]
    async fn suggested_strategy_supports_team() {
        let dir = tempdir().unwrap();
        let state = crate::domain::team_state::TeamState {
            version: 1,
            session_id: "sess-team-strategy".to_string(),
            goal: "Parallel goal".to_string(),
            phase: crate::domain::team_state::TeamPhase::Fixing,
            project_root: dir.path().display().to_string(),
            subtasks: vec![],
            started_at: Utc::now(),
            updated_at: Utc::now(),
        };
        crate::domain::team_state::write_state(dir.path(), &state)
            .await
            .unwrap();

        let strategy = suggested_mode_strategy(dir.path(), "sess-team-strategy", "team")
            .await
            .unwrap();
        assert_eq!(
            strategy,
            crate::domain::agents::scheduler::SchedulingStrategy::VerificationFirst
        );
    }
}
