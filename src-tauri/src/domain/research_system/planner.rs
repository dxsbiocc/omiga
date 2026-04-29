use super::intake::IntakeAnalyzer;
use super::models::{
    BudgetSpec, ExecutionRoute, IntakeAssessment, OutputContract, PermissionSpec, TaskEdge,
    TaskGraph, TaskSpec, VerificationSpec,
};
use uuid::Uuid;

pub trait PlanningModel {
    fn plan(&self, user_request: &str) -> Result<TaskGraph, String>;
}

pub struct Planner {
    intake: IntakeAnalyzer,
    model: Option<Box<dyn PlanningModel + Send + Sync>>,
}

impl Default for Planner {
    fn default() -> Self {
        Self::new()
    }
}

impl Planner {
    pub fn new() -> Self {
        Self {
            intake: IntakeAnalyzer::new(),
            model: None,
        }
    }

    pub fn with_model(model: Box<dyn PlanningModel + Send + Sync>) -> Self {
        Self {
            intake: IntakeAnalyzer::new(),
            model: Some(model),
        }
    }

    pub fn analyze_intake(&self, user_request: &str) -> IntakeAssessment {
        self.intake.analyze(user_request)
    }

    pub fn plan(&self, user_request: &str) -> Result<TaskGraph, String> {
        if let Some(model) = &self.model {
            return model.plan(user_request);
        }
        Ok(self.heuristic_plan(user_request))
    }

    fn heuristic_plan(&self, user_request: &str) -> TaskGraph {
        let intake = self.analyze_intake(user_request);
        let lowered = user_request.to_lowercase();
        let retrieval = contains_any(
            &lowered,
            &[
                "检索",
                "文献",
                "论文",
                "search",
                "research",
                "rna-seq",
                "scrna",
                "single-cell",
            ],
        );
        let analysis = contains_any(
            &lowered,
            &["分析", "analysis", "compare", "适用场景", "interpret"],
        );
        let visualization = contains_any(
            &lowered,
            &["可视化", "visual", "plot", "chart", "figure", "图"],
        );
        let report = contains_any(&lowered, &["报告", "report", "总结", "汇报"]) || analysis;
        let code = contains_any(&lowered, &["代码", "实现", "code", "implement"]);
        let debug = contains_any(&lowered, &["调试", "debug", "修复", "error", "fix"]);
        let biology = contains_any(&lowered, &["生物", "rna", "single cell", "单细胞", "seq"]);
        let method = contains_any(&lowered, &["方法", "method", "算法", "algorithm"]);
        let processing = contains_any(
            &lowered,
            &["清洗", "process", "格式", "预处理", "normalize"],
        );

        let intent = ResearchIntentFlags {
            retrieval,
            analysis,
            visualization,
            report,
            code,
            debug,
            biology,
            method,
            processing,
        };

        if matches!(intake.execution_route, ExecutionRoute::Solo) {
            return build_simple_graph(user_request, &lowered, &intake);
        }

        build_complex_graph(user_request, intent, &intake)
    }
}

#[derive(Debug, Clone, Copy)]
struct ResearchIntentFlags {
    retrieval: bool,
    analysis: bool,
    visualization: bool,
    report: bool,
    code: bool,
    debug: bool,
    biology: bool,
    method: bool,
    processing: bool,
}

fn build_simple_graph(user_request: &str, lowered: &str, intake: &IntakeAssessment) -> TaskGraph {
    let agent = if contains_any(lowered, &["代码", "实现", "code"]) {
        "programmer.code"
    } else if contains_any(lowered, &["debug", "error", "修复"]) {
        "debugger.error"
    } else if contains_any(lowered, &["检索", "search", "文献"]) {
        "seeker.web_research"
    } else if contains_any(lowered, &["分析", "analysis"]) {
        "analyzer.data"
    } else {
        "reporter.final"
    };
    let task = base_task("task-1", user_request, agent)
        .with_expected_output(simple_output_contract(agent))
        .with_verification(VerificationSpec {
            required_checks: vec!["shape".to_string(), "consistency".to_string()],
            required_evidence_count: if agent == "seeker.web_research" { 1 } else { 0 },
            require_test_results: false,
            require_consistency_statement: true,
        });

    TaskGraph {
        graph_id: format!("graph-{}", Uuid::new_v4()),
        user_goal: user_request.to_string(),
        assumptions: merge_assumptions(
            &intake.assumptions,
            &["Single-task flow is enough for this request shape.".to_string()],
        ),
        ambiguities: intake.ambiguities.clone(),
        execution_route: intake.execution_route,
        tasks: vec![task],
        edges: Vec::new(),
        global_constraints: default_global_constraints(),
        final_output_contract: simple_output_contract(agent),
        execution_budget: BudgetSpec {
            max_iterations: 3,
            max_retries_per_task: 1,
            max_tasks: 1,
            max_tool_calls: Some(4),
        },
    }
}

fn build_complex_graph(
    user_request: &str,
    intent: ResearchIntentFlags,
    intake: &IntakeAssessment,
) -> TaskGraph {
    let mut tasks = Vec::new();
    let mut edges = Vec::new();
    let mut latest_dependencies: Vec<String> = Vec::new();
    let execution_budget = BudgetSpec {
        max_iterations: 8,
        max_retries_per_task: 2,
        max_tasks: 10,
        max_tool_calls: Some(8),
    };

    if intent.retrieval {
        let task = base_task(
            "retrieve-1",
            format!("Collect external evidence for: {}", user_request),
            "seeker.web_research",
        )
        .with_expected_output(OutputContract {
            format: "json".to_string(),
            required_fields: vec!["findings".to_string(), "evidence_refs".to_string()],
            requires_evidence: true,
            minimum_artifacts: 0,
            schema_hint: serde_json::json!({"findings": ["string"], "evidence_refs": ["string"]}),
        })
        .with_verification(VerificationSpec {
            required_checks: vec![
                "evidence_presence".to_string(),
                "source_quality".to_string(),
            ],
            required_evidence_count: 2,
            require_test_results: false,
            require_consistency_statement: true,
        })
        .with_requested_tools(vec!["search".to_string()])
        .with_requested_permissions(PermissionSpec {
            read: vec!["web".to_string()],
            write: vec!["evidence_store".to_string()],
            execute: Vec::new(),
            external_side_effect: Vec::new(),
            human_approval_required: false,
        });
        latest_dependencies.push(task.task_id.clone());
        tasks.push(task);
    }

    if intent.processing {
        let task = base_task(
            "process-1",
            "Normalize upstream evidence or data into a comparable structure",
            "processor.data",
        )
        .with_dependencies(latest_dependencies.clone())
        .with_expected_output(OutputContract {
            format: "json".to_string(),
            required_fields: vec!["transformed_output".to_string()],
            requires_evidence: false,
            minimum_artifacts: 1,
            schema_hint: serde_json::json!({"transformed_output": "string"}),
        })
        .with_verification(VerificationSpec {
            required_checks: vec!["transformation_trace".to_string()],
            required_evidence_count: 0,
            require_test_results: false,
            require_consistency_statement: true,
        });
        edges.extend(task.dependencies.iter().map(|dep| TaskEdge {
            from: dep.clone(),
            to: task.task_id.clone(),
        }));
        latest_dependencies = vec![task.task_id.clone()];
        tasks.push(task);
    }

    let analysis_dependencies = latest_dependencies.clone();
    let mut analysis_outputs = Vec::new();

    if intent.analysis {
        let task = base_task(
            "analyze-1",
            "Analyze retrieved material and summarize applicability or trade-offs",
            "analyzer.data",
        )
        .with_dependencies(analysis_dependencies.clone())
        .with_expected_output(OutputContract {
            format: "json".to_string(),
            required_fields: vec!["analysis".to_string(), "conclusions".to_string()],
            requires_evidence: intent.retrieval,
            minimum_artifacts: 0,
            schema_hint: serde_json::json!({"analysis": "string", "conclusions": ["string"]}),
        })
        .with_verification(VerificationSpec {
            required_checks: vec!["conclusion_alignment".to_string()],
            required_evidence_count: if intent.retrieval { 1 } else { 0 },
            require_test_results: false,
            require_consistency_statement: true,
        });
        edges.extend(task.dependencies.iter().map(|dep| TaskEdge {
            from: dep.clone(),
            to: task.task_id.clone(),
        }));
        analysis_outputs.push(task.task_id.clone());
        tasks.push(task);
    }

    if intent.method {
        let task = base_task(
            "method-1",
            "Recommend methods and explain when they apply",
            "algorithm.method",
        )
        .with_dependencies(analysis_dependencies.clone())
        .with_expected_output(OutputContract {
            format: "json".to_string(),
            required_fields: vec!["recommendation".to_string(), "tradeoffs".to_string()],
            requires_evidence: intent.retrieval,
            minimum_artifacts: 0,
            schema_hint: serde_json::json!({"recommendation": "string", "tradeoffs": ["string"]}),
        })
        .with_verification(VerificationSpec {
            required_checks: vec!["tradeoff_coverage".to_string()],
            required_evidence_count: if intent.retrieval { 1 } else { 0 },
            require_test_results: false,
            require_consistency_statement: true,
        });
        edges.extend(task.dependencies.iter().map(|dep| TaskEdge {
            from: dep.clone(),
            to: task.task_id.clone(),
        }));
        analysis_outputs.push(task.task_id.clone());
        tasks.push(task);
    }

    if intent.biology {
        let task = base_task(
            "biology-1",
            "Interpret the evidence from a biology domain perspective",
            "biologist.domain",
        )
        .with_dependencies(analysis_dependencies.clone())
        .with_expected_output(OutputContract {
            format: "json".to_string(),
            required_fields: vec![
                "biological_interpretation".to_string(),
                "hypotheses".to_string(),
            ],
            requires_evidence: intent.retrieval,
            minimum_artifacts: 0,
            schema_hint: serde_json::json!({"biological_interpretation": "string", "hypotheses": ["string"]}),
        })
        .with_verification(VerificationSpec {
            required_checks: vec!["hypothesis_boundary".to_string()],
            required_evidence_count: if intent.retrieval { 1 } else { 0 },
            require_test_results: false,
            require_consistency_statement: true,
        });
        edges.extend(task.dependencies.iter().map(|dep| TaskEdge {
            from: dep.clone(),
            to: task.task_id.clone(),
        }));
        analysis_outputs.push(task.task_id.clone());
        tasks.push(task);
    }

    if intent.code {
        let code_dependencies = if analysis_outputs.is_empty() {
            analysis_dependencies.clone()
        } else {
            analysis_outputs.clone()
        };
        let task = base_task(
            "code-1",
            "Implement the requested solution",
            "programmer.code",
        )
        .with_dependencies(code_dependencies)
        .with_expected_output(OutputContract {
            format: "json".to_string(),
            required_fields: vec!["code".to_string(), "change_summary".to_string()],
            requires_evidence: false,
            minimum_artifacts: 1,
            schema_hint: serde_json::json!({"code": "string", "change_summary": "string"}),
        })
        .with_verification(VerificationSpec {
            required_checks: vec!["code_contract".to_string()],
            required_evidence_count: 0,
            require_test_results: false,
            require_consistency_statement: true,
        })
        .with_requested_tools(vec!["shell".to_string()])
        .with_requested_permissions(PermissionSpec {
            read: vec!["task_context".to_string()],
            write: vec!["artifact_store".to_string()],
            execute: vec!["test_runner".to_string()],
            external_side_effect: Vec::new(),
            human_approval_required: true,
        });
        edges.extend(task.dependencies.iter().map(|dep| TaskEdge {
            from: dep.clone(),
            to: task.task_id.clone(),
        }));
        analysis_outputs.push(task.task_id.clone());
        tasks.push(task);
    }

    if intent.debug {
        let debug_dependencies = if analysis_outputs.is_empty() {
            analysis_dependencies.clone()
        } else {
            analysis_outputs.clone()
        };
        let task = base_task(
            "debug-1",
            "Investigate failures and recommend targeted fixes",
            "debugger.error",
        )
        .with_dependencies(debug_dependencies)
        .with_expected_output(OutputContract {
            format: "json".to_string(),
            required_fields: vec!["root_cause".to_string(), "fix_plan".to_string()],
            requires_evidence: false,
            minimum_artifacts: 0,
            schema_hint: serde_json::json!({"root_cause": "string", "fix_plan": "string"}),
        })
        .with_verification(VerificationSpec {
            required_checks: vec!["failure_trace_alignment".to_string()],
            required_evidence_count: 0,
            require_test_results: false,
            require_consistency_statement: true,
        })
        .with_requested_tools(vec!["shell".to_string()])
        .with_requested_permissions(PermissionSpec {
            read: vec!["trace_store".to_string()],
            write: vec!["artifact_store".to_string()],
            execute: vec!["test_runner".to_string()],
            external_side_effect: Vec::new(),
            human_approval_required: true,
        });
        edges.extend(task.dependencies.iter().map(|dep| TaskEdge {
            from: dep.clone(),
            to: task.task_id.clone(),
        }));
        analysis_outputs.push(task.task_id.clone());
        tasks.push(task);
    }

    if intent.visualization {
        let viz_dependencies = if analysis_outputs.is_empty() {
            analysis_dependencies.clone()
        } else {
            analysis_outputs.clone()
        };
        let task = base_task(
            "visualize-1",
            "Design visualization recommendations or specs",
            "painter.visualization",
        )
        .with_dependencies(viz_dependencies)
        .with_expected_output(OutputContract {
            format: "json".to_string(),
            required_fields: vec!["visualization_plan".to_string()],
            requires_evidence: false,
            minimum_artifacts: 1,
            schema_hint: serde_json::json!({"visualization_plan": "string"}),
        })
        .with_verification(VerificationSpec {
            required_checks: vec!["visual_fit".to_string()],
            required_evidence_count: 0,
            require_test_results: false,
            require_consistency_statement: true,
        });
        edges.extend(task.dependencies.iter().map(|dep| TaskEdge {
            from: dep.clone(),
            to: task.task_id.clone(),
        }));
        analysis_outputs.push(task.task_id.clone());
        tasks.push(task);
    }

    if intent.report || analysis_outputs.len() > 1 || tasks.len() > 1 {
        let dependencies = if analysis_outputs.is_empty() {
            latest_dependencies.clone()
        } else {
            analysis_outputs.clone()
        };
        let task = base_task(
            "report-1",
            "Synthesize the final answer for the user",
            "reporter.final",
        )
        .with_dependencies(dependencies)
        .with_expected_output(OutputContract {
            format: "json".to_string(),
            required_fields: vec!["final_report".to_string(), "citations".to_string()],
            requires_evidence: intent.retrieval,
            minimum_artifacts: 0,
            schema_hint: serde_json::json!({"final_report": "string", "citations": ["string"]}),
        })
        .with_verification(VerificationSpec {
            required_checks: vec!["report_consistency".to_string()],
            required_evidence_count: if intent.retrieval { 1 } else { 0 },
            require_test_results: false,
            require_consistency_statement: true,
        });
        edges.extend(task.dependencies.iter().map(|dep| TaskEdge {
            from: dep.clone(),
            to: task.task_id.clone(),
        }));
        tasks.push(task);
    }

    TaskGraph {
        graph_id: format!("graph-{}", Uuid::new_v4()),
        user_goal: user_request.to_string(),
        assumptions: merge_assumptions(
            &intake.assumptions,
            &["The executor will re-check permissions before each task.".to_string()],
        ),
        ambiguities: intake.ambiguities.clone(),
        execution_route: intake.execution_route,
        tasks,
        edges,
        global_constraints: default_global_constraints(),
        final_output_contract: OutputContract {
            format: "json".to_string(),
            required_fields: vec!["final_report".to_string()],
            requires_evidence: intent.retrieval,
            minimum_artifacts: if intent.visualization { 1 } else { 0 },
            schema_hint: serde_json::json!({"final_report": "string"}),
        },
        execution_budget,
    }
}

fn default_global_constraints() -> Vec<String> {
    vec![
        "No destructive actions.".to_string(),
        "All high-risk actions require permission checks.".to_string(),
        "Reviewer verdict is required before final answer.".to_string(),
    ]
}

fn simple_output_contract(agent: &str) -> OutputContract {
    match agent {
        "seeker.web_research" => OutputContract {
            format: "json".to_string(),
            required_fields: vec!["findings".to_string(), "evidence_refs".to_string()],
            requires_evidence: true,
            minimum_artifacts: 0,
            schema_hint: serde_json::json!({"findings": ["string"], "evidence_refs": ["string"]}),
        },
        "programmer.code" => OutputContract {
            format: "json".to_string(),
            required_fields: vec!["code".to_string(), "change_summary".to_string()],
            requires_evidence: false,
            minimum_artifacts: 1,
            schema_hint: serde_json::json!({"code": "string", "change_summary": "string"}),
        },
        "debugger.error" => OutputContract {
            format: "json".to_string(),
            required_fields: vec!["root_cause".to_string(), "fix_plan".to_string()],
            requires_evidence: false,
            minimum_artifacts: 0,
            schema_hint: serde_json::json!({"root_cause": "string", "fix_plan": "string"}),
        },
        "analyzer.data" => OutputContract {
            format: "json".to_string(),
            required_fields: vec!["analysis".to_string(), "conclusions".to_string()],
            requires_evidence: false,
            minimum_artifacts: 0,
            schema_hint: serde_json::json!({"analysis": "string", "conclusions": ["string"]}),
        },
        _ => OutputContract {
            format: "json".to_string(),
            required_fields: vec!["final_report".to_string()],
            requires_evidence: false,
            minimum_artifacts: 0,
            schema_hint: serde_json::json!({"final_report": "string"}),
        },
    }
}

fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|keyword| text.contains(keyword))
}

fn merge_assumptions(base: &[String], additions: &[String]) -> Vec<String> {
    let mut merged = base.to_vec();
    for item in additions {
        if !merged.contains(item) {
            merged.push(item.clone());
        }
    }
    merged
}

trait TaskSpecExt {
    fn with_dependencies(self, dependencies: Vec<String>) -> Self;
    fn with_expected_output(self, expected_output: OutputContract) -> Self;
    fn with_verification(self, verification: VerificationSpec) -> Self;
    fn with_requested_tools(self, requested_tools: Vec<String>) -> Self;
    fn with_requested_permissions(self, requested_permissions: PermissionSpec) -> Self;
}

impl TaskSpecExt for TaskSpec {
    fn with_dependencies(mut self, dependencies: Vec<String>) -> Self {
        self.dependencies = dependencies;
        self
    }

    fn with_expected_output(mut self, expected_output: OutputContract) -> Self {
        self.expected_output = expected_output;
        self
    }

    fn with_verification(mut self, verification: VerificationSpec) -> Self {
        self.verification = verification;
        self
    }

    fn with_requested_tools(mut self, requested_tools: Vec<String>) -> Self {
        self.requested_tools = requested_tools;
        self
    }

    fn with_requested_permissions(mut self, requested_permissions: PermissionSpec) -> Self {
        self.requested_permissions = requested_permissions;
        self
    }
}

fn base_task(
    task_id: impl Into<String>,
    goal: impl Into<String>,
    agent: impl Into<String>,
) -> TaskSpec {
    TaskSpec {
        task_id: task_id.into(),
        goal: goal.into(),
        assigned_agent: agent.into(),
        dependencies: Vec::new(),
        input_refs: Vec::new(),
        constraints: vec!["Do not hide uncertainty.".to_string()],
        expected_output: OutputContract::default(),
        success_criteria: vec!["Produce a structured result that can be reviewed.".to_string()],
        verification: VerificationSpec::default(),
        failure_conditions: vec!["Result is empty or unverifiable.".to_string()],
        stop_conditions: vec!["Budget exhausted.".to_string()],
        budget: BudgetSpec {
            max_iterations: 4,
            max_retries_per_task: 1,
            max_tasks: 10,
            max_tool_calls: Some(6),
        },
        requested_tools: Vec::new(),
        requested_permissions: PermissionSpec::default(),
    }
}
