use super::models::{
    AgentResult, PermissionDecision, PermissionStatus, ReviewResult, ReviewStatus, TaskSpec,
};
use serde_json::Value;

#[derive(Debug, Default, Clone, Copy)]
pub struct Reviewer;

impl Reviewer {
    pub fn new() -> Self {
        Self
    }

    pub fn review(
        &self,
        task_spec: &TaskSpec,
        result: &AgentResult,
        permission_decision: &PermissionDecision,
    ) -> ReviewResult {
        let mut blocking_issues = Vec::new();
        let mut non_blocking_issues = Vec::new();
        let mut required_fixes = Vec::new();

        if result.task_id.trim().is_empty() || result.task_id != task_spec.task_id {
            blocking_issues.push("result task_id is missing or mismatched".to_string());
        }
        if result.agent_id.trim().is_empty() {
            blocking_issues.push("result agent_id is missing".to_string());
        }
        if is_empty_output(&result.output) {
            blocking_issues.push("result output is empty".to_string());
        }
        if !matches!(permission_decision.status, PermissionStatus::Allowed) {
            blocking_issues.push(format!(
                "permission status is {:?}: {}",
                permission_decision.status,
                permission_decision.reasons.join("; ")
            ));
        }

        for field in &task_spec.expected_output.required_fields {
            if !output_contains_field(&result.output, field) {
                required_fixes.push(format!("missing expected output field '{}'", field));
            }
        }

        if task_spec.expected_output.requires_evidence
            || task_spec.verification.required_evidence_count > 0
        {
            let required_count = task_spec.verification.required_evidence_count.max(1);
            if result.evidence_refs.len() < required_count {
                required_fixes.push(format!(
                    "expected at least {} evidence refs but found {}",
                    required_count,
                    result.evidence_refs.len()
                ));
            }
        }

        if result
            .tool_calls
            .as_ref()
            .map(|calls| {
                task_spec
                    .budget
                    .max_tool_calls
                    .map(|limit| calls.len() as u32 > limit)
                    .unwrap_or(false)
            })
            .unwrap_or(false)
        {
            blocking_issues.push("tool call budget exceeded".to_string());
        }

        if task_spec.verification.require_consistency_statement
            && !output_contains_field(&result.output, "consistency_statement")
        {
            non_blocking_issues.push("missing consistency statement".to_string());
            required_fixes.push("add a consistency_statement field".to_string());
        }

        if !task_spec.success_criteria.is_empty() {
            let coverage = result
                .output
                .get("criteria_coverage")
                .and_then(Value::as_array)
                .map(|items| items.len())
                .unwrap_or(0);
            if coverage < task_spec.success_criteria.len() {
                required_fixes.push("success criteria coverage is incomplete".to_string());
            }
        }

        let status = if !blocking_issues.is_empty() {
            ReviewStatus::Fail
        } else if !required_fixes.is_empty() {
            ReviewStatus::Revise
        } else {
            ReviewStatus::Pass
        };

        let score = ((10
            - (blocking_issues.len() * 3 + required_fixes.len() * 2 + non_blocking_issues.len()))
        .max(0) as f32)
            / 10.0;

        ReviewResult {
            task_id: task_spec.task_id.clone(),
            status,
            score,
            blocking_issues,
            non_blocking_issues,
            required_fixes,
            final_answer_allowed: matches!(status, ReviewStatus::Pass),
        }
    }
}

fn is_empty_output(output: &Value) -> bool {
    match output {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

fn output_contains_field(output: &Value, field: &str) -> bool {
    match output {
        Value::Object(map) => map.contains_key(field),
        Value::String(text) => text.contains(field),
        _ => false,
    }
}
