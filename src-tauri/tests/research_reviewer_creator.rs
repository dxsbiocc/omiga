use omiga_lib::domain::research_system::{
    AgentPatchAction, AgentPatchProposal, AgentRegistry, AgentRegistryStore, AgentResult,
    ApprovalStatus, BudgetSpec, Creator, InMemoryAgentRegistryStore, InMemoryProposalStore,
    OutputContract, PermissionDecision, PermissionSpec, PermissionStatus, ProposalStore,
    RegistryPatchMode, ResultStatus, ReviewStatus, Reviewer, TaskSpec, TraceKind, TraceRecord,
    VerificationSpec,
};
use serde_json::json;

fn task_requiring_evidence() -> TaskSpec {
    TaskSpec {
        task_id: "task-1".to_string(),
        goal: "Summarize evidence".to_string(),
        assigned_agent: "reporter.final".to_string(),
        dependencies: Vec::new(),
        input_refs: Vec::new(),
        constraints: Vec::new(),
        expected_output: OutputContract {
            format: "json".to_string(),
            required_fields: vec!["final_report".to_string()],
            requires_evidence: true,
            minimum_artifacts: 0,
            schema_hint: json!({}),
        },
        success_criteria: vec!["Return a reviewable result".to_string()],
        verification: VerificationSpec {
            required_checks: vec!["shape".to_string()],
            required_evidence_count: 1,
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
fn reviewer_marks_missing_evidence_for_revision() {
    let task = task_requiring_evidence();
    let result = AgentResult {
        task_id: "task-1".to_string(),
        agent_id: "reporter.final".to_string(),
        status: ResultStatus::Completed,
        output: json!({
            "final_report": "Mock report",
            "criteria_coverage": ["Return a reviewable result"],
            "consistency_statement": "ok"
        }),
        evidence_refs: Vec::new(),
        artifact_refs: Vec::new(),
        issues: Vec::new(),
        token_usage: None,
        tool_calls: None,
        generated_evidence: Vec::new(),
        generated_artifacts: Vec::new(),
        permission_status: Some(PermissionStatus::Allowed),
    };

    let review = Reviewer::new().review(
        &task,
        &result,
        &PermissionDecision {
            status: PermissionStatus::Allowed,
            reasons: Vec::new(),
        },
    );

    assert!(matches!(review.status, ReviewStatus::Revise));
    assert!(!review.required_fixes.is_empty());
}

#[test]
fn creator_generates_proposal_from_repeated_failures_without_mutating_registry() {
    let registry = AgentRegistry::default_registry().expect("default registry");
    let before_count = registry.list().len();
    let creator = Creator::new();
    let traces = vec![
        TraceRecord {
            id: "trace-1".to_string(),
            graph_id: "graph-1".to_string(),
            task_id: Some("task-1".to_string()),
            agent_id: Some("analyzer.data".to_string()),
            attempt: 1,
            kind: TraceKind::TaskReviewed,
            status: ResultStatus::NeedsRevision,
            message: "revise".to_string(),
            detail: json!({}),
            created_at: "2026-04-24T00:00:00Z".to_string(),
        },
        TraceRecord {
            id: "trace-2".to_string(),
            graph_id: "graph-1".to_string(),
            task_id: Some("task-2".to_string()),
            agent_id: Some("analyzer.data".to_string()),
            attempt: 1,
            kind: TraceKind::TaskReviewed,
            status: ResultStatus::Failed,
            message: "fail".to_string(),
            detail: json!({}),
            created_at: "2026-04-24T00:01:00Z".to_string(),
        },
        TraceRecord {
            id: "trace-3".to_string(),
            graph_id: "graph-1".to_string(),
            task_id: Some("task-3".to_string()),
            agent_id: Some("reporter.final".to_string()),
            attempt: 0,
            kind: TraceKind::TaskQueued,
            status: ResultStatus::Pending,
            message: "queued".to_string(),
            detail: json!({ "goal": "Summarize single cell differential expression methods" }),
            created_at: "2026-04-24T00:02:00Z".to_string(),
        },
        TraceRecord {
            id: "trace-4".to_string(),
            graph_id: "graph-2".to_string(),
            task_id: Some("task-4".to_string()),
            agent_id: Some("reporter.final".to_string()),
            attempt: 0,
            kind: TraceKind::TaskQueued,
            status: ResultStatus::Pending,
            message: "queued".to_string(),
            detail: json!({ "goal": "Summarize single cell differential expression methods" }),
            created_at: "2026-04-24T00:03:00Z".to_string(),
        },
        TraceRecord {
            id: "trace-5".to_string(),
            graph_id: "graph-3".to_string(),
            task_id: Some("task-5".to_string()),
            agent_id: Some("reporter.final".to_string()),
            attempt: 0,
            kind: TraceKind::TaskQueued,
            status: ResultStatus::Pending,
            message: "queued".to_string(),
            detail: json!({ "goal": "Summarize single cell differential expression methods" }),
            created_at: "2026-04-24T00:04:00Z".to_string(),
        },
    ];

    let proposals = creator.analyze_traces(&registry, &traces);
    assert!(proposals.len() >= 2);
    assert_eq!(registry.list().len(), before_count);
    assert!(proposals
        .iter()
        .all(|proposal| matches!(proposal.approval_status, ApprovalStatus::Pending)));
}

#[test]
fn creator_approves_split_with_manual_patch_plan_without_mutating_registry() {
    let creator = Creator::new();
    let mut proposal_store = InMemoryProposalStore::default();
    let registry = AgentRegistry::default_registry().expect("default registry");
    let before_count = registry.list().len();
    let mut registry_store = InMemoryAgentRegistryStore::new(registry.clone());
    let proposal = AgentPatchProposal {
        proposal_id: "proposal-split".to_string(),
        action: AgentPatchAction::Split,
        candidate_agent: None,
        target_agents: vec!["analyzer.data".to_string()],
        reason: "Repeated failure pattern".to_string(),
        expected_benefit: "Narrow the capability boundary".to_string(),
        required_tools: Vec::new(),
        eval_plan: vec!["Replay the failing cases".to_string()],
        rollback_plan: vec!["Restore the current agent card".to_string()],
        approval_status: ApprovalStatus::Pending,
        registry_patch: None,
    };
    proposal_store
        .save(proposal)
        .expect("proposal save should work");

    let approved = creator
        .approve_proposal(
            "proposal-split",
            &mut proposal_store,
            Some(&mut registry_store),
        )
        .expect("split approval should now return a manual patch plan");
    assert!(matches!(approved.approval_status, ApprovalStatus::Approved));
    let patch = approved
        .registry_patch
        .expect("split proposal should include a patch plan");
    assert!(matches!(patch.mode, RegistryPatchMode::Manual));
    assert_eq!(patch.draft_cards.len(), 2);
    assert!(!patch.steps.is_empty());

    let stored = proposal_store
        .get("proposal-split")
        .expect("proposal should still exist");
    assert!(matches!(stored.approval_status, ApprovalStatus::Approved));
    assert!(stored.registry_patch.is_some());

    let after_registry = registry_store.load().expect("load registry");
    assert_eq!(after_registry.list().len(), before_count);
    assert!(after_registry.get("analyzer.data").is_some());
}

#[test]
fn creator_approves_merge_with_manual_patch_plan() {
    let creator = Creator::new();
    let mut proposal_store = InMemoryProposalStore::default();
    let registry = AgentRegistry::default_registry().expect("default registry");
    let mut registry_store = InMemoryAgentRegistryStore::new(registry);
    let proposal = AgentPatchProposal {
        proposal_id: "proposal-merge".to_string(),
        action: AgentPatchAction::Merge,
        candidate_agent: None,
        target_agents: vec!["analyzer.data".to_string(), "algorithm.method".to_string()],
        reason: "The two agents repeatedly overlap".to_string(),
        expected_benefit: "Reduce routing ambiguity".to_string(),
        required_tools: Vec::new(),
        eval_plan: vec!["Replay the overlap-heavy requests".to_string()],
        rollback_plan: vec!["Restore the source cards".to_string()],
        approval_status: ApprovalStatus::Pending,
        registry_patch: None,
    };
    proposal_store
        .save(proposal)
        .expect("proposal save should work");

    let approved = creator
        .approve_proposal(
            "proposal-merge",
            &mut proposal_store,
            Some(&mut registry_store),
        )
        .expect("merge approval should produce a manual patch plan");
    assert!(matches!(approved.approval_status, ApprovalStatus::Approved));
    let patch = approved
        .registry_patch
        .expect("merge proposal should include a patch plan");
    assert!(matches!(patch.mode, RegistryPatchMode::Manual));
    assert_eq!(patch.draft_cards.len(), 1);
    assert!(patch.summary.contains("merged"));
}
