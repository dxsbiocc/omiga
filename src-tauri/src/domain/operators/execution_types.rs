use std::collections::{BTreeMap, BTreeSet};

use crate::domain::environment_fallback::{
    classify_provisioning_failure, fallback_suggestions, ProvisioningFailure,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

use super::{
    operator_invocation_preflight_answered_params, operator_invocation_preflight_metadata,
    operator_param_sources,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorInvocation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(default)]
    pub inputs: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub params: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub resources: BTreeMap<String, JsonValue>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SacctDiagnostic {
    pub state: String,
    pub exit_code: String,
    pub max_rss_kb: Option<u64>,
    pub elapsed: String,
    pub reason: Option<String>,
    pub req_mem: Option<String>,
    pub category: SacctFailureCategory,
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SacctFailureCategory {
    Oom,
    Timeout,
    Cancelled,
    FailedExit,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorToolError {
    pub kind: String,
    pub retryable: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous_errors: Vec<OperatorRetryAttemptSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioning_failure: Option<ProvisioningFailure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slurm_diagnostic: Option<SacctDiagnostic>,
}

impl OperatorToolError {
    pub fn new(kind: impl Into<String>, retryable: bool, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            retryable,
            message: message.into(),
            attempt: None,
            max_attempts: None,
            previous_errors: Vec::new(),
            field: None,
            run_dir: None,
            stdout_tail: None,
            stderr_tail: None,
            suggested_action: None,
            provisioning_failure: None,
            slurm_diagnostic: None,
        }
    }

    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    pub fn with_run_dir(mut self, run_dir: impl Into<String>) -> Self {
        self.run_dir = Some(run_dir.into());
        self
    }

    pub fn with_logs(mut self, stdout_tail: Option<String>, stderr_tail: Option<String>) -> Self {
        self.stdout_tail = stdout_tail;
        self.stderr_tail = stderr_tail;
        self
    }

    pub fn with_suggested_action(mut self, action: impl Into<String>) -> Self {
        self.suggested_action = Some(action.into());
        self
    }

    pub fn with_provisioning_failure(
        mut self,
        provisioning_failure: Option<ProvisioningFailure>,
    ) -> Self {
        self.provisioning_failure = provisioning_failure;
        self
    }

    pub fn with_slurm_diagnostic(mut self, diagnostic: SacctDiagnostic) -> Self {
        self.slurm_diagnostic = Some(diagnostic);
        self
    }

    pub(crate) fn with_retry_state(mut self, retry: &OperatorRetryState) -> Self {
        self.attempt = Some(retry.attempt);
        self.max_attempts = Some(retry.max_attempts);
        self.previous_errors = retry.previous_errors.clone();
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRetryAttemptSummary {
    pub attempt: u32,
    pub kind: String,
    pub retryable: bool,
    pub message: String,
}

impl OperatorRetryAttemptSummary {
    pub(crate) fn from_error(attempt: u32, error: &OperatorToolError) -> Self {
        Self {
            attempt,
            kind: error.kind.clone(),
            retryable: error.retryable,
            message: error.message.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OperatorRetryState {
    pub(crate) attempt: u32,
    pub(crate) max_attempts: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) previous_errors: Vec<OperatorRetryAttemptSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperatorRetryPolicy {
    pub(crate) max_attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OperatorRunCacheMetadata {
    pub(crate) key: String,
    #[serde(default)]
    pub(crate) hit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_run_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OperatorRunResult {
    pub(crate) status: String,
    pub(crate) run_id: String,
    pub(crate) location: String,
    pub(crate) operator: OperatorRunIdentity,
    pub(crate) run_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) run_context: Option<OperatorRunContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provenance_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) export_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) environment_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) environment_explicit_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) markdown_report: Option<String>,
    pub(crate) outputs: BTreeMap<String, Vec<ArtifactRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) structured_outputs: Option<JsonValue>,
    pub(crate) effective_inputs: BTreeMap<String, JsonValue>,
    pub(crate) input_fingerprints: BTreeMap<String, JsonValue>,
    pub(crate) effective_params: BTreeMap<String, JsonValue>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) param_sources: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) preflight: Option<JsonValue>,
    pub(crate) effective_resources: BTreeMap<String, JsonValue>,
    pub(crate) attempt: u32,
    pub(crate) max_attempts: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) previous_errors: Vec<OperatorRetryAttemptSummary>,
    pub(crate) enforcement: JsonValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cache: Option<OperatorRunCacheMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<OperatorToolError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OperatorRunIdentity {
    pub(crate) alias: String,
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) source_plugin: String,
    pub(crate) manifest_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OperatorRunStatusMetadata {
    pub(crate) run_id: String,
    pub(crate) location: String,
    pub(crate) operator: OperatorRunIdentity,
    pub(crate) run_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) run_context: Option<OperatorRunContext>,
    pub(crate) retry: Option<OperatorRetryState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunSummary {
    pub run_id: String,
    pub status: String,
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_plugin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_test_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_test_name: Option<String>,
    pub run_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export_dir: Option<String>,
    pub output_count: usize,
    pub structured_output_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slurm_diagnostic: Option<SacctDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_hit: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_source_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_source_run_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunDetail {
    pub run_id: String,
    pub location: String,
    pub run_dir: String,
    pub source_path: String,
    pub document: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunLog {
    pub run_id: String,
    pub location: String,
    pub log_name: String,
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunVerification {
    pub run_id: String,
    pub location: String,
    pub run_dir: String,
    pub ok: bool,
    pub checks: Vec<OperatorRunCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactRef {
    pub(crate) location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) server: Option<String>,
    pub(crate) path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fingerprint: Option<JsonValue>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunCleanupRequest {
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub keep_latest: Option<usize>,
    #[serde(default)]
    pub max_age_days: Option<u64>,
    #[serde(default)]
    pub include_cache_hits: bool,
    #[serde(default)]
    pub include_failed: bool,
    #[serde(default)]
    pub include_succeeded: bool,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub operator_alias: Option<String>,
    #[serde(default)]
    pub operator_id: Option<String>,
    #[serde(default)]
    pub operator_version: Option<String>,
    #[serde(default)]
    pub source_plugin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunCleanupCandidate {
    pub run_id: String,
    pub status: String,
    pub location: String,
    pub run_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_hit: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_source_run_id: Option<String>,
    pub output_count: usize,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_bytes: Option<u64>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunCleanupResult {
    pub dry_run: bool,
    pub location: String,
    pub runs_root: String,
    pub scanned_count: usize,
    pub matched_count: usize,
    pub deleted_count: usize,
    pub skipped_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_bytes: Option<u64>,
    pub candidates: Vec<OperatorRunCleanupCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunCheck {
    pub name: String,
    pub ok: bool,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperatorExecutionSurfaceKind {
    Local,
    Ssh,
    Sandbox,
}

#[derive(Debug, Clone)]
pub(crate) struct OperatorExecutionSurface {
    pub(crate) kind: OperatorExecutionSurfaceKind,
    pub(crate) run_dir: String,
}

impl OperatorExecutionSurface {
    pub(crate) fn for_context(ctx: &crate::domain::tools::ToolContext, run_id: &str) -> Self {
        match ctx.execution_environment.as_str() {
            "ssh" => Self {
                kind: OperatorExecutionSurfaceKind::Ssh,
                run_dir: crate::domain::tools::env_store::remote_path(
                    ctx,
                    &super::operator_run_relative_path(run_id),
                ),
            },
            "sandbox" | "remote" => Self {
                kind: OperatorExecutionSurfaceKind::Sandbox,
                run_dir: crate::domain::tools::env_store::remote_path(
                    ctx,
                    &super::operator_run_relative_path(run_id),
                ),
            },
            _ => Self {
                kind: OperatorExecutionSurfaceKind::Local,
                run_dir: super::operator_run_dir(&ctx.project_root, run_id)
                    .to_string_lossy()
                    .into_owned(),
            },
        }
    }

    pub(crate) fn for_runs_root(ctx: &crate::domain::tools::ToolContext) -> Self {
        match ctx.execution_environment.as_str() {
            "ssh" => Self {
                kind: OperatorExecutionSurfaceKind::Ssh,
                run_dir: crate::domain::tools::env_store::remote_path(
                    ctx,
                    &super::operator_runs_relative_path(),
                ),
            },
            "sandbox" | "remote" => Self {
                kind: OperatorExecutionSurfaceKind::Sandbox,
                run_dir: crate::domain::tools::env_store::remote_path(
                    ctx,
                    &super::operator_runs_relative_path(),
                ),
            },
            _ => Self {
                kind: OperatorExecutionSurfaceKind::Local,
                run_dir: super::operator_runs_root(&ctx.project_root)
                    .to_string_lossy()
                    .into_owned(),
            },
        }
    }

    pub(crate) fn is_environment(&self) -> bool {
        self.kind != OperatorExecutionSurfaceKind::Local
    }

    pub(crate) fn artifact_location(&self) -> &'static str {
        match self.kind {
            OperatorExecutionSurfaceKind::Local => "local",
            OperatorExecutionSurfaceKind::Ssh => "ssh",
            OperatorExecutionSurfaceKind::Sandbox => "sandbox",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_test_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_test_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_execution_id: Option<String>,
    /// When true, skip cache lookup even if the operator manifest enables caching.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bypass_cache: bool,
}

impl OperatorRunContext {
    pub(crate) fn is_empty(&self) -> bool {
        self.kind.as_deref().unwrap_or_default().trim().is_empty()
            && self
                .smoke_test_id
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            && self
                .smoke_test_name
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            && self
                .parent_execution_id
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
    }

    pub(crate) fn normalized(self) -> Option<Self> {
        let normalized = Self {
            kind: super::manifest::normalize_optional_string(self.kind),
            smoke_test_id: super::manifest::normalize_optional_string(self.smoke_test_id),
            smoke_test_name: super::manifest::normalize_optional_string(self.smoke_test_name),
            parent_execution_id: super::manifest::normalize_optional_string(
                self.parent_execution_id,
            ),
            bypass_cache: self.bypass_cache,
        };
        (!normalized.is_empty()).then_some(normalized)
    }
}

pub(crate) fn failure_json(
    alias: &str,
    resolved: Option<&super::ResolvedOperator>,
    run_dir: Option<&str>,
    run_context: Option<OperatorRunContext>,
    error: OperatorToolError,
) -> String {
    let identity = resolved.map(|resolved| OperatorRunIdentity {
        alias: alias.to_string(),
        id: resolved.spec.metadata.id.clone(),
        version: resolved.spec.metadata.version.clone(),
        source_plugin: resolved.spec.source.source_plugin.clone(),
        manifest_path: resolved
            .spec
            .source
            .manifest_path
            .to_string_lossy()
            .into_owned(),
    });
    serde_json::to_string_pretty(&json!({
        "status": "failed",
        "operator": identity,
        "runDir": run_dir,
        "runContext": run_context,
        "error": error,
    }))
    .unwrap_or_else(|_| "{\"status\":\"failed\"}".to_string())
}

pub(crate) fn provisioning_failure_for_error(
    exit_code: Option<i64>,
    stderr_tail: Option<&str>,
) -> Option<ProvisioningFailure> {
    let kind = classify_provisioning_failure(exit_code, stderr_tail.unwrap_or_default())?;
    let availability = crate::domain::environment_availability::load_cache();
    let records = availability.records.values().cloned().collect::<Vec<_>>();
    let suggestions = fallback_suggestions(&kind, &records);
    Some(ProvisioningFailure { kind, suggestions })
}

pub(crate) async fn record_operator_success_best_effort(
    ctx: &crate::domain::tools::ToolContext,
    result: &OperatorRunResult,
    started_at: &str,
) {
    let output_summary = operator_output_summary(result);
    let mut metadata = json!({
        "runId": result.run_id,
        "runDir": result.run_dir,
        "provenancePath": result.provenance_path,
        "exportDir": result.export_dir,
        "markdownReport": result.markdown_report,
        "operatorAlias": result.operator.alias,
        "runContext": result.run_context,
        "paramSources": result.param_sources,
        "cache": result.cache,
    });
    if let Some(preflight) = &result.preflight {
        metadata["preflight"] = preflight.clone();
    }
    let selected_params = selected_params_for_source(
        &result.effective_params,
        &result.param_sources,
        "user_preflight",
    );
    if !selected_params.is_empty() {
        metadata["selectedParams"] = json!(selected_params);
    }
    let record = crate::domain::execution_records::ExecutionRecordInput {
        kind: "operator".to_string(),
        unit_id: Some(result.operator.id.clone()),
        canonical_id: Some(canonical_operator_unit_id(&result.operator)),
        provider_plugin: Some(result.operator.source_plugin.clone()),
        status: result.status.clone(),
        session_id: ctx.session_id.clone(),
        parent_execution_id: result
            .run_context
            .as_ref()
            .and_then(|context| context.parent_execution_id.clone()),
        started_at: Some(started_at.to_string()),
        ended_at: Some(chrono::Utc::now().to_rfc3339()),
        input_hash: crate::domain::execution_records::hash_execution_map(&result.effective_inputs),
        param_hash: crate::domain::execution_records::hash_execution_map(&result.effective_params),
        output_summary_json: Some(output_summary),
        runtime_json: Some(result.enforcement.clone()),
        metadata_json: Some(metadata),
    };
    crate::domain::execution_records::record_execution_best_effort(&ctx.project_root, record).await;
}

fn selected_params_for_source(
    effective_params: &BTreeMap<String, JsonValue>,
    param_sources: &BTreeMap<String, String>,
    wanted_source: &str,
) -> BTreeMap<String, JsonValue> {
    effective_params
        .iter()
        .filter_map(|(param, value)| {
            param_sources
                .get(param)
                .filter(|source| source.as_str() == wanted_source)
                .map(|_| (param.clone(), value.clone()))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn record_operator_failure_best_effort(
    ctx: &crate::domain::tools::ToolContext,
    alias: &str,
    resolved: Option<&super::ResolvedOperator>,
    run_dir: Option<&str>,
    invocation: Option<&super::OperatorInvocation>,
    run_context: Option<OperatorRunContext>,
    started_at: &str,
    error: &OperatorToolError,
) {
    let operator = resolved.map(|resolved| OperatorRunIdentity {
        alias: alias.to_string(),
        id: resolved.spec.metadata.id.clone(),
        version: resolved.spec.metadata.version.clone(),
        source_plugin: resolved.spec.source.source_plugin.clone(),
        manifest_path: resolved
            .spec
            .source
            .manifest_path
            .to_string_lossy()
            .into_owned(),
    });
    let mut metadata = json!({
        "runDir": run_dir,
        "operatorAlias": alias,
        "operator": operator,
        "runContext": run_context,
        "error": error,
    });
    if let (Some(resolved), Some(invocation)) = (resolved, invocation) {
        let preflight_param_names = operator_invocation_preflight_answered_params(invocation);
        let supplied_param_names = invocation.params.keys().cloned().collect::<BTreeSet<_>>();
        metadata["paramSources"] = json!(operator_param_sources(
            &resolved.spec,
            &supplied_param_names,
            &preflight_param_names,
            &invocation.params,
        ));
        if let Some(preflight) = operator_invocation_preflight_metadata(invocation) {
            metadata["preflight"] = preflight;
        }
    }
    let (input_hash, param_hash) = invocation
        .map(|invocation| {
            (
                crate::domain::execution_records::hash_execution_map(&invocation.inputs),
                crate::domain::execution_records::hash_execution_map(&invocation.params),
            )
        })
        .unwrap_or((None, None));
    let output_summary = json!({
        "errorKind": error.kind,
        "retryable": error.retryable,
        "runDir": run_dir,
    });
    let record = crate::domain::execution_records::ExecutionRecordInput {
        kind: "operator".to_string(),
        unit_id: operator.as_ref().map(|operator| operator.id.clone()),
        canonical_id: operator.as_ref().map(canonical_operator_unit_id),
        provider_plugin: operator
            .as_ref()
            .map(|operator| operator.source_plugin.clone()),
        status: "failed".to_string(),
        session_id: ctx.session_id.clone(),
        parent_execution_id: run_context
            .as_ref()
            .and_then(|context| context.parent_execution_id.clone()),
        started_at: Some(started_at.to_string()),
        ended_at: Some(chrono::Utc::now().to_rfc3339()),
        input_hash,
        param_hash,
        output_summary_json: Some(output_summary),
        runtime_json: Some(enforcement_json_for_context(ctx)),
        metadata_json: Some(metadata),
    };
    crate::domain::execution_records::record_execution_best_effort(&ctx.project_root, record).await;
}

pub(crate) fn operator_output_summary(result: &OperatorRunResult) -> JsonValue {
    let output_artifact_count = result
        .outputs
        .values()
        .map(|artifacts| artifacts.len())
        .sum::<usize>();
    json!({
        "runId": result.run_id,
        "outputKeys": result.outputs.keys().cloned().collect::<Vec<_>>(),
        "outputArtifactCount": output_artifact_count,
        "structuredOutputCount": result
            .structured_outputs
            .as_ref()
            .map(output_artifact_count_json)
            .unwrap_or(0),
        "status": result.status,
    })
}

fn output_artifact_count_json(value: &JsonValue) -> usize {
    value.as_object().map(|object| object.len()).unwrap_or(0)
}

fn enforcement_json_for_context(ctx: &crate::domain::tools::ToolContext) -> JsonValue {
    json!({
        "executionEnvironment": ctx.execution_environment,
        "sandboxBackend": ctx.sandbox_backend,
        "localVenvType": ctx.local_venv_type,
        "localVenvName": ctx.local_venv_name,
    })
}

pub(crate) fn canonical_operator_unit_id(operator: &OperatorRunIdentity) -> String {
    format!("{}/operator/{}", operator.source_plugin, operator.id.trim())
}

pub(crate) fn canonical_operator_unit_id_for_spec(spec: &super::OperatorSpec) -> String {
    format!(
        "{}/operator/{}",
        spec.source.source_plugin,
        spec.metadata.id.trim()
    )
}
