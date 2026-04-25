use super::models::{
    AgentCard, AgentResult, AssembledContext, BudgetSpec, ControlPlaneReport, IntakeAssessment,
    OutputContract, PermissionStatus, ResultStatus, ReviewResult, ReviewStatus, TaskGraph,
    TaskSpec, VerificationSpec,
};
use super::permissions::PermissionManager;
use super::planner::Planner;
use super::registry::AgentRegistry;
use super::reviewer::Reviewer;
use super::runner::AgentRunner;
use serde_json::json;
use std::collections::BTreeMap;

pub struct ResearchDirector<R> {
    registry: AgentRegistry,
    runner: R,
    reviewer: Reviewer,
    permission_manager: PermissionManager,
    planner: Planner,
}

impl<R: AgentRunner> ResearchDirector<R> {
    pub fn new(registry: AgentRegistry, runner: R) -> Self {
        Self {
            registry,
            runner,
            reviewer: Reviewer::new(),
            permission_manager: PermissionManager::new(),
            planner: Planner::new(),
        }
    }

    pub fn registry(&self) -> &AgentRegistry {
        &self.registry
    }

    pub fn prepare(&self, user_request: &str) -> Result<(TaskGraph, ControlPlaneReport), String> {
        let intake_card = self.require_card("mind_hunter.intake")?;
        let planner_card = self.require_card("planner.task_graph")?;
        let _reviewer_card = self.require_card("reviewer.verifier")?;
        let _executor_card = self.require_card("executor.supervisor")?;

        let intake_assessment = self.planner.analyze_intake(user_request);
        let intake_stage = self.run_intake_stage(intake_card, &intake_assessment)?;

        let task_graph = self.planner.plan(user_request)?;
        let planning_stage =
            self.run_planning_stage(planner_card, &task_graph, &intake_stage.0.output)?;

        Ok((
            task_graph,
            ControlPlaneReport {
                intake_assessment,
                intake_result: intake_stage.0,
                intake_review: intake_stage.1,
                planner_result: planning_stage.0,
                planner_review: planning_stage.1,
            },
        ))
    }

    fn require_card(&self, id: &str) -> Result<&AgentCard, String> {
        self.registry
            .get(id)
            .ok_or_else(|| format!("required control-plane agent '{}' not found", id))
    }

    fn run_intake_stage(
        &self,
        card: &AgentCard,
        intake: &IntakeAssessment,
    ) -> Result<(AgentResult, ReviewResult), String> {
        let task_spec = TaskSpec {
            task_id: "control-plane.intake".to_string(),
            goal: "Assess user intent and determine execution route".to_string(),
            assigned_agent: card.id.clone(),
            dependencies: Vec::new(),
            input_refs: Vec::new(),
            constraints: vec!["Do not hide uncertainty.".to_string()],
            expected_output: OutputContract {
                format: "json".to_string(),
                required_fields: vec![
                    "assumptions".to_string(),
                    "ambiguities".to_string(),
                    "execution_route".to_string(),
                    "complexity_score".to_string(),
                ],
                requires_evidence: false,
                minimum_artifacts: 0,
                schema_hint: json!({}),
            },
            success_criteria: card.success_criteria.clone(),
            verification: VerificationSpec {
                required_checks: vec!["shape".to_string(), "route_present".to_string()],
                required_evidence_count: 0,
                require_test_results: false,
                require_consistency_statement: true,
            },
            failure_conditions: vec!["Intake result is empty.".to_string()],
            stop_conditions: vec!["Control-plane validation failed.".to_string()],
            budget: BudgetSpec::default(),
            requested_tools: Vec::new(),
            requested_permissions: Default::default(),
        };
        let context = AssembledContext {
            agent_id: card.id.clone(),
            task_id: task_spec.task_id.clone(),
            sections: BTreeMap::from([
                ("user_goal".to_string(), json!(intake.user_goal)),
                (
                    "intake_assessment".to_string(),
                    json!({
                        "user_goal": intake.user_goal,
                        "assumptions": intake.assumptions,
                        "ambiguities": intake.ambiguities,
                        "execution_route": intake.execution_route,
                        "complexity_score": intake.complexity_score,
                    }),
                ),
                ("task_spec".to_string(), json!(task_spec)),
                ("agent_instructions".to_string(), json!(card.instructions)),
            ]),
            omitted_sections: Vec::new(),
            token_estimate: 256,
        };

        self.run_control_plane_task(card, &task_spec, &context, "control-plane intake")
    }

    fn run_planning_stage(
        &self,
        card: &AgentCard,
        task_graph: &TaskGraph,
        intake_output: &serde_json::Value,
    ) -> Result<(AgentResult, ReviewResult), String> {
        let task_spec = TaskSpec {
            task_id: "control-plane.planner".to_string(),
            goal: "Produce a structured task graph".to_string(),
            assigned_agent: card.id.clone(),
            dependencies: vec!["control-plane.intake".to_string()],
            input_refs: Vec::new(),
            constraints: vec!["Return a structured task graph.".to_string()],
            expected_output: OutputContract {
                format: "json".to_string(),
                required_fields: vec![
                    "tasks".to_string(),
                    "edges".to_string(),
                    "final_output_contract".to_string(),
                ],
                requires_evidence: false,
                minimum_artifacts: 0,
                schema_hint: json!({}),
            },
            success_criteria: card.success_criteria.clone(),
            verification: VerificationSpec {
                required_checks: vec!["shape".to_string(), "termination_fields".to_string()],
                required_evidence_count: 0,
                require_test_results: false,
                require_consistency_statement: true,
            },
            failure_conditions: vec!["Task graph is empty.".to_string()],
            stop_conditions: vec!["Control-plane validation failed.".to_string()],
            budget: BudgetSpec::default(),
            requested_tools: Vec::new(),
            requested_permissions: Default::default(),
        };
        let context = AssembledContext {
            agent_id: card.id.clone(),
            task_id: task_spec.task_id.clone(),
            sections: BTreeMap::from([
                ("user_goal".to_string(), json!(task_graph.user_goal)),
                ("global_context".to_string(), intake_output.clone()),
                (
                    "planned_task_graph".to_string(),
                    json!({
                        "graph_id": task_graph.graph_id,
                        "tasks": task_graph.tasks,
                        "edges": task_graph.edges,
                        "assumptions": task_graph.assumptions,
                        "ambiguities": task_graph.ambiguities,
                        "execution_route": task_graph.execution_route,
                        "final_output_contract": task_graph.final_output_contract,
                    }),
                ),
                ("task_spec".to_string(), json!(task_spec)),
                ("agent_instructions".to_string(), json!(card.instructions)),
            ]),
            omitted_sections: Vec::new(),
            token_estimate: 512,
        };

        self.run_control_plane_task(card, &task_spec, &context, "control-plane planner")
    }

    fn run_control_plane_task(
        &self,
        card: &AgentCard,
        task_spec: &TaskSpec,
        context: &AssembledContext,
        label: &str,
    ) -> Result<(AgentResult, ReviewResult), String> {
        let permission_decision = self.permission_manager.check(card, task_spec);
        if !matches!(permission_decision.status, PermissionStatus::Allowed) {
            return Err(format!(
                "{} permission denied: {}",
                label,
                permission_decision.reasons.join("; ")
            ));
        }

        let mut result = self.runner.run(card, task_spec, context)?;
        result.permission_status = Some(permission_decision.status);
        let review = self
            .reviewer
            .review(task_spec, &result, &permission_decision);

        if !matches!(result.status, ResultStatus::Completed) {
            return Err(format!("{} runner status was {:?}", label, result.status));
        }
        if !matches!(review.status, ReviewStatus::Pass) {
            return Err(format!("{} review did not pass", label));
        }

        Ok((result, review))
    }
}
