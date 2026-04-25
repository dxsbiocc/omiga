use omiga_lib::domain::research_system::{
    AgentRegistry, ArtifactRecord, ArtifactStore, ContextAssembler, InMemoryArtifactStore,
    InMemoryEvidenceStore, PermissionManager, TaskGraph, TaskSpec,
};
use omiga_lib::domain::research_system::{
    BudgetSpec, ExecutionRoute, OutputContract, PermissionSpec, VerificationSpec,
};
use serde_json::json;

fn sample_task(task_id: &str, agent: &str) -> TaskSpec {
    TaskSpec {
        task_id: task_id.to_string(),
        goal: "sample goal".to_string(),
        assigned_agent: agent.to_string(),
        dependencies: Vec::new(),
        input_refs: vec!["artifact-1".to_string()],
        constraints: Vec::new(),
        expected_output: OutputContract {
            format: "json".to_string(),
            required_fields: vec!["final_report".to_string()],
            requires_evidence: false,
            minimum_artifacts: 0,
            schema_hint: json!({}),
        },
        success_criteria: vec!["Return a structured answer.".to_string()],
        verification: VerificationSpec {
            required_checks: vec!["shape".to_string()],
            required_evidence_count: 0,
            require_test_results: false,
            require_consistency_statement: true,
        },
        failure_conditions: Vec::new(),
        stop_conditions: Vec::new(),
        budget: BudgetSpec::default(),
        requested_tools: Vec::new(),
        requested_permissions: PermissionSpec::default(),
    }
}

#[test]
fn context_assembler_respects_exclude_policy() {
    let registry = AgentRegistry::default_registry().expect("default registry");
    let mut reporter = registry.get("reporter.final").expect("reporter").clone();
    reporter
        .context_policy
        .exclude
        .push("artifact_refs".to_string());

    let graph = TaskGraph {
        graph_id: "graph-1".to_string(),
        user_goal: "sample goal".to_string(),
        assumptions: vec!["assume concise output".to_string()],
        ambiguities: Vec::new(),
        execution_route: ExecutionRoute::Solo,
        tasks: vec![sample_task("task-1", "reporter.final")],
        edges: Vec::new(),
        global_constraints: vec!["no destructive actions".to_string()],
        final_output_contract: OutputContract::default(),
        execution_budget: BudgetSpec::default(),
    };

    let mut artifact_store = InMemoryArtifactStore::default();
    artifact_store
        .save(ArtifactRecord {
            id: "artifact-1".to_string(),
            task_id: "seed".to_string(),
            name: "seed.json".to_string(),
            kind: "data".to_string(),
            location: "memory://artifact-1".to_string(),
            content: json!({"ok": true}),
        })
        .expect("artifact save");
    let evidence_store = InMemoryEvidenceStore::default();

    let assembled = ContextAssembler::new().assemble(
        &graph,
        &graph.tasks[0],
        &reporter,
        &Default::default(),
        &evidence_store,
        &artifact_store,
    );

    assert!(!assembled.sections.contains_key("artifact_refs"));
    assert!(assembled
        .omitted_sections
        .contains(&"artifact_refs".to_string()));
}

#[test]
fn context_assembler_keeps_global_context_for_specialists() {
    let registry = AgentRegistry::default_registry().expect("default registry");
    let analyzer = registry.get("analyzer.data").expect("analyzer");

    let graph = TaskGraph {
        graph_id: "graph-2".to_string(),
        user_goal: "sample goal".to_string(),
        assumptions: vec!["assume concise output".to_string()],
        ambiguities: vec!["missing final deliverable".to_string()],
        execution_route: ExecutionRoute::Workflow,
        tasks: vec![sample_task("task-2", "analyzer.data")],
        edges: Vec::new(),
        global_constraints: vec!["no destructive actions".to_string()],
        final_output_contract: OutputContract::default(),
        execution_budget: BudgetSpec::default(),
    };

    let assembled = ContextAssembler::new().assemble(
        &graph,
        &graph.tasks[0],
        analyzer,
        &Default::default(),
        &InMemoryEvidenceStore::default(),
        &InMemoryArtifactStore::default(),
    );

    let global_context = assembled
        .sections
        .get("global_context")
        .expect("global_context should be preserved");
    assert_eq!(global_context["execution_route"], json!("workflow"));
    assert_eq!(
        global_context["ambiguities"],
        json!(["missing final deliverable"])
    );
}

#[test]
fn context_assembler_truncates_structured_sections() {
    let registry = AgentRegistry::default_registry().expect("default registry");
    let mut reporter = registry.get("reporter.final").expect("reporter").clone();
    reporter.context_policy.max_input_tokens = 10;

    let mut task = sample_task("task-3", "reporter.final");
    task.constraints = vec![
        "x".repeat(200),
        "y".repeat(200),
        "z".repeat(200),
        "w".repeat(200),
    ];

    let graph = TaskGraph {
        graph_id: "graph-3".to_string(),
        user_goal: "sample goal".to_string(),
        assumptions: vec!["assume concise output".to_string()],
        ambiguities: vec!["ambiguous".to_string()],
        execution_route: ExecutionRoute::Workflow,
        tasks: vec![task],
        edges: Vec::new(),
        global_constraints: vec!["no destructive actions".to_string()],
        final_output_contract: OutputContract::default(),
        execution_budget: BudgetSpec::default(),
    };

    let assembled = ContextAssembler::new().assemble(
        &graph,
        &graph.tasks[0],
        &reporter,
        &Default::default(),
        &InMemoryEvidenceStore::default(),
        &InMemoryArtifactStore::default(),
    );

    let task_spec = assembled
        .sections
        .get("task_spec")
        .expect("task_spec should be present");
    assert!(task_spec.to_string().chars().count() <= 40);
}

#[test]
fn permission_manager_rejects_forbidden_tool() {
    let registry = AgentRegistry::default_registry().expect("default registry");
    let seeker = registry.get("seeker.web_research").expect("seeker");
    let mut task = sample_task("task-1", "seeker.web_research");
    task.requested_tools = vec!["shell".to_string()];

    let decision = PermissionManager::new().check(seeker, &task);
    assert!(matches!(
        decision.status,
        omiga_lib::domain::research_system::PermissionStatus::Denied
    ));
}
