use omiga_lib::domain::research_system::{
    AgentCard, AgentRegistry, AgentResult, AgentRunner, AssembledContext, BudgetSpec,
    ContextAssembler, ExecutionRoute, Executor, ExecutorDependencies, ExecutorStores,
    InMemoryArtifactStore, InMemoryEvidenceStore, InMemoryTaskGraphStore, InMemoryTraceStore,
    MockAgentRunner, OutputContract, PermissionManager, PermissionSpec, ResultStatus, ReviewStatus,
    Reviewer, TaskEdge, TaskGraph, TaskSpec, TraceKind, VerificationSpec,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn make_task(
    task_id: &str,
    goal: &str,
    agent: &str,
    deps: Vec<&str>,
    requires_evidence: bool,
) -> TaskSpec {
    TaskSpec {
        task_id: task_id.to_string(),
        goal: goal.to_string(),
        assigned_agent: agent.to_string(),
        dependencies: deps.into_iter().map(str::to_string).collect(),
        input_refs: Vec::new(),
        constraints: Vec::new(),
        expected_output: match agent {
            "seeker.web_research" => OutputContract {
                format: "json".to_string(),
                required_fields: vec!["findings".to_string(), "evidence_refs".to_string()],
                requires_evidence: true,
                minimum_artifacts: 0,
                schema_hint: json!({}),
            },
            "analyzer.data" => OutputContract {
                format: "json".to_string(),
                required_fields: vec!["analysis".to_string(), "conclusions".to_string()],
                requires_evidence,
                minimum_artifacts: 0,
                schema_hint: json!({}),
            },
            "painter.visualization" => OutputContract {
                format: "json".to_string(),
                required_fields: vec!["visualization_plan".to_string()],
                requires_evidence: false,
                minimum_artifacts: 1,
                schema_hint: json!({}),
            },
            _ => OutputContract {
                format: "json".to_string(),
                required_fields: vec!["final_report".to_string(), "citations".to_string()],
                requires_evidence,
                minimum_artifacts: 0,
                schema_hint: json!({}),
            },
        },
        success_criteria: vec!["Produce a reviewable structured result.".to_string()],
        verification: VerificationSpec {
            required_checks: vec!["shape".to_string()],
            required_evidence_count: if requires_evidence { 1 } else { 0 },
            require_test_results: false,
            require_consistency_statement: true,
        },
        failure_conditions: Vec::new(),
        stop_conditions: Vec::new(),
        budget: BudgetSpec {
            max_iterations: 4,
            max_retries_per_task: 1,
            max_tasks: 8,
            max_tool_calls: Some(4),
        },
        requested_tools: if agent == "seeker.web_research" {
            vec!["web_search".to_string()]
        } else {
            Vec::new()
        },
        requested_permissions: if agent == "seeker.web_research" {
            PermissionSpec {
                read: vec!["web".to_string()],
                write: vec!["evidence_store".to_string()],
                execute: Vec::new(),
                external_side_effect: Vec::new(),
                human_approval_required: false,
            }
        } else {
            Default::default()
        },
    }
}

fn make_executor<R: AgentRunner + Sync>(registry: AgentRegistry, runner: R) -> Executor<R> {
    Executor::new(ExecutorDependencies {
        registry,
        runner,
        reviewer: Reviewer::new(),
        permission_manager: PermissionManager::new(),
        context_assembler: ContextAssembler::new(),
        stores: ExecutorStores {
            task_graph_store: Box::new(InMemoryTaskGraphStore::default()),
            artifact_store: Box::new(InMemoryArtifactStore::default()),
            evidence_store: Box::new(InMemoryEvidenceStore::default()),
            trace_store: Box::new(InMemoryTraceStore::default()),
        },
    })
}

#[test]
fn executor_can_run_simple_task_graph() {
    let registry = AgentRegistry::default_registry().expect("default registry");
    let graph = TaskGraph {
        graph_id: "graph-simple".to_string(),
        user_goal: "write a report".to_string(),
        assumptions: vec!["mock mode".to_string()],
        ambiguities: Vec::new(),
        execution_route: ExecutionRoute::Solo,
        tasks: vec![make_task(
            "report-1",
            "Write the final report",
            "reporter.final",
            vec![],
            false,
        )],
        edges: Vec::new(),
        global_constraints: vec!["review required".to_string()],
        final_output_contract: OutputContract {
            format: "json".to_string(),
            required_fields: vec!["final_report".to_string()],
            requires_evidence: false,
            minimum_artifacts: 0,
            schema_hint: json!({}),
        },
        execution_budget: BudgetSpec {
            max_iterations: 4,
            max_retries_per_task: 1,
            max_tasks: 4,
            max_tool_calls: Some(4),
        },
    };

    let mut executor = make_executor(registry, MockAgentRunner::new());

    let result = executor.execute(graph).expect("execute");
    assert!(matches!(result.status, ResultStatus::Completed));
    assert_eq!(result.task_results.len(), 1);
    assert_eq!(result.review_results.len(), 1);
    assert!(matches!(
        result.review_results["report-1"].status,
        ReviewStatus::Pass
    ));
}

#[test]
fn executor_respects_dependency_order_for_multiple_tasks() {
    let registry = AgentRegistry::default_registry().expect("default registry");
    let graph = TaskGraph {
        graph_id: "graph-complex".to_string(),
        user_goal: "research and report".to_string(),
        assumptions: vec!["mock mode".to_string()],
        ambiguities: Vec::new(),
        execution_route: ExecutionRoute::Workflow,
        tasks: vec![
            make_task(
                "retrieve-1",
                "Retrieve evidence",
                "seeker.web_research",
                vec![],
                true,
            ),
            make_task(
                "analyze-1",
                "Analyze evidence",
                "analyzer.data",
                vec!["retrieve-1"],
                true,
            ),
            make_task(
                "report-1",
                "Write report",
                "reporter.final",
                vec!["analyze-1"],
                true,
            ),
        ],
        edges: vec![
            TaskEdge {
                from: "retrieve-1".to_string(),
                to: "analyze-1".to_string(),
            },
            TaskEdge {
                from: "analyze-1".to_string(),
                to: "report-1".to_string(),
            },
        ],
        global_constraints: vec!["review required".to_string()],
        final_output_contract: OutputContract {
            format: "json".to_string(),
            required_fields: vec!["final_report".to_string()],
            requires_evidence: true,
            minimum_artifacts: 0,
            schema_hint: json!({}),
        },
        execution_budget: BudgetSpec {
            max_iterations: 6,
            max_retries_per_task: 1,
            max_tasks: 6,
            max_tool_calls: Some(6),
        },
    };

    let mut executor = make_executor(registry, MockAgentRunner::new());

    let result = executor.execute(graph).expect("execute");
    let started_order = result
        .trace_records
        .iter()
        .filter(|trace| matches!(trace.kind, TraceKind::TaskStarted))
        .filter_map(|trace| trace.task_id.clone())
        .collect::<Vec<_>>();

    assert_eq!(started_order, vec!["retrieve-1", "analyze-1", "report-1"]);
    assert_eq!(result.task_results.len(), 3);
}

#[test]
fn executor_allows_multi_agent_batches_to_fit_iteration_budget() {
    let registry = AgentRegistry::default_registry().expect("default registry");
    let graph = TaskGraph {
        graph_id: "graph-multi-agent".to_string(),
        user_goal: "research in parallel".to_string(),
        assumptions: vec!["mock mode".to_string()],
        ambiguities: Vec::new(),
        execution_route: ExecutionRoute::MultiAgent,
        tasks: vec![
            make_task(
                "retrieve-1",
                "Retrieve evidence",
                "seeker.web_research",
                vec![],
                true,
            ),
            make_task(
                "analyze-1",
                "Analyze evidence",
                "analyzer.data",
                vec!["retrieve-1"],
                true,
            ),
            make_task(
                "visualize-1",
                "Design visualization",
                "painter.visualization",
                vec!["retrieve-1"],
                false,
            ),
            make_task(
                "report-1",
                "Write report",
                "reporter.final",
                vec!["analyze-1", "visualize-1"],
                true,
            ),
        ],
        edges: vec![
            TaskEdge {
                from: "retrieve-1".to_string(),
                to: "analyze-1".to_string(),
            },
            TaskEdge {
                from: "retrieve-1".to_string(),
                to: "visualize-1".to_string(),
            },
            TaskEdge {
                from: "analyze-1".to_string(),
                to: "report-1".to_string(),
            },
            TaskEdge {
                from: "visualize-1".to_string(),
                to: "report-1".to_string(),
            },
        ],
        global_constraints: vec!["review required".to_string()],
        final_output_contract: OutputContract {
            format: "json".to_string(),
            required_fields: vec!["final_report".to_string()],
            requires_evidence: true,
            minimum_artifacts: 0,
            schema_hint: json!({}),
        },
        execution_budget: BudgetSpec {
            max_iterations: 3,
            max_retries_per_task: 1,
            max_tasks: 6,
            max_tool_calls: Some(6),
        },
    };

    let mut executor = make_executor(registry, MockAgentRunner::new());

    let result = executor.execute(graph).expect("execute");

    assert!(matches!(result.status, ResultStatus::Completed));
    assert_eq!(result.task_results.len(), 4);
}

#[derive(Clone)]
struct ProbeConcurrentRunner {
    current: Arc<AtomicUsize>,
    max_seen: Arc<AtomicUsize>,
}

impl ProbeConcurrentRunner {
    fn new() -> Self {
        Self {
            current: Arc::new(AtomicUsize::new(0)),
            max_seen: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn max_seen(&self) -> usize {
        self.max_seen.load(Ordering::SeqCst)
    }
}

impl AgentRunner for ProbeConcurrentRunner {
    fn run(
        &self,
        agent_card: &AgentCard,
        task_spec: &TaskSpec,
        _context: &AssembledContext,
    ) -> Result<AgentResult, String> {
        let running = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self
            .max_seen
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |prev| {
                Some(prev.max(running))
            });
        thread::sleep(Duration::from_millis(50));
        self.current.fetch_sub(1, Ordering::SeqCst);

        Ok(AgentResult {
            task_id: task_spec.task_id.clone(),
            agent_id: agent_card.id.clone(),
            status: ResultStatus::Completed,
            output: json!({
                "final_report": format!("mock report for {}", task_spec.task_id),
                "citations": [],
                "criteria_coverage": task_spec.success_criteria,
                "consistency_statement": "probe runner output",
            }),
            evidence_refs: Vec::new(),
            artifact_refs: Vec::new(),
            issues: Vec::new(),
            token_usage: None,
            tool_calls: None,
            generated_evidence: Vec::new(),
            generated_artifacts: Vec::new(),
            permission_status: Some(omiga_lib::domain::research_system::PermissionStatus::Allowed),
        })
    }
}

#[test]
fn executor_runs_multi_agent_ready_tasks_concurrently() {
    let registry = AgentRegistry::default_registry().expect("default registry");
    let runner = ProbeConcurrentRunner::new();
    let graph = TaskGraph {
        graph_id: "graph-multi-agent-concurrent".to_string(),
        user_goal: "parallel report synthesis".to_string(),
        assumptions: vec!["probe mode".to_string()],
        ambiguities: Vec::new(),
        execution_route: ExecutionRoute::MultiAgent,
        tasks: vec![
            make_task(
                "report-a",
                "Write report A",
                "reporter.final",
                vec![],
                false,
            ),
            make_task(
                "report-b",
                "Write report B",
                "reporter.final",
                vec![],
                false,
            ),
            make_task(
                "report-final",
                "Write report final",
                "reporter.final",
                vec!["report-a", "report-b"],
                false,
            ),
        ],
        edges: vec![
            TaskEdge {
                from: "report-a".to_string(),
                to: "report-final".to_string(),
            },
            TaskEdge {
                from: "report-b".to_string(),
                to: "report-final".to_string(),
            },
        ],
        global_constraints: vec!["review required".to_string()],
        final_output_contract: OutputContract {
            format: "json".to_string(),
            required_fields: vec!["final_report".to_string()],
            requires_evidence: false,
            minimum_artifacts: 0,
            schema_hint: json!({}),
        },
        execution_budget: BudgetSpec {
            max_iterations: 3,
            max_retries_per_task: 1,
            max_tasks: 5,
            max_tool_calls: Some(4),
        },
    };

    let mut executor = make_executor(registry, runner.clone());

    let result = executor.execute(graph).expect("execute");

    assert!(matches!(result.status, ResultStatus::Completed));
    assert!(runner.max_seen() >= 2);
}
