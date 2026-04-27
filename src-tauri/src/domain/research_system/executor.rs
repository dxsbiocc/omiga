use super::context::ContextAssembler;
use super::models::{
    AgentCard, AgentResult, AssembledContext, ExecutionRoute, OrchestrationResult,
    PermissionDecision, PermissionStatus, ResultStatus, ReviewStatus, TaskGraph, TaskSpec,
    TraceKind, TraceRecord,
};
use super::permissions::PermissionManager;
use super::registry::AgentRegistry;
use super::reviewer::Reviewer;
use super::runner::AgentRunner;
use super::stores::{ArtifactStore, EvidenceStore, TaskGraphStore, TraceStore};
use chrono::Utc;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::thread;
use uuid::Uuid;

pub struct Executor<R> {
    registry: AgentRegistry,
    runner: R,
    reviewer: Reviewer,
    permission_manager: PermissionManager,
    context_assembler: ContextAssembler,
    task_graph_store: Box<dyn TaskGraphStore>,
    artifact_store: Box<dyn ArtifactStore>,
    evidence_store: Box<dyn EvidenceStore>,
    trace_store: Box<dyn TraceStore>,
}

pub struct ExecutorStores {
    pub task_graph_store: Box<dyn TaskGraphStore>,
    pub artifact_store: Box<dyn ArtifactStore>,
    pub evidence_store: Box<dyn EvidenceStore>,
    pub trace_store: Box<dyn TraceStore>,
}

pub struct ExecutorDependencies<R> {
    pub registry: AgentRegistry,
    pub runner: R,
    pub reviewer: Reviewer,
    pub permission_manager: PermissionManager,
    pub context_assembler: ContextAssembler,
    pub stores: ExecutorStores,
}

struct TraceDraft<'a> {
    graph_id: &'a str,
    task: Option<&'a TaskSpec>,
    attempt: u32,
    kind: TraceKind,
    status: ResultStatus,
    message: &'a str,
    detail: serde_json::Value,
}

macro_rules! record_trace {
    ($executor:expr, $graph_id:expr, $task:expr, $attempt:expr, $kind:expr, $status:expr, $message:expr, $detail:expr $(,)?) => {
        $executor.record_trace(TraceDraft {
            graph_id: $graph_id,
            task: $task,
            attempt: $attempt,
            kind: $kind,
            status: $status,
            message: $message,
            detail: $detail,
        })
    };
}

impl<R: AgentRunner + Sync> Executor<R> {
    pub fn new(deps: ExecutorDependencies<R>) -> Self {
        Self {
            registry: deps.registry,
            runner: deps.runner,
            reviewer: deps.reviewer,
            permission_manager: deps.permission_manager,
            context_assembler: deps.context_assembler,
            task_graph_store: deps.stores.task_graph_store,
            artifact_store: deps.stores.artifact_store,
            evidence_store: deps.stores.evidence_store,
            trace_store: deps.stores.trace_store,
        }
    }

    pub fn execute(&mut self, mut graph: TaskGraph) -> Result<OrchestrationResult, String> {
        self.task_graph_store.save(graph.clone())?;
        if graph.tasks.len() as u32 > graph.execution_budget.max_tasks {
            return Ok(OrchestrationResult {
                graph_id: graph.graph_id.clone(),
                status: ResultStatus::Failed,
                control_plane_report: None,
                task_results: BTreeMap::new(),
                review_results: BTreeMap::new(),
                trace_records: Vec::new(),
                final_output: None,
                issues: vec!["task graph exceeds max_tasks budget".to_string()],
            });
        }

        let mut task_results = BTreeMap::new();
        let mut review_results = BTreeMap::new();
        let mut successful_tasks = BTreeSet::new();
        let mut failed_tasks = BTreeSet::new();
        let mut attempt_counts: HashMap<String, u32> = HashMap::new();
        let mut iterations = 0_u32;

        for task in &graph.tasks {
            record_trace!(
                self,
                &graph.graph_id,
                Some(task),
                0,
                TraceKind::TaskQueued,
                ResultStatus::Pending,
                "task queued",
                json!({ "goal": task.goal }),
            )?;
        }

        while successful_tasks.len() + failed_tasks.len() < graph.tasks.len()
            && iterations < graph.execution_budget.max_iterations
        {
            let ready_indices = graph
                .tasks
                .iter()
                .enumerate()
                .filter(|(_, task)| {
                    !successful_tasks.contains(&task.task_id)
                        && !failed_tasks.contains(&task.task_id)
                        && task
                            .dependencies
                            .iter()
                            .all(|dependency| successful_tasks.contains(dependency))
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();

            let batch_indices = if matches!(graph.execution_route, ExecutionRoute::MultiAgent) {
                ready_indices
            } else {
                ready_indices.into_iter().take(1).collect()
            };

            if batch_indices.is_empty() {
                break;
            }

            iterations += 1;
            let mut runnable_tasks = Vec::new();

            for index in batch_indices {
                if successful_tasks.contains(&graph.tasks[index].task_id)
                    || failed_tasks.contains(&graph.tasks[index].task_id)
                {
                    continue;
                }

                let task = graph.tasks[index].clone();
                let attempt = attempt_counts.entry(task.task_id.clone()).or_insert(0);
                *attempt += 1;

                record_trace!(
                    self,
                    &graph.graph_id,
                    Some(&task),
                    *attempt,
                    TraceKind::TaskStarted,
                    ResultStatus::Running,
                    "task started",
                    json!({ "goal": task.goal }),
                )?;

                let Some(agent_card) = self.registry.get(&task.assigned_agent).cloned() else {
                    failed_tasks.insert(task.task_id.clone());
                    record_trace!(
                        self,
                        &graph.graph_id,
                        Some(&task),
                        *attempt,
                        TraceKind::TaskCompleted,
                        ResultStatus::Failed,
                        "assigned agent not found",
                        json!({ "assigned_agent": task.assigned_agent }),
                    )?;
                    continue;
                };

                let permission_decision = self.permission_manager.check(&agent_card, &task);
                match permission_decision.status {
                    PermissionStatus::Denied => {
                        let result = blocked_result(
                            &task,
                            &agent_card.id,
                            ResultStatus::Blocked,
                            &permission_decision,
                        );
                        let review = self.reviewer.review(&task, &result, &permission_decision);
                        task_results.insert(task.task_id.clone(), result);
                        review_results.insert(task.task_id.clone(), review);
                        failed_tasks.insert(task.task_id.clone());
                        record_trace!(
                            self,
                            &graph.graph_id,
                            Some(&task),
                            *attempt,
                            TraceKind::PermissionDenied,
                            ResultStatus::Blocked,
                            "permission denied",
                            json!({ "reasons": permission_decision.reasons }),
                        )?;
                        continue;
                    }
                    PermissionStatus::RequiresApproval => {
                        let result = blocked_result(
                            &task,
                            &agent_card.id,
                            ResultStatus::ApprovalRequired,
                            &permission_decision,
                        );
                        let review = self.reviewer.review(&task, &result, &permission_decision);
                        task_results.insert(task.task_id.clone(), result);
                        review_results.insert(task.task_id.clone(), review);
                        failed_tasks.insert(task.task_id.clone());
                        record_trace!(
                            self,
                            &graph.graph_id,
                            Some(&task),
                            *attempt,
                            TraceKind::ApprovalRequired,
                            ResultStatus::ApprovalRequired,
                            "approval required",
                            json!({ "reasons": permission_decision.reasons }),
                        )?;
                        continue;
                    }
                    PermissionStatus::Allowed => {}
                }

                let context = self.context_assembler.assemble(
                    &graph,
                    &task,
                    &agent_card,
                    &task_results,
                    self.evidence_store.as_ref(),
                    self.artifact_store.as_ref(),
                );

                runnable_tasks.push(RunnableTask {
                    index,
                    task,
                    attempt: *attempt,
                    agent_card,
                    permission_decision,
                    context,
                });
            }

            let executed_tasks = if matches!(graph.execution_route, ExecutionRoute::MultiAgent)
                && runnable_tasks.len() > 1
            {
                self.run_runnable_tasks_concurrently(runnable_tasks)?
            } else {
                self.run_runnable_tasks_sequentially(runnable_tasks)
            };

            for executed in executed_tasks {
                let ExecutedTask {
                    runnable_task,
                    mut result,
                } = executed;
                let RunnableTask {
                    index,
                    task,
                    attempt,
                    agent_card,
                    permission_decision,
                    ..
                } = runnable_task;

                for evidence in &result.generated_evidence {
                    self.evidence_store.save(evidence.clone())?;
                }
                for artifact in &result.generated_artifacts {
                    self.artifact_store.save(artifact.clone())?;
                }
                result.permission_status = Some(permission_decision.status);

                record_trace!(
                    self,
                    &graph.graph_id,
                    Some(&task),
                    attempt,
                    TraceKind::TaskCompleted,
                    result.status,
                    "task completed",
                    json!({ "agent_id": agent_card.id, "issues": result.issues }),
                )?;

                let review = self.reviewer.review(&task, &result, &permission_decision);
                record_trace!(
                    self,
                    &graph.graph_id,
                    Some(&task),
                    attempt,
                    TraceKind::TaskReviewed,
                    map_review_status(review.status),
                    "task reviewed",
                    json!({
                        "review_status": review.status,
                        "blocking_issues": review.blocking_issues,
                        "required_fixes": review.required_fixes,
                    }),
                )?;

                let should_retry = matches!(review.status, ReviewStatus::Revise)
                    && attempt <= task.budget.max_retries_per_task
                    && attempt <= graph.execution_budget.max_retries_per_task;

                if should_retry {
                    if let Some(task_to_update) = graph.tasks.get_mut(index) {
                        for fix in &review.required_fixes {
                            if !task_to_update.constraints.contains(fix) {
                                task_to_update.constraints.push(fix.clone());
                            }
                        }
                    }
                    record_trace!(
                        self,
                        &graph.graph_id,
                        Some(&task),
                        attempt,
                        TraceKind::RetryScheduled,
                        ResultStatus::NeedsRevision,
                        "retry scheduled",
                        json!({ "required_fixes": review.required_fixes }),
                    )?;
                } else if matches!(review.status, ReviewStatus::Pass) {
                    successful_tasks.insert(task.task_id.clone());
                } else {
                    failed_tasks.insert(task.task_id.clone());
                }

                task_results.insert(task.task_id.clone(), result);
                review_results.insert(task.task_id.clone(), review);
            }
        }

        let status = if failed_tasks.is_empty() && successful_tasks.len() == graph.tasks.len() {
            ResultStatus::Completed
        } else if review_results
            .values()
            .any(|review| matches!(review.status, ReviewStatus::Fail))
        {
            ResultStatus::Failed
        } else if task_results
            .values()
            .any(|result| matches!(result.status, ResultStatus::ApprovalRequired))
        {
            ResultStatus::ApprovalRequired
        } else {
            ResultStatus::Blocked
        };

        let final_output = graph
            .tasks
            .iter()
            .find(|task| task.assigned_agent == "reporter.final")
            .and_then(|task| task_results.get(&task.task_id))
            .or_else(|| task_results.values().last())
            .map(|result| result.output.clone());

        let trace_records = self.trace_store.list_by_graph(&graph.graph_id);
        let mut issues = Vec::new();
        if iterations >= graph.execution_budget.max_iterations {
            issues.push("execution budget max_iterations reached".to_string());
        }
        if successful_tasks.len() + failed_tasks.len() < graph.tasks.len() {
            issues.push("not all tasks could be finalized".to_string());
        }

        Ok(OrchestrationResult {
            graph_id: graph.graph_id.clone(),
            status,
            control_plane_report: None,
            task_results,
            review_results,
            trace_records,
            final_output,
            issues,
        })
    }

    fn record_trace(&mut self, draft: TraceDraft<'_>) -> Result<(), String> {
        let trace = TraceRecord {
            id: format!("trace-{}", Uuid::new_v4()),
            graph_id: draft.graph_id.to_string(),
            task_id: draft.task.map(|task| task.task_id.clone()),
            agent_id: draft.task.map(|task| task.assigned_agent.clone()),
            attempt: draft.attempt,
            kind: draft.kind,
            status: draft.status,
            message: draft.message.to_string(),
            detail: draft.detail,
            created_at: Utc::now().to_rfc3339(),
        };
        self.trace_store.append(trace)
    }

    fn run_runnable_tasks_sequentially(
        &self,
        runnable_tasks: Vec<RunnableTask>,
    ) -> Vec<ExecutedTask> {
        runnable_tasks
            .into_iter()
            .map(|runnable_task| {
                let result = self.run_with_fallback(
                    &runnable_task.agent_card,
                    &runnable_task.task,
                    &runnable_task.context,
                );
                ExecutedTask {
                    runnable_task,
                    result,
                }
            })
            .collect()
    }

    fn run_runnable_tasks_concurrently(
        &self,
        runnable_tasks: Vec<RunnableTask>,
    ) -> Result<Vec<ExecutedTask>, String> {
        let runner = &self.runner;
        thread::scope(|scope| {
            let handles = runnable_tasks
                .into_iter()
                .map(|runnable_task| {
                    scope.spawn(move || {
                        let result = match runner.run(
                            &runnable_task.agent_card,
                            &runnable_task.task,
                            &runnable_task.context,
                        ) {
                            Ok(result) => result,
                            Err(error) => runner_error_result(
                                &runnable_task.task,
                                &runnable_task.agent_card.id,
                                error,
                            ),
                        };
                        ExecutedTask {
                            runnable_task,
                            result,
                        }
                    })
                })
                .collect::<Vec<_>>();

            let mut executed = Vec::new();
            for handle in handles {
                executed.push(
                    handle
                        .join()
                        .map_err(|_| "multi_agent worker thread panicked".to_string())?,
                );
            }
            Ok(executed)
        })
    }

    fn run_with_fallback(
        &self,
        agent_card: &AgentCard,
        task: &TaskSpec,
        context: &AssembledContext,
    ) -> AgentResult {
        match self.runner.run(agent_card, task, context) {
            Ok(result) => result,
            Err(error) => runner_error_result(task, &agent_card.id, error),
        }
    }
}

fn blocked_result(
    task: &TaskSpec,
    agent_id: &str,
    status: ResultStatus,
    permission_decision: &PermissionDecision,
) -> AgentResult {
    AgentResult {
        task_id: task.task_id.clone(),
        agent_id: agent_id.to_string(),
        status,
        output: json!({ "permission_reasons": permission_decision.reasons }),
        evidence_refs: Vec::new(),
        artifact_refs: Vec::new(),
        issues: permission_decision.reasons.clone(),
        token_usage: None,
        tool_calls: None,
        generated_evidence: Vec::new(),
        generated_artifacts: Vec::new(),
        permission_status: Some(permission_decision.status),
    }
}

fn map_review_status(status: ReviewStatus) -> ResultStatus {
    match status {
        ReviewStatus::Pass => ResultStatus::Completed,
        ReviewStatus::Revise => ResultStatus::NeedsRevision,
        ReviewStatus::Fail => ResultStatus::Failed,
    }
}

struct RunnableTask {
    index: usize,
    task: TaskSpec,
    attempt: u32,
    agent_card: AgentCard,
    permission_decision: PermissionDecision,
    context: AssembledContext,
}

struct ExecutedTask {
    runnable_task: RunnableTask,
    result: AgentResult,
}

fn runner_error_result(task: &TaskSpec, agent_id: &str, error: String) -> AgentResult {
    AgentResult {
        task_id: task.task_id.clone(),
        agent_id: agent_id.to_string(),
        status: ResultStatus::Failed,
        output: json!({ "error": error }),
        evidence_refs: Vec::new(),
        artifact_refs: Vec::new(),
        issues: vec!["runner returned an error".to_string()],
        token_usage: None,
        tool_calls: None,
        generated_evidence: Vec::new(),
        generated_artifacts: Vec::new(),
        permission_status: Some(PermissionStatus::Allowed),
    }
}
