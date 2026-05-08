use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AgentToolPolicy {
    #[serde(default)]
    pub allowed: Vec<String>,
    #[serde(default)]
    pub forbidden: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PermissionSpec {
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub write: Vec<String>,
    #[serde(default)]
    pub execute: Vec<String>,
    #[serde(default)]
    pub external_side_effect: Vec<String>,
    #[serde(default)]
    pub human_approval_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MemoryScope {
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub write: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ContextPolicy {
    #[serde(default = "default_max_input_tokens")]
    pub max_input_tokens: usize,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub summarization_required: bool,
}

const fn default_max_input_tokens() -> usize {
    6_000
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentCard {
    pub id: String,
    pub name: String,
    pub version: String,
    pub category: String,
    pub description: String,
    #[serde(default)]
    pub use_when: Vec<String>,
    #[serde(default)]
    pub avoid_when: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub tools: AgentToolPolicy,
    #[serde(default)]
    pub permissions: PermissionSpec,
    #[serde(default)]
    pub memory_scope: MemoryScope,
    #[serde(default)]
    pub context_policy: ContextPolicy,
    #[serde(default = "empty_object")]
    pub input_schema: Value,
    #[serde(default = "empty_object")]
    pub output_schema: Value,
    #[serde(default)]
    pub handoff_targets: Vec<String>,
    #[serde(default)]
    pub failure_modes: Vec<String>,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default)]
    pub evals: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct OutputContract {
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub required_fields: Vec<String>,
    #[serde(default)]
    pub requires_evidence: bool,
    #[serde(default)]
    pub minimum_artifacts: usize,
    #[serde(default = "empty_object")]
    pub schema_hint: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct VerificationSpec {
    #[serde(default)]
    pub required_checks: Vec<String>,
    #[serde(default)]
    pub required_evidence_count: usize,
    #[serde(default)]
    pub require_test_results: bool,
    #[serde(default)]
    pub require_consistency_statement: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BudgetSpec {
    pub max_iterations: u32,
    pub max_retries_per_task: u32,
    pub max_tasks: u32,
    #[serde(default)]
    pub max_tool_calls: Option<u32>,
}

impl Default for BudgetSpec {
    fn default() -> Self {
        Self {
            max_iterations: 6,
            max_retries_per_task: 1,
            max_tasks: 8,
            max_tool_calls: Some(6),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskSpec {
    pub task_id: String,
    pub goal: String,
    pub assigned_agent: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub input_refs: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub expected_output: OutputContract,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default)]
    pub verification: VerificationSpec,
    #[serde(default)]
    pub failure_conditions: Vec<String>,
    #[serde(default)]
    pub stop_conditions: Vec<String>,
    #[serde(default)]
    pub budget: BudgetSpec,
    #[serde(default)]
    pub requested_tools: Vec<String>,
    #[serde(default)]
    pub requested_permissions: PermissionSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskEdge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskGraph {
    pub graph_id: String,
    pub user_goal: String,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub ambiguities: Vec<String>,
    #[serde(default)]
    pub execution_route: ExecutionRoute,
    #[serde(default)]
    pub tasks: Vec<TaskSpec>,
    #[serde(default)]
    pub edges: Vec<TaskEdge>,
    #[serde(default)]
    pub global_constraints: Vec<String>,
    #[serde(default)]
    pub final_output_contract: OutputContract,
    #[serde(default)]
    pub execution_budget: BudgetSpec,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionRoute {
    #[default]
    Solo,
    Workflow,
    MultiAgent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct IntakeAssessment {
    pub user_goal: String,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub ambiguities: Vec<String>,
    #[serde(default)]
    pub complexity_score: usize,
    #[serde(default)]
    pub execution_route: ExecutionRoute,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    Pending,
    Running,
    Completed,
    NeedsRevision,
    Failed,
    Blocked,
    ApprovalRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TokenUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ToolCallRecord {
    pub tool_name: String,
    #[serde(default)]
    pub arguments: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceRecord {
    pub id: String,
    pub task_id: String,
    pub summary: String,
    pub source: String,
    pub quality: String,
    #[serde(default)]
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactRecord {
    pub id: String,
    pub task_id: String,
    pub name: String,
    pub kind: String,
    pub location: String,
    #[serde(default = "empty_object")]
    pub content: Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionStatus {
    Allowed,
    RequiresApproval,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionDecision {
    pub status: PermissionStatus,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentResult {
    pub task_id: String,
    pub agent_id: String,
    pub status: ResultStatus,
    #[serde(default = "empty_object")]
    pub output: Value,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub issues: Vec<String>,
    #[serde(default)]
    pub token_usage: Option<TokenUsage>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCallRecord>>,
    #[serde(default)]
    pub generated_evidence: Vec<EvidenceRecord>,
    #[serde(default)]
    pub generated_artifacts: Vec<ArtifactRecord>,
    #[serde(default)]
    pub permission_status: Option<PermissionStatus>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Pass,
    Revise,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewResult {
    pub task_id: String,
    pub status: ReviewStatus,
    pub score: f32,
    #[serde(default)]
    pub blocking_issues: Vec<String>,
    #[serde(default)]
    pub non_blocking_issues: Vec<String>,
    #[serde(default)]
    pub required_fixes: Vec<String>,
    pub final_answer_allowed: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TraceKind {
    TaskQueued,
    TaskStarted,
    TaskCompleted,
    TaskReviewed,
    RetryScheduled,
    PermissionDenied,
    ApprovalRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceRecord {
    pub id: String,
    pub graph_id: String,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    pub attempt: u32,
    pub kind: TraceKind,
    pub status: ResultStatus,
    pub message: String,
    #[serde(default = "empty_object")]
    pub detail: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentPatchAction {
    Create,
    Split,
    Merge,
    Retire,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Applied,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegistryPatchMode {
    Applied,
    Manual,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegistryPatchAction {
    CreateCard,
    DisableAgent,
    MergeCards,
    UpdateRouting,
    ManualReview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryPatchStep {
    pub action: RegistryPatchAction,
    #[serde(default)]
    pub target_agent: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentRegistryPatch {
    pub mode: RegistryPatchMode,
    pub summary: String,
    #[serde(default)]
    pub steps: Vec<RegistryPatchStep>,
    #[serde(default)]
    pub draft_cards: Vec<AgentCard>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentPatchProposal {
    pub proposal_id: String,
    pub action: AgentPatchAction,
    #[serde(default)]
    pub candidate_agent: Option<AgentCard>,
    #[serde(default)]
    pub target_agents: Vec<String>,
    pub reason: String,
    pub expected_benefit: String,
    #[serde(default)]
    pub required_tools: Vec<String>,
    #[serde(default)]
    pub eval_plan: Vec<String>,
    #[serde(default)]
    pub rollback_plan: Vec<String>,
    pub approval_status: ApprovalStatus,
    #[serde(default)]
    pub registry_patch: Option<AgentRegistryPatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssembledContext {
    pub agent_id: String,
    pub task_id: String,
    #[serde(default)]
    pub sections: BTreeMap<String, Value>,
    #[serde(default)]
    pub omitted_sections: Vec<String>,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrchestrationResult {
    pub graph_id: String,
    pub status: ResultStatus,
    #[serde(default)]
    pub control_plane_report: Option<ControlPlaneReport>,
    #[serde(default)]
    pub task_results: BTreeMap<String, AgentResult>,
    #[serde(default)]
    pub review_results: BTreeMap<String, ReviewResult>,
    #[serde(default)]
    pub trace_records: Vec<TraceRecord>,
    #[serde(default)]
    pub final_output: Option<Value>,
    #[serde(default)]
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ControlPlaneReport {
    pub intake_assessment: IntakeAssessment,
    pub intake_result: AgentResult,
    pub intake_review: ReviewResult,
    pub planner_result: AgentResult,
    pub planner_review: ReviewResult,
}

fn default_true() -> bool {
    true
}

fn empty_object() -> Value {
    Value::Object(Default::default())
}
