//! Omiga operator runtime.
//!
//! Operators are plugin-provided, declarative program adapters.  A single
//! Operator represents one executable program; program subcommands or modes are
//! modeled as Operation parameters and are executed through the generic
//! `operator_execute` tool.  The legacy dynamic `operator__{id}` compatibility
//! path remains available for existing callers, but it is no longer the primary
//! model-facing discovery surface.
//!
//! The MVP keeps rich structured errors and explicit execution context in one
//! module so UI/model responses can include actionable field/run/log metadata.
//! Revisit these clippy allowances when the runtime is split into smaller
//! registry/validation/execution modules.

#![allow(clippy::result_large_err, clippy::too_many_arguments)]

use crate::domain::env_hygiene;
use serde::Serialize;
use serde_json::{json, Map as JsonMap, Value as JsonValue};
#[cfg(test)]
use std::collections::HashMap;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};

mod chains;
mod conda_env;
mod container;
mod execution;
mod execution_types;
mod io_support;
mod manifest;
mod outputs;
mod registry;
mod scripts;
mod slurm;
mod validation;

pub use execution::{
    execute_operator_tool_call, execute_operator_tool_call_with_context,
    execute_resolved_operator_tool_call_with_context, with_operator_queue_status_sender,
    OperatorQueueStatusSender,
};

pub use outputs::{
    cleanup_operator_runs_for_context, list_operator_runs_for_context,
    read_operator_run_for_context, read_operator_run_log_for_context,
    verify_operator_run_for_context,
};

pub(crate) use execution::{cache_config_enabled, operator_manifest_diagnostics_from_plugins};
pub(crate) use outputs::{
    operator_run_dir, operator_run_relative_path, operator_runs_relative_path, operator_runs_root,
};

pub(crate) use conda_env::{
    operator_conda_environment_command, operator_conda_environment_selection,
    MICROMAMBA_BOOTSTRAP_SHELL,
};

pub(crate) use container::{
    container_runtime_prepare_script, containerized_operator_command,
    operator_container_for_command, operator_container_selection_for_profile,
    operator_environment_profile, operator_environment_ref_error_command,
    OperatorContainerImagePrepare, OperatorContainerKind,
};

pub(crate) use slurm::{execute_via_slurm, operator_uses_slurm_scheduler};

pub(crate) use outputs::{
    collect_environment_outputs, collect_local_outputs, read_environment_json,
    read_environment_structured_outputs, read_local_structured_outputs, update_environment_status,
    update_local_status, validate_structured_outputs_against_manifest, write_environment_json,
};

#[cfg(test)]
pub(crate) use execution::{
    execute_resolved_operator, operator_execution_command, operator_retry_policy,
    parse_remote_path_fingerprint, runtime_supported, sha256_file, should_retry_operator_error,
};

#[cfg(test)]
pub(crate) use slurm::parse_sacct_diagnostic_output;

// Intentionally no extra test re-exports; duplicates are intentionally avoided above.

pub(crate) use execution_types::{
    failure_json, provisioning_failure_for_error, record_operator_failure_best_effort,
    record_operator_success_best_effort, ArtifactRef, OperatorExecutionSurface,
    OperatorExecutionSurfaceKind, OperatorRetryPolicy, OperatorRetryState,
    OperatorRunCacheMetadata, OperatorRunCheck, OperatorRunIdentity, OperatorRunResult,
    OperatorRunStatusMetadata,
};
pub use execution_types::{
    OperatorInvocation, OperatorRetryAttemptSummary, OperatorRunCleanupCandidate,
    OperatorRunCleanupRequest, OperatorRunCleanupResult, OperatorRunContext, OperatorRunDetail,
    OperatorRunLog, OperatorRunSummary, OperatorRunVerification, OperatorToolError,
    SacctDiagnostic, SacctFailureCategory,
};

pub use chains::{
    delete_user_chain_template, list_user_chain_templates, save_user_chain_template,
    save_user_script_operator, user_chains_dir, user_operators_dir, ChainStep, ChainTemplate,
    UserOperatorInput, UserOperatorOutput, UserOperatorParam,
};
#[cfg(test)]
pub(crate) use conda_env::{conda_environment_shell_script, OperatorCondaEnvironmentSelection};
#[cfg(test)]
pub(crate) use container::container_runtime_preflight_script;
pub(crate) use container::selected_direct_container;
pub(crate) use execution::{
    execute_env_command, operator_environment_cwd, operator_environment_manifest_dir,
    operator_profile_relative_path, operator_runtime_env_ref, profile_runtime_extra_str,
    runtime_axis_values, safe_operator_env_component, sha256_hex, validate_output_glob_pattern,
};
pub(crate) use io_support::{
    current_epoch_ms, read_json_value, read_tail, read_tail_limited, safe_relative_string,
    write_json_file,
};
#[cfg(test)]
pub(crate) use manifest::OperatorPreflightAskWhen;
pub(crate) use manifest::{
    discover_manifest_paths, load_operator_manifest, validate_operator_id,
    OperatorCandidateSummary, OperatorExecutionSpec, OperatorFieldKind, OperatorFieldSpec,
    OperatorInterfaceSpec, OperatorManifestDiagnostic, OperatorMetadata,
    OperatorOperationGroupSummary, OperatorOperationSpec, OperatorOperationSummary,
    OperatorPreflightOptionSpec, OperatorPreflightQuestionSpec, OperatorPreflightSpec,
    OperatorRegistryEntry, OperatorRegistryFile, OperatorRegistryUpdate, OperatorResourceSpec,
    OperatorSource, OperatorSpec, ResolvedOperator, OPERATOR_API_VERSION_V1ALPHA1, OPERATOR_KIND,
};
#[cfg(test)]
pub(crate) use outputs::{
    apply_status_metadata, list_local_operator_runs, read_local_operator_run,
};
pub(crate) use outputs::{
    export_environment_operator_results, export_local_operator_results, is_safe_operator_run_id,
    json_string_at, operator_result_markdown_report, remote_tail, rfc3339_sort_key,
    summarize_local_operator_run_dir, summarize_operator_run_documents,
    write_environment_operator_result_readme, write_local_operator_result_readme,
};
#[cfg(test)]
pub(crate) use registry::{
    apply_operator_registry_update, discover_operator_candidates_from_plugins,
    format_enabled_operator_tools_system_section_from_resolved, resolve_enabled_operators_from,
};
pub use registry::{
    describe_operator, discover_operator_candidates, enabled_operator_tool_schemas,
    format_enabled_operator_tools_system_section, list_operator_summaries, load_registry_file,
    operator_favorites, registry_path, resolve_enabled_operators, resolve_operator_alias,
    set_operator_enabled,
};
pub(crate) use registry::{operator_candidate_summary, operator_operation_summaries};
pub(crate) use scripts::{command_with_log_capture, sh_quote, shell_join};
pub(crate) use validation::{
    apply_equal_bindings, apply_param_defaults, apply_resource_defaults_and_overrides,
    canonicalize_inputs, expand_argv, list_operator_summaries_for_plugin_root,
    operator_execute_preflight_question_with_project_preferences,
    operator_invocation_preflight_answered_params, operator_invocation_preflight_metadata,
    operator_invocation_preflight_param_sources, operator_operation_from_invocation,
    operator_param_sources, operator_parameters_schema,
    operator_preflight_question_for_spec_with_recommended_params,
    operator_preflight_question_with_project_preferences, operator_spec_for_operation,
    reject_unknown_fields, validate_field_value, validate_field_values,
    OPERATOR_PREFLIGHT_MAX_OPTIONS, OPERATOR_PREFLIGHT_MAX_QUESTIONS,
};
pub(crate) use validation::{
    apply_operator_execute_preflight_answers, apply_operator_preflight_answers,
    apply_operator_preflight_answers_for_spec, operator_operation_groups_for_spec,
    operator_operation_names, operator_operation_summaries_for_spec,
};
pub use validation::{operator_preflight_question, operator_preflight_question_for_spec};
#[cfg(test)]
pub(crate) use validation::{
    operator_tool_schema, OPERATOR_PARAM_SOURCE_DEFAULT, OPERATOR_PARAM_SOURCE_USER_PREFLIGHT,
    OPERATOR_PREFLIGHT_METADATA_KEY,
};
#[cfg(test)]
pub(crate) use validation::{
    preflight_answer_labels, preflight_question_should_ask, preflight_value_for_answer,
};
pub const OPERATOR_TOOL_PREFIX: &str = "operator__";
pub const OPERATOR_EXECUTE_TOOL_NAME: &str = "operator_execute";
const OPERATOR_STATE_DIR_NAME: &str = ".omiga";
const RUNS_RELATIVE_PATH: &str = "runs";
const OPERATOR_STRUCTURED_OUTPUTS_FILE: &str = "outputs.json";
const OPERATOR_STRUCTURED_OUTPUTS_MAX_BYTES: u64 = 1024 * 1024;

pub fn list_operator_manifest_diagnostics() -> Vec<OperatorManifestDiagnostic> {
    let outcome = crate::domain::plugins::plugin_load_outcome();
    operator_manifest_diagnostics_from_plugins(outcome.plugins())
}

pub fn list_operator_authoring_diagnostics() -> Vec<OperatorManifestDiagnostic> {
    let mut diagnostics = discover_operator_candidates()
        .iter()
        .flat_map(|spec| {
            operator_preflight_authoring_diagnostics(spec)
                .into_iter()
                .chain(operator_external_network_authoring_diagnostics(spec))
                .chain(operator_interface_authoring_diagnostics(spec))
        })
        .collect::<Vec<_>>();
    diagnostics.sort_by(|left, right| {
        left.source_plugin
            .cmp(&right.source_plugin)
            .then_with(|| left.manifest_path.cmp(&right.manifest_path))
            .then_with(|| left.message.cmp(&right.message))
    });
    diagnostics
}

fn operator_preflight_authoring_diagnostics(
    spec: &OperatorSpec,
) -> Vec<OperatorManifestDiagnostic> {
    let Some(preflight) = &spec.preflight else {
        return Vec::new();
    };
    if preflight.questions.is_empty() {
        return Vec::new();
    }
    let asks_method = preflight
        .questions
        .iter()
        .any(preflight_question_mentions_method_choice);
    let asks_threshold_or_filter = preflight
        .questions
        .iter()
        .any(preflight_question_mentions_threshold_or_filter);
    let only_data_or_grouping = preflight
        .questions
        .iter()
        .all(preflight_question_mentions_data_or_grouping);

    if only_data_or_grouping && !asks_method && !asks_threshold_or_filter {
        return vec![OperatorManifestDiagnostic {
            source_plugin: spec.source.source_plugin.clone(),
            manifest_path: spec.source.manifest_path.to_string_lossy().into_owned(),
            severity: "warning".to_string(),
            message: format!(
                "operator `{}` preflight only asks data/grouping questions; add method, threshold, or filtering choices when those decisions affect analysis semantics",
                spec.metadata.id
            ),
        }];
    }
    Vec::new()
}

fn operator_external_network_authoring_diagnostics(
    spec: &OperatorSpec,
) -> Vec<OperatorManifestDiagnostic> {
    if !operator_declares_external_network(spec) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    if !cache_config_enabled(spec.cache.as_ref()) {
        diagnostics.push(OperatorManifestDiagnostic {
            source_plugin: spec.source.source_plugin.clone(),
            manifest_path: spec.source.manifest_path.to_string_lossy().into_owned(),
            severity: "warning".to_string(),
            message: format!(
                "operator `{}` declares external_network permissions but has no enabled cache policy; add cache.enabled plus policy metadata for repeatable network runs",
                spec.metadata.id
            ),
        });
    }

    let mode_supports_offline_fixture = spec
        .interface
        .params
        .get("mode")
        .filter(|field| matches!(field.kind, OperatorFieldKind::Enum))
        .map(|field| {
            field.enum_values.iter().any(|value| {
                value
                    .as_str()
                    .map(|value| value == "offline_fixture")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    let fixture_param_exists = spec.interface.params.contains_key("fixture_json");
    if !mode_supports_offline_fixture || !fixture_param_exists {
        diagnostics.push(OperatorManifestDiagnostic {
            source_plugin: spec.source.source_plugin.clone(),
            manifest_path: spec.source.manifest_path.to_string_lossy().into_owned(),
            severity: "warning".to_string(),
            message: format!(
                "operator `{}` declares external_network permissions but does not expose both mode=offline_fixture and fixture_json params for deterministic offline validation",
                spec.metadata.id
            ),
        });
    }

    diagnostics
}

fn operator_interface_authoring_diagnostics(
    spec: &OperatorSpec,
) -> Vec<OperatorManifestDiagnostic> {
    let mut diagnostics = Vec::new();
    let preflight_param_ids: std::collections::HashSet<&str> = spec
        .preflight
        .as_ref()
        .map(|pf| pf.questions.iter().map(|q| q.param.as_str()).collect())
        .unwrap_or_default();
    for (param_name, param_spec) in &spec.interface.params {
        if param_spec.required && !preflight_param_ids.contains(param_name.as_str()) {
            diagnostics.push(OperatorManifestDiagnostic {
                source_plugin: spec.source.source_plugin.clone(),
                manifest_path: spec.source.manifest_path.to_string_lossy().into_owned(),
                severity: "warning".to_string(),
                message: format!(
                    "param `{param_name}` is required but has no preflight question; non-interactive callers must always supply it",
                ),
            });
        }
    }
    for (output_name, output_spec) in &spec.interface.outputs {
        let is_file_like = matches!(
            output_spec.kind,
            OperatorFieldKind::File | OperatorFieldKind::FileArray
        );
        if is_file_like
            && output_spec
                .glob
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
        {
            diagnostics.push(OperatorManifestDiagnostic {
                source_plugin: spec.source.source_plugin.clone(),
                manifest_path: spec.source.manifest_path.to_string_lossy().into_owned(),
                severity: "warning".to_string(),
                message: format!(
                    "output `{output_name}` is a file kind but has no glob pattern; the runtime cannot collect it",
                ),
            });
        }
    }
    diagnostics
}

fn operator_declares_external_network(spec: &OperatorSpec) -> bool {
    spec.metadata
        .tags
        .iter()
        .any(|tag| tag.eq_ignore_ascii_case("external-network"))
        || spec
            .permissions
            .as_ref()
            .and_then(|permissions| permissions.get("sideEffects"))
            .and_then(JsonValue::as_array)
            .map(|side_effects| {
                side_effects.iter().any(|value| {
                    value
                        .as_str()
                        .map(|value| value.eq_ignore_ascii_case("external_network"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
}

fn preflight_question_mentions_method_choice(question: &OperatorPreflightQuestionSpec) -> bool {
    preflight_question_text(question).contains_any(&[
        "method",
        "stat",
        "test",
        "model",
        "algorithm",
        "de_method",
        "方法",
        "统计",
        "检验",
        "模型",
    ])
}

fn preflight_question_mentions_threshold_or_filter(
    question: &OperatorPreflightQuestionSpec,
) -> bool {
    preflight_question_text(question).contains_any(&[
        "threshold",
        "cutoff",
        "filter",
        "pvalue",
        "p-value",
        "fdr",
        "padj",
        "log2fc",
        "fc",
        "min",
        "max",
        "top",
        "size",
        "阈值",
        "过滤",
        "筛选",
        "显著",
        "p值",
    ])
}

fn preflight_question_mentions_data_or_grouping(question: &OperatorPreflightQuestionSpec) -> bool {
    preflight_question_text(question).contains_any(&[
        "input",
        "data",
        "type",
        "sample",
        "group",
        "control",
        "case",
        "metadata",
        "column",
        "comparison",
        "delimiter",
        "row",
        "输入",
        "数据",
        "样本",
        "分组",
        "对照",
        "列",
        "比较",
    ])
}

fn preflight_question_text(question: &OperatorPreflightQuestionSpec) -> String {
    format!(
        "{} {} {} {}",
        question.id.as_deref().unwrap_or_default(),
        question.param,
        question.question,
        question.header
    )
    .to_ascii_lowercase()
}

trait ContainsAny {
    fn contains_any(&self, needles: &[&str]) -> bool;
}

impl ContainsAny for String {
    fn contains_any(&self, needles: &[&str]) -> bool {
        needles.iter().any(|needle| self.contains(needle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::environment_fallback::{
        classify_provisioning_failure, ProvisioningFailureKind,
    };
    use std::ffi::{OsStr, OsString};
    use std::sync::Mutex;
    use tempfile::TempDir;

    static OPERATOR_ENV_HYGIENE_LOCK: Mutex<()> = Mutex::new(());

    struct ScopedEnvVar {
        key: &'static str,
        old: Option<OsString>,
    }

    impl ScopedEnvVar {
        fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let old = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, old }
        }

        fn remove(key: &'static str) -> Self {
            let old = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, old }
        }
    }

    impl Drop for ScopedEnvVar {
        fn drop(&mut self) {
            match self.old.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    struct ScopedEnvKeep {
        old: Option<OsString>,
    }

    impl ScopedEnvKeep {
        fn unset() -> Self {
            let old = std::env::var_os("OMIGA_ENV_KEEP");
            std::env::remove_var("OMIGA_ENV_KEEP");
            Self { old }
        }
    }

    impl Drop for ScopedEnvKeep {
        fn drop(&mut self) {
            if let Some(old) = self.old.take() {
                std::env::set_var("OMIGA_ENV_KEEP", old);
            } else {
                std::env::remove_var("OMIGA_ENV_KEEP");
            }
        }
    }

    #[test]
    fn parses_sacct_diagnostics_for_common_outcomes() {
        let oom =
            parse_sacct_diagnostic_output("OUT_OF_MEMORY|0:9|204800K|00:10:00|OutOfMemory|4096M\n")
                .expect("OOM sacct row");
        assert_eq!(oom.state, "OUT_OF_MEMORY");
        assert_eq!(oom.exit_code, "0:9");
        assert_eq!(oom.max_rss_kb, Some(204800));
        assert_eq!(oom.category, SacctFailureCategory::Oom);
        assert_eq!(
            oom.suggested_action.as_deref(),
            Some("Re-run with --mem=300MB")
        );

        let timeout =
            parse_sacct_diagnostic_output("TIMEOUT|0:0|1024K|01:00:00|TIME_LIMIT|2048M\n")
                .expect("timeout sacct row");
        assert_eq!(timeout.category, SacctFailureCategory::Timeout);
        assert_eq!(
            timeout.suggested_action.as_deref(),
            Some("Re-run with --time=02:00:00")
        );

        let success = parse_sacct_diagnostic_output("COMPLETED|0:0|1024K|00:05:00||1024M\n")
            .expect("success sacct row");
        assert_eq!(success.category, SacctFailureCategory::Other);
        assert_eq!(success.suggested_action, None);
    }

    fn write_smoke_operator_plugin(tmp: &TempDir) -> PathBuf {
        let plugin_root = tmp.path().join("operator-smoke");
        fs::create_dir_all(plugin_root.join("operators/write-text-report")).unwrap();
        fs::create_dir_all(plugin_root.join("operators/container-text-report")).unwrap();
        fs::create_dir_all(plugin_root.join("scripts")).unwrap();
        fs::write(
            plugin_root.join("plugin.json"),
            r#"{"name":"operator-smoke","version":"0.1.0","operators":"./operators"}"#,
        )
        .unwrap();
        fs::write(
            plugin_root.join("operators/write-text-report/operator.yaml"),
            WRITE_TEXT_REPORT_OPERATOR,
        )
        .unwrap();
        fs::write(
            plugin_root.join("operators/container-text-report/operator.yaml"),
            CONTAINER_TEXT_REPORT_OPERATOR,
        )
        .unwrap();
        fs::write(
            plugin_root.join("scripts/write_text_report.sh"),
            WRITE_TEXT_REPORT_SCRIPT,
        )
        .unwrap();
        fs::write(
            plugin_root.join("scripts/write_container_report.sh"),
            WRITE_CONTAINER_REPORT_SCRIPT,
        )
        .unwrap();
        plugin_root
    }

    fn smoke_operator_paths(tmp: &TempDir) -> (PathBuf, PathBuf) {
        smoke_operator_manifest_path(tmp, "write-text-report")
    }

    fn container_operator_paths(tmp: &TempDir) -> (PathBuf, PathBuf) {
        smoke_operator_manifest_path(tmp, "container-text-report")
    }

    fn smoke_operator_manifest_path(tmp: &TempDir, operator_dir: &str) -> (PathBuf, PathBuf) {
        let plugin_root = write_smoke_operator_plugin(tmp);
        let manifest = plugin_root
            .join("operators")
            .join(operator_dir)
            .join("operator.yaml");
        (plugin_root, manifest)
    }

    fn curated_marketplace_root() -> PathBuf {
        let marketplace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .parent()
            .expect("workspace root")
            .join("omiga-plugins");
        marketplace_root
    }

    fn curated_marketplace_plugin_roots() -> Vec<(String, PathBuf)> {
        let marketplace_root = curated_marketplace_root();
        let raw = std::fs::read_to_string(marketplace_root.join("marketplace.json"))
            .expect("marketplace manifest");
        let manifest_json: serde_json::Value =
            serde_json::from_str(&raw).expect("marketplace json");
        manifest_json
            .get("plugins")
            .and_then(|plugins| plugins.as_array())
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let name = entry.get("name")?.as_str()?.to_string();
                let source_path = entry.get("source")?.get("path")?.as_str()?;
                Some((
                    name,
                    marketplace_root.join(source_path.trim_start_matches("./")),
                ))
            })
            .collect()
    }

    fn curated_operator_loaded_plugins() -> Vec<crate::domain::plugins::LoadedPlugin> {
        curated_marketplace_plugin_roots()
            .into_iter()
            .filter_map(|(plugin_name, plugin_root)| {
                let manifest = crate::domain::plugins::load_plugin_manifest(&plugin_root)?;
                manifest.operators.as_ref()?;
                Some(crate::domain::plugins::LoadedPlugin {
                    id: format!("{plugin_name}@omiga-curated"),
                    manifest_name: Some(plugin_name),
                    display_name: None,
                    description: None,
                    root: plugin_root,
                    enabled: true,
                    skill_roots: Vec::new(),
                    mcp_servers: HashMap::new(),
                    apps: Vec::new(),
                    retrieval: None,
                    error: None,
                })
            })
            .collect()
    }

    const WRITE_TEXT_REPORT_OPERATOR: &str = r#"apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: write_text_report
  version: 0.1.0
  name: Write Text Report
  description: Write a deterministic text artifact from a message parameter.
  tags:
    - smoke-test
    - text
interface:
  params:
    message:
      kind: string
      description: Text to write into the generated report artifact.
      default: hello from Omiga operator
    repeat:
      kind: integer
      description: Number of report lines to write.
      default: 1
      minimum: 1
      maximum: 20
  outputs:
    report:
      kind: file
      description: Generated text report.
      glob: operator-report.txt
      required: true
smokeTests:
  - id: default
    name: Write text report smoke
    description: Generates a deterministic two-line report artifact.
    params:
      message: hello operator smoke
      repeat: 2
    resources: {}
runtime:
  placement:
    supported:
      - local
      - ssh
  container:
    supported:
      - none
resources:
  cpu:
    default: 1
    exposed: true
  walltime:
    default: 60s
    exposed: true
execution:
  argv:
    - /bin/sh
    - ./scripts/write_text_report.sh
    - ${outdir}
    - ${params.message}
    - ${params.repeat}
"#;

    const CONTAINER_TEXT_REPORT_OPERATOR: &str = r#"apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: container_text_report
  version: 0.1.0
  name: Container Text Report
  description: Write a deterministic text artifact from inside Docker or Singularity.
  tags:
    - smoke-test
    - container
    - docker
    - singularity
interface:
  params:
    message:
      kind: string
      description: Text to write into the generated container report artifact.
      default: hello from Omiga container operator
    repeat:
      kind: integer
      description: Number of report lines to write before the runtime marker.
      default: 1
      minimum: 1
      maximum: 20
  outputs:
    report:
      kind: file
      description: Generated container text report.
      glob: container-operator-report.txt
      required: true
smokeTests:
  - id: default
    name: Active container smoke
    description: Runs the operator through the active Docker or Singularity backend and writes a two-line report plus a runtime marker.
    params:
      message: hello container operator smoke
      repeat: 2
    resources: {}
runtime:
  placement:
    supported:
      - local
      - ssh
  container:
    supported:
      - docker
      - singularity
    images:
      docker: alpine:3.19
      singularity: docker://alpine:3.19
  scheduler:
    supported:
      - none
resources:
  cpu:
    default: 1
    exposed: true
  walltime:
    default: 120s
    exposed: true
execution:
  argv:
    - /bin/sh
    - ./scripts/write_container_report.sh
    - ${outdir}
    - ${params.message}
    - ${params.repeat}
"#;

    const WRITE_TEXT_REPORT_SCRIPT: &str = r#"#!/bin/sh
set -eu

outdir="${1:?missing outdir}"
message="${2:-hello from Omiga operator}"
repeat="${3:-1}"

case "$repeat" in
  ''|*[!0-9]*) repeat=1 ;;
esac
if [ "$repeat" -lt 1 ]; then repeat=1; fi
if [ "$repeat" -gt 20 ]; then repeat=20; fi

mkdir -p "$outdir"
: > "$outdir/operator-report.txt"
i=0
while [ "$i" -lt "$repeat" ]; do
  printf '%s\n' "$message" >> "$outdir/operator-report.txt"
  i=$((i + 1))
done

printf 'wrote %s line(s) to %s\n' "$repeat" "$outdir/operator-report.txt"
"#;

    const WRITE_CONTAINER_REPORT_SCRIPT: &str = r#"#!/bin/sh
set -eu

outdir="${1:?missing outdir}"
message="${2:-hello from Omiga container operator}"
repeat="${3:-1}"

case "$repeat" in
  ''|*[!0-9]*) repeat=1 ;;
esac
if [ "$repeat" -lt 1 ]; then repeat=1; fi
if [ "$repeat" -gt 20 ]; then repeat=20; fi

mkdir -p "$outdir"
report="$outdir/container-operator-report.txt"
: > "$report"
i=0
while [ "$i" -lt "$repeat" ]; do
  printf '%s\n' "$message" >> "$report"
  i=$((i + 1))
done

printf 'container smoke runtime: %s\n' "$(uname -s 2>/dev/null || printf unknown)" >> "$report"
printf 'wrote %s line(s) plus runtime marker to %s\n' "$repeat" "$report"
"#;

    fn cached_report_operator_spec(
        tmp: &TempDir,
        marker_path: &Path,
        cache: Option<JsonValue>,
    ) -> OperatorSpec {
        OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "cached_report".to_string(),
                version: "1".to_string(),
                name: None,
                description: Some("cacheable local report".to_string()),
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec {
                inputs: BTreeMap::from([(
                    "input".to_string(),
                    OperatorFieldSpec {
                        kind: OperatorFieldKind::File,
                        required: true,
                        ..OperatorFieldSpec::default()
                    },
                )]),
                outputs: BTreeMap::from([(
                    "report".to_string(),
                    OperatorFieldSpec {
                        kind: OperatorFieldKind::File,
                        required: true,
                        glob: Some("report.txt".to_string()),
                        ..OperatorFieldSpec::default()
                    },
                )]),
                ..OperatorInterfaceSpec::default()
            },
            operations: BTreeMap::new(),
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "printf 'run\\n' >> \"$1\"; cat \"$2\" > \"$3/report.txt\"".to_string(),
                    "cached_report".to_string(),
                    marker_path.to_string_lossy().into_owned(),
                    "${inputs.input}".to_string(),
                    "${outdir}".to_string(),
                ],
            },
            preflight: None,
            runtime: None,
            cache,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "test@local".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        }
    }

    fn simple_operator_spec(
        tmp: &TempDir,
        id: &str,
        version: &str,
        source_plugin: &str,
    ) -> OperatorSpec {
        OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: id.to_string(),
                version: version.to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec::default(),
            operations: BTreeMap::new(),
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["true".to_string()],
            },
            preflight: None,
            runtime: None,
            cache: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: source_plugin.to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        }
    }

    fn argv_operator_spec(tmp: &TempDir, argv: &[&str]) -> OperatorSpec {
        OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "x".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec::default(),
            operations: BTreeMap::new(),
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: argv.iter().map(|value| value.to_string()).collect(),
            },
            preflight: None,
            runtime: None,
            cache: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "p".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        }
    }

    fn input_file_invocation(input: &str) -> OperatorInvocation {
        OperatorInvocation {
            operation: None,
            inputs: BTreeMap::from([("input".to_string(), JsonValue::String(input.to_string()))]),
            params: BTreeMap::new(),
            resources: BTreeMap::new(),
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn operator_tool_schema_surfaces_resource_profile_warning() {
        let tmp = TempDir::new().unwrap();
        let mut spec = argv_operator_spec(&tmp, &["/bin/echo", "ok"]);
        spec.metadata.description = Some("Align reads".to_string());
        spec.runtime = Some(json!({
            "resourceProfile": {
                "tier": "hpc-recommended",
                "localPolicy": "warn",
                "recommendedCpu": 32,
                "recommendedMemoryGb": 128,
                "diskGb": 200,
                "notes": ["Use SSH/server/HPC for production RNA-seq runs."]
            }
        }));

        let schema = operator_tool_schema(ResolvedOperator {
            alias: "align".to_string(),
            spec,
        });

        assert!(schema.description.contains("Resource note"));
        assert!(schema.description.contains("HPC/server recommended"));
        assert!(schema.description.contains("32 CPU recommended"));
        assert!(schema.description.contains("128 GB RAM recommended"));
    }

    #[tokio::test]
    async fn successful_operator_tool_call_writes_execution_record() {
        let tmp = TempDir::new().unwrap();
        let spec = argv_operator_spec(
            &tmp,
            &[
                "/bin/sh",
                "-c",
                "printf 'execution-record-success\\n' >/dev/null",
            ],
        );
        let ctx = crate::domain::tools::ToolContext::new(tmp.path())
            .with_session_id(Some("session-op".to_string()));
        let arguments =
            serde_json::to_string(&OperatorInvocation::default()).expect("serialize invocation");

        let (raw, is_error) = execute_resolved_operator_tool_call_with_context(
            &ctx,
            "x",
            ResolvedOperator {
                alias: "x".to_string(),
                spec,
            },
            &arguments,
            None,
        )
        .await;

        assert!(!is_error, "{raw}");
        let rows = crate::domain::execution_records::list_recent_execution_records(tmp.path(), 10)
            .await
            .expect("records");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "operator");
        assert_eq!(rows[0].unit_id.as_deref(), Some("x"));
        assert_eq!(rows[0].canonical_id.as_deref(), Some("p/operator/x"));
        assert_eq!(rows[0].provider_plugin.as_deref(), Some("p"));
        assert_eq!(rows[0].status, "succeeded");
        assert_eq!(rows[0].session_id.as_deref(), Some("session-op"));
    }

    #[tokio::test]
    async fn operator_result_and_record_include_param_sources() {
        let tmp = TempDir::new().unwrap();
        let mut spec = argv_operator_spec(&tmp, &["/bin/sh", "-c", "true"]);
        spec.interface.params.insert(
            "method".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::Enum,
                enum_values: vec![json!("auto"), json!("manual")],
                ..OperatorFieldSpec::default()
            },
        );
        spec.interface.params.insert(
            "alpha".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::Number,
                default: Some(json!(0.05)),
                ..OperatorFieldSpec::default()
            },
        );
        let invocation = OperatorInvocation {
            params: BTreeMap::from([("method".to_string(), json!("manual"))]),
            metadata: BTreeMap::from([(
                OPERATOR_PREFLIGHT_METADATA_KEY.to_string(),
                json!({
                    "source": "operator_preflight",
                    "operatorId": "x",
                    "answeredParams": [{"param": "method"}],
                    "paramsBySource": {"method": OPERATOR_PARAM_SOURCE_USER_PREFLIGHT},
                }),
            )]),
            ..OperatorInvocation::default()
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let arguments = serde_json::to_string(&invocation).expect("serialize invocation");

        let (raw, is_error) = execute_resolved_operator_tool_call_with_context(
            &ctx,
            "x",
            ResolvedOperator {
                alias: "x".to_string(),
                spec,
            },
            &arguments,
            None,
        )
        .await;

        assert!(!is_error, "{raw}");
        let parsed = serde_json::from_str::<JsonValue>(&raw).unwrap();
        assert_eq!(
            parsed["paramSources"]["method"],
            OPERATOR_PARAM_SOURCE_USER_PREFLIGHT
        );
        assert_eq!(
            parsed["paramSources"]["alpha"],
            OPERATOR_PARAM_SOURCE_DEFAULT
        );
        assert_eq!(
            parsed["preflight"]["paramsBySource"]["method"],
            OPERATOR_PARAM_SOURCE_USER_PREFLIGHT
        );

        let rows = crate::domain::execution_records::list_recent_execution_records(tmp.path(), 10)
            .await
            .expect("records");
        let metadata = rows[0].metadata_json.as_deref().unwrap_or_default();
        assert!(metadata.contains("\"paramSources\""));
        assert!(metadata.contains(OPERATOR_PARAM_SOURCE_USER_PREFLIGHT));
    }

    #[tokio::test]
    async fn failed_operator_tool_call_writes_execution_record() {
        let tmp = TempDir::new().unwrap();
        let spec = argv_operator_spec(
            &tmp,
            &["/bin/sh", "-c", "echo execution-record-failure >&2; exit 7"],
        );
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let arguments =
            serde_json::to_string(&OperatorInvocation::default()).expect("serialize invocation");

        let (raw, is_error) = execute_resolved_operator_tool_call_with_context(
            &ctx,
            "x",
            ResolvedOperator {
                alias: "x".to_string(),
                spec,
            },
            &arguments,
            None,
        )
        .await;

        assert!(is_error, "{raw}");
        let rows = crate::domain::execution_records::list_recent_execution_records(tmp.path(), 10)
            .await
            .expect("records");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "operator");
        assert_eq!(rows[0].unit_id.as_deref(), Some("x"));
        assert_eq!(rows[0].status, "failed");
        assert!(rows[0]
            .metadata_json
            .as_deref()
            .unwrap_or_default()
            .contains("tool_exit_nonzero"));
    }

    #[tokio::test]
    async fn operator_success_does_not_fail_when_execution_record_write_fails() {
        let tmp = TempDir::new().unwrap();
        let state_dir = tmp.path().join(".omiga");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join("execution"), "not a directory").unwrap();
        let spec = argv_operator_spec(
            &tmp,
            &[
                "/bin/sh",
                "-c",
                "printf 'record-write-blocked\\n' >/dev/null",
            ],
        );
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let arguments =
            serde_json::to_string(&OperatorInvocation::default()).expect("serialize invocation");

        let (raw, is_error) = execute_resolved_operator_tool_call_with_context(
            &ctx,
            "x",
            ResolvedOperator {
                alias: "x".to_string(),
                spec,
            },
            &arguments,
            None,
        )
        .await;

        assert!(!is_error, "{raw}");
        let parsed: JsonValue = serde_json::from_str(&raw).expect("operator result json");
        assert_eq!(parsed["status"], "succeeded");
        assert!(state_dir.join("execution").is_file());
    }

    #[test]
    fn parses_manifest_and_generates_tool_schema() {
        let tmp = TempDir::new().unwrap();
        let manifest = tmp.path().join("operator.yaml");
        fs::write(
            &manifest,
            r#"
apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: fastqc
  version: 0.12.1
  description: FASTQ quality control
interface:
  inputs:
    reads:
      kind: file_array
      required: true
      formats: [fastq.gz]
  params:
    threads:
      kind: integer
      default: 4
  outputs:
    reports:
      kind: file_array
      glob: "*.html"
      required: true
execution:
  argv: ["fastqc", "--threads", "${params.threads}", "${inputs.reads}"]
cache:
  enabled: true
resources:
  cpu:
    default: 4
    exposed: true
bindings:
  - param: threads
    resource: cpu
"#,
        )
        .unwrap();
        let spec = load_operator_manifest(&manifest, "p@m", tmp.path()).unwrap();
        assert_eq!(spec.metadata.id, "fastqc");
        assert_eq!(spec.cache, Some(json!({"enabled": true})));
        let schema = operator_parameters_schema(&spec);
        assert_eq!(schema["required"][0], "inputs");
        assert!(schema["properties"]["inputs"]["properties"]["reads"]["items"].is_object());
        assert_eq!(
            schema["properties"]["resources"]["properties"]["cpu"]["type"],
            "integer"
        );
    }

    #[test]
    fn discovers_operators_from_manifest_declared_path() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("plugin.json"),
            r#"{"name":"custom-operator-plugin","operators":"./custom-units"}"#,
        )
        .unwrap();
        let manifest = tmp
            .path()
            .join("custom-units")
            .join("custom")
            .join("operator.yaml");
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(
            &manifest,
            r#"
apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: custom_manifest_path
  version: 1
execution:
  argv: ["true"]
"#,
        )
        .unwrap();
        let plugin = crate::domain::plugins::LoadedPlugin {
            id: "custom-operator-plugin@local".to_string(),
            manifest_name: Some("custom-operator-plugin".to_string()),
            display_name: None,
            description: None,
            root: tmp.path().to_path_buf(),
            enabled: true,
            skill_roots: Vec::new(),
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            retrieval: None,
            error: None,
        };

        let candidates = discover_operator_candidates_from_plugins([&plugin]);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].metadata.id, "custom_manifest_path");
    }

    #[test]
    fn discovers_temp_smoke_operator_from_active_plugin() {
        let tmp = TempDir::new().unwrap();
        let (plugin_root, manifest) = smoke_operator_paths(&tmp);
        assert!(manifest.is_file());

        let plugin = crate::domain::plugins::LoadedPlugin {
            id: "operator-smoke@omiga-curated".to_string(),
            manifest_name: Some("operator-smoke".to_string()),
            display_name: Some("Smoke Test".to_string()),
            description: None,
            root: plugin_root,
            enabled: true,
            skill_roots: Vec::new(),
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            retrieval: None,
            error: None,
        };

        let candidates = discover_operator_candidates_from_plugins([&plugin]);
        let smoke = candidates
            .iter()
            .find(|candidate| candidate.metadata.id == "write_text_report")
            .expect("temp smoke operator should be discovered");
        assert_eq!(smoke.source.source_plugin, "operator-smoke@omiga-curated");
        assert_eq!(smoke.metadata.version, "0.1.0");
        assert_eq!(smoke.execution.argv[0], "/bin/sh");
        assert_eq!(smoke.execution.argv[1], "./scripts/write_text_report.sh");
        assert_eq!(smoke.smoke_tests.len(), 1);
        assert_eq!(smoke.smoke_tests[0].id, "default");
        assert_eq!(
            smoke.smoke_tests[0].arguments.params["message"],
            "hello operator smoke"
        );

        let container = candidates
            .iter()
            .find(|candidate| candidate.metadata.id == "container_text_report")
            .expect("temp container smoke operator should be discovered");
        assert_eq!(
            container.source.source_plugin,
            "operator-smoke@omiga-curated"
        );
        assert_eq!(container.metadata.version, "0.1.0");
        assert_eq!(container.execution.argv[0], "/bin/sh");
        assert_eq!(
            container.execution.argv[1],
            "./scripts/write_container_report.sh"
        );
        assert_eq!(container.smoke_tests.len(), 1);
        assert_eq!(container.smoke_tests[0].id, "default");

        let schema = operator_parameters_schema(smoke);
        assert_eq!(
            schema["properties"]["params"]["properties"]["message"]["type"],
            "string"
        );
        let argv = expand_argv(
            smoke,
            &BTreeMap::new(),
            &BTreeMap::from([
                ("message".to_string(), json!("hello")),
                ("repeat".to_string(), json!(1)),
            ]),
            &BTreeMap::new(),
            "/tmp/run",
        )
        .unwrap();
        assert!(Path::new(&argv[1]).is_absolute());
        assert!(argv[1].ends_with("scripts/write_text_report.sh"));
    }

    #[test]
    fn discovers_curated_operator_plugins_without_core_plugin_name_assumptions() {
        let plugins = curated_operator_loaded_plugins();
        assert!(
            !plugins.is_empty(),
            "external marketplace should provide operator plugins"
        );
        let plugin_refs = plugins.iter().collect::<Vec<_>>();
        let candidates = discover_operator_candidates_from_plugins(plugin_refs);
        assert!(
            !candidates.is_empty(),
            "operator plugins should expose operator candidates"
        );

        let mut aliases = HashSet::new();
        let mut operation_count = 0usize;
        for candidate in &candidates {
            assert!(
                aliases.insert(candidate.metadata.id.clone()),
                "duplicate marketplace operator id `{}`",
                candidate.metadata.id
            );
            assert!(
                candidate.source.source_plugin.ends_with("@omiga-curated"),
                "candidate source should stay scoped to its plugin: {}",
                candidate.source.source_plugin
            );
            assert!(
                !candidate.operations.is_empty(),
                "operator `{}` should expose at least one operation",
                candidate.metadata.id
            );
            operation_count += candidate.operations.len();
        }
        assert!(operation_count >= candidates.len());
    }

    #[test]
    fn marketplace_plugins_without_operator_roots_do_not_expose_operator_units() {
        let plugin = curated_marketplace_plugin_roots()
            .into_iter()
            .find_map(|(plugin_name, plugin_root)| {
                let manifest = crate::domain::plugins::load_plugin_manifest(&plugin_root)?;
                if manifest.operators.is_some() {
                    return None;
                }
                Some(crate::domain::plugins::LoadedPlugin {
                    id: format!("{plugin_name}@omiga-curated"),
                    manifest_name: Some(plugin_name),
                    display_name: None,
                    description: None,
                    root: plugin_root,
                    enabled: true,
                    skill_roots: Vec::new(),
                    mcp_servers: HashMap::new(),
                    apps: Vec::new(),
                    retrieval: None,
                    error: None,
                })
            })
            .expect("marketplace plugin without operators");

        let candidates = discover_operator_candidates_from_plugins([&plugin]);
        assert!(candidates.is_empty());
    }

    #[test]
    fn preflight_rules_are_manifest_driven() {
        let question = OperatorPreflightQuestionSpec {
            id: Some("method".to_string()),
            param: "method".to_string(),
            question: "Pick method?".to_string(),
            header: "Method".to_string(),
            multi_select: false,
            ask_when: OperatorPreflightAskWhen {
                always: false,
                missing: true,
                empty: true,
                values: vec![json!("auto")],
            },
            show_when: None,
            options: vec![
                OperatorPreflightOptionSpec {
                    label: "Auto".to_string(),
                    description: "Let the operator choose.".to_string(),
                    value: json!("auto"),
                    preview: None,
                    custom: false,
                    custom_placeholder: None,
                },
                OperatorPreflightOptionSpec {
                    label: "Manual".to_string(),
                    description: "Use a fixed method.".to_string(),
                    value: json!("manual"),
                    preview: None,
                    custom: false,
                    custom_placeholder: None,
                },
            ],
        };

        assert!(preflight_question_should_ask(&question, None));
        let mut auto_params = JsonMap::new();
        auto_params.insert("method".to_string(), JsonValue::String("AUTO".to_string()));
        assert!(preflight_question_should_ask(&question, Some(&auto_params)));

        let mut manual_params = JsonMap::new();
        manual_params.insert(
            "method".to_string(),
            JsonValue::String("manual".to_string()),
        );
        assert!(!preflight_question_should_ask(
            &question,
            Some(&manual_params)
        ));

        let mut ask_params = JsonMap::new();
        ask_params.insert("method".to_string(), JsonValue::String("ASK".to_string()));
        assert!(preflight_question_should_ask(&question, Some(&ask_params)));

        let mut ask_object_params = JsonMap::new();
        ask_object_params.insert("method".to_string(), json!({"state": "ask"}));
        assert!(preflight_question_should_ask(
            &question,
            Some(&ask_object_params)
        ));

        assert_eq!(
            preflight_answer_labels(&json!("Auto, Manual"), true),
            vec!["Auto".to_string(), "Manual".to_string()]
        );

        let always_question = OperatorPreflightQuestionSpec {
            ask_when: OperatorPreflightAskWhen {
                always: true,
                missing: false,
                empty: false,
                values: Vec::new(),
            },
            ..question
        };
        assert!(preflight_question_should_ask(
            &always_question,
            Some(&manual_params)
        ));
    }

    #[test]
    fn preflight_answers_record_param_source_metadata() {
        let tmp = TempDir::new().unwrap();
        let mut spec = argv_operator_spec(&tmp, &["true"]);
        spec.metadata.id = "preflight_metadata".to_string();
        spec.interface.params.insert(
            "method".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::Enum,
                enum_values: vec![json!("auto"), json!("manual")],
                ..OperatorFieldSpec::default()
            },
        );
        spec.preflight = Some(OperatorPreflightSpec {
            questions: vec![OperatorPreflightQuestionSpec {
                id: Some("method".to_string()),
                param: "method".to_string(),
                question: "Pick method?".to_string(),
                header: "Method".to_string(),
                multi_select: false,
                ask_when: OperatorPreflightAskWhen {
                    always: true,
                    missing: false,
                    empty: false,
                    values: Vec::new(),
                },
                show_when: None,
                options: vec![
                    OperatorPreflightOptionSpec {
                        label: "Auto".to_string(),
                        description: "Auto method".to_string(),
                        value: json!("auto"),
                        preview: None,
                        custom: false,
                        custom_placeholder: None,
                    },
                    OperatorPreflightOptionSpec {
                        label: "Manual".to_string(),
                        description: "Manual method".to_string(),
                        value: json!("manual"),
                        preview: None,
                        custom: false,
                        custom_placeholder: None,
                    },
                ],
            }],
        });
        let updated = apply_operator_preflight_answers_for_spec(
            &spec,
            spec.preflight.as_ref().unwrap(),
            &json!({"params": {"method": "ask"}}).to_string(),
            &json!({"answers": {"Pick method?": "Manual"}}),
        )
        .expect("apply preflight");
        let parsed = serde_json::from_str::<JsonValue>(&updated).unwrap();

        assert_eq!(parsed["params"]["method"], "manual");
        assert_eq!(
            parsed["metadata"]["preflight"]["paramsBySource"]["method"],
            OPERATOR_PARAM_SOURCE_USER_PREFLIGHT
        );
        assert_eq!(
            parsed["metadata"]["preflight"]["answeredParams"][0]["param"],
            "method"
        );
    }

    #[test]
    fn preflight_project_preferences_recommend_without_overriding_explicit_params() {
        let tmp = TempDir::new().unwrap();
        let mut spec = argv_operator_spec(&tmp, &["true"]);
        spec.metadata.id = "preflight_recommendation".to_string();
        spec.interface.params.insert(
            "method".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::Enum,
                enum_values: vec![json!("auto"), json!("manual")],
                ..OperatorFieldSpec::default()
            },
        );
        spec.preflight = Some(OperatorPreflightSpec {
            questions: vec![OperatorPreflightQuestionSpec {
                id: Some("method".to_string()),
                param: "method".to_string(),
                question: "Pick method?".to_string(),
                header: "Method".to_string(),
                multi_select: false,
                ask_when: OperatorPreflightAskWhen {
                    always: false,
                    missing: true,
                    empty: true,
                    values: Vec::new(),
                },
                show_when: None,
                options: vec![
                    OperatorPreflightOptionSpec {
                        label: "Auto".to_string(),
                        description: "Auto method".to_string(),
                        value: json!("auto"),
                        preview: None,
                        custom: false,
                        custom_placeholder: None,
                    },
                    OperatorPreflightOptionSpec {
                        label: "Manual".to_string(),
                        description: "Manual method".to_string(),
                        value: json!("manual"),
                        preview: None,
                        custom: false,
                        custom_placeholder: None,
                    },
                ],
            }],
        });
        let recommended_params = BTreeMap::from([("method".to_string(), json!("manual"))]);

        let missing_params = operator_preflight_question_for_spec_with_recommended_params(
            &spec,
            Some("recommend"),
            None,
            Some(&recommended_params),
        )
        .expect("missing params should ask");
        assert_eq!(missing_params.questions[0].options[0].label, "Manual");
        assert!(missing_params.questions[0].options[0].recommended);
        assert!(missing_params.questions[0].options[0]
            .description
            .contains("推荐"));

        let mut explicit_params = JsonMap::new();
        explicit_params.insert("method".to_string(), json!("auto"));
        assert!(
            operator_preflight_question_for_spec_with_recommended_params(
                &spec,
                Some("recommend"),
                Some(&explicit_params),
                Some(&recommended_params),
            )
            .is_none(),
            "project preferences must not override explicit caller params"
        );

        let mut ask_params = JsonMap::new();
        ask_params.insert("method".to_string(), json!({"state": "ask"}));
        let ask_question = operator_preflight_question_for_spec_with_recommended_params(
            &spec,
            Some("recommend"),
            Some(&ask_params),
            Some(&recommended_params),
        )
        .expect("explicit ask state should still ask with recommendation");
        assert_eq!(ask_question.questions[0].options[0].label, "Manual");
    }

    #[test]
    fn preflight_authoring_diagnostics_flag_data_only_questions() {
        let tmp = TempDir::new().unwrap();
        let mut spec = argv_operator_spec(&tmp, &["true"]);
        spec.metadata.id = "data_only_preflight".to_string();
        spec.interface.params.insert(
            "group_column".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::String,
                ..OperatorFieldSpec::default()
            },
        );
        spec.interface.params.insert(
            "sample_column".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::String,
                ..OperatorFieldSpec::default()
            },
        );
        spec.preflight = Some(OperatorPreflightSpec {
            questions: vec![
                test_preflight_question("group", "group_column", "Which group column?"),
                test_preflight_question("sample", "sample_column", "Which sample column?"),
            ],
        });

        let diagnostics = operator_preflight_authoring_diagnostics(&spec);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, "warning");
        assert!(diagnostics[0].message.contains("data/grouping"));

        spec.interface.params.insert(
            "de_method".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::Enum,
                enum_values: vec![json!("auto"), json!("deseq2")],
                ..OperatorFieldSpec::default()
            },
        );
        spec.preflight
            .as_mut()
            .unwrap()
            .questions
            .push(test_preflight_question(
                "method",
                "de_method",
                "Which analysis method?",
            ));
        assert!(operator_preflight_authoring_diagnostics(&spec).is_empty());
    }

    #[test]
    fn external_network_authoring_diagnostics_require_cache_and_fixture_mode() {
        let tmp = TempDir::new().unwrap();
        let mut spec = argv_operator_spec(&tmp, &["true"]);
        spec.metadata.id = "network_operator".to_string();
        spec.metadata.tags.push("external-network".to_string());
        spec.permissions = Some(json!({
            "sideEffects": ["external_network"],
            "network": {"hosts": ["example.test"], "mode": "read_only"}
        }));

        let diagnostics = operator_external_network_authoring_diagnostics(&spec);

        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("cache.enabled")));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("offline_fixture")));

        spec.cache = Some(json!({
            "enabled": true,
            "policyVersion": "external-network/v1"
        }));
        spec.interface.params.insert(
            "mode".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::Enum,
                enum_values: vec![json!("auto"), json!("live"), json!("offline_fixture")],
                default: Some(json!("auto")),
                ..OperatorFieldSpec::default()
            },
        );
        spec.interface.params.insert(
            "fixture_json".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::String,
                default: Some(json!("")),
                ..OperatorFieldSpec::default()
            },
        );

        assert!(operator_external_network_authoring_diagnostics(&spec).is_empty());
    }

    fn test_preflight_question(
        id: &str,
        param: &str,
        question: &str,
    ) -> OperatorPreflightQuestionSpec {
        OperatorPreflightQuestionSpec {
            id: Some(id.to_string()),
            param: param.to_string(),
            question: question.to_string(),
            header: id.to_string(),
            multi_select: false,
            ask_when: OperatorPreflightAskWhen {
                always: true,
                missing: false,
                empty: false,
                values: Vec::new(),
            },
            show_when: None,
            options: vec![
                OperatorPreflightOptionSpec {
                    label: "A".to_string(),
                    description: "First option".to_string(),
                    value: json!("a"),
                    preview: None,
                    custom: false,
                    custom_placeholder: None,
                },
                OperatorPreflightOptionSpec {
                    label: "B".to_string(),
                    description: "Second option".to_string(),
                    value: json!("b"),
                    preview: None,
                    custom: false,
                    custom_placeholder: None,
                },
            ],
        }
    }

    #[test]
    fn preflight_custom_answers_parse_against_param_type() {
        let field = OperatorFieldSpec {
            kind: OperatorFieldKind::Number,
            minimum: Some(0.0),
            maximum: Some(1.0),
            ..OperatorFieldSpec::default()
        };
        let question = OperatorPreflightQuestionSpec {
            id: Some("fdr".to_string()),
            param: "pvalue_threshold".to_string(),
            question: "Pick FDR?".to_string(),
            header: "FDR".to_string(),
            multi_select: false,
            ask_when: OperatorPreflightAskWhen {
                always: true,
                missing: false,
                empty: false,
                values: Vec::new(),
            },
            show_when: None,
            options: vec![
                OperatorPreflightOptionSpec {
                    label: "FDR 0.05".to_string(),
                    description: "Default".to_string(),
                    value: json!(0.05),
                    preview: None,
                    custom: false,
                    custom_placeholder: None,
                },
                OperatorPreflightOptionSpec {
                    label: "自定义".to_string(),
                    description: "Typed value".to_string(),
                    value: json!(0.05),
                    preview: None,
                    custom: true,
                    custom_placeholder: Some("0.05".to_string()),
                },
            ],
        };

        assert_eq!(
            preflight_value_for_answer(&question, &field, "自定义：0.2").unwrap(),
            json!(0.2)
        );
        assert!(preflight_value_for_answer(&question, &field, "自定义：2").is_err());
    }

    #[test]
    fn temp_container_smoke_operator_builds_docker_command() {
        let tmp = TempDir::new().unwrap();
        let (plugin_root, manifest) = container_operator_paths(&tmp);
        let spec =
            load_operator_manifest(&manifest, "operator-smoke@omiga-curated", plugin_root).unwrap();
        assert_eq!(spec.metadata.id, "container_text_report");
        assert_eq!(spec.smoke_tests.len(), 1);
        assert_eq!(spec.smoke_tests[0].id, "default");

        let plain_ctx = crate::domain::tools::ToolContext::new(tmp.path());
        assert!(!runtime_supported(&plain_ctx, &spec));
        let docker_ctx =
            crate::domain::tools::ToolContext::new(tmp.path()).with_sandbox_backend("docker");
        assert!(runtime_supported(&docker_ctx, &spec));
        let singularity_ctx =
            crate::domain::tools::ToolContext::new(tmp.path()).with_sandbox_backend("singularity");
        assert!(runtime_supported(&singularity_ctx, &spec));

        let smoke = &spec.smoke_tests[0].arguments;
        let run_dir = "/tmp/oprun_container_smoke";
        let argv = expand_argv(
            &spec,
            &BTreeMap::new(),
            &smoke.params,
            &smoke.resources,
            run_dir,
        )
        .unwrap();
        assert!(Path::new(&argv[1]).is_absolute());
        assert!(argv[1].ends_with("scripts/write_container_report.sh"));

        let command = operator_execution_command(
            &docker_ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            run_dir,
            &argv,
            &BTreeMap::new(),
        );
        assert!(command.contains("'docker' 'run' '--rm'"));
        assert!(command.contains("'alpine:3.19'"));
        assert!(command.contains("'hello container operator smoke'"));
        assert!(command.contains("write_container_report.sh"));
        assert!(command.contains(&format!(
            "'{}:{}:ro'",
            spec.source.plugin_root.to_string_lossy(),
            spec.source.plugin_root.to_string_lossy()
        )));
        assert!(command.contains(&format!("'{run_dir}:{run_dir}'")));
    }

    #[tokio::test]
    #[ignore = "requires a running Docker daemon and access to the alpine:3.19 image"]
    async fn executes_temp_container_smoke_operator_with_docker_runtime() {
        let tmp = TempDir::new().unwrap();
        let (plugin_root, manifest) = container_operator_paths(&tmp);
        let spec =
            load_operator_manifest(&manifest, "operator-smoke@omiga-curated", plugin_root).unwrap();
        let smoke_invocation = spec
            .smoke_tests
            .iter()
            .find(|test| test.id == "default")
            .expect("container smoke test")
            .arguments
            .clone();
        let ctx = crate::domain::tools::ToolContext::new(tmp.path()).with_sandbox_backend("docker");

        let result = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "container_text_report".to_string(),
                spec,
            },
            smoke_invocation,
            Some(OperatorRunContext {
                kind: Some("smoke".to_string()),
                smoke_test_id: Some("default".to_string()),
                smoke_test_name: Some("Active container smoke".to_string()),
                parent_execution_id: None,
                bypass_cache: false,
            }),
        )
        .await
        .unwrap();

        assert_eq!(result.status, "succeeded");
        assert_eq!(result.location, "local");
        assert_eq!(result.outputs["report"].len(), 1);
        assert_eq!(result.enforcement["container"], "docker");
        let report = fs::read_to_string(&result.outputs["report"][0].path).unwrap();
        assert!(report.contains("hello container operator smoke"));
        assert!(report.contains("container smoke runtime:"));
    }

    #[test]
    fn rejects_smoke_test_ids_with_invalid_characters() {
        let tmp = TempDir::new().unwrap();
        let manifest = tmp.path().join("operator.yaml");
        fs::write(
            &manifest,
            r#"
apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: bad_smoke
  version: 1
smokeTests:
  - id: bad/id
    params: {}
execution:
  argv: ["true"]
"#,
        )
        .unwrap();

        let error = load_operator_manifest(&manifest, "p@m", tmp.path()).unwrap_err();
        assert!(error.contains("operator smoke test id `bad/id`"));
    }

    #[test]
    fn reports_invalid_operator_manifest_diagnostics() {
        let tmp = TempDir::new().unwrap();
        let manifest = tmp
            .path()
            .join("operators")
            .join("bad")
            .join("operator.yaml");
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(
            &manifest,
            r#"
apiVersion: wrong/v1
kind: Operator
metadata:
  id: bad
  version: 1
execution:
  argv: ["true"]
"#,
        )
        .unwrap();
        let plugin = crate::domain::plugins::LoadedPlugin {
            id: "bad-operator@local".to_string(),
            manifest_name: Some("bad-operator".to_string()),
            display_name: Some("Bad Operator".to_string()),
            description: None,
            root: tmp.path().to_path_buf(),
            enabled: true,
            skill_roots: Vec::new(),
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            retrieval: None,
            error: None,
        };

        let diagnostics = operator_manifest_diagnostics_from_plugins([&plugin]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].source_plugin, "bad-operator@local");
        assert_eq!(diagnostics[0].severity, "error");
        assert!(diagnostics[0]
            .message
            .contains("unsupported operator apiVersion"));
    }

    #[test]
    fn active_plugin_operator_auto_exposes_with_default_alias() {
        let tmp = TempDir::new().unwrap();
        let spec = simple_operator_spec(&tmp, "fastqc", "1", "bio@builtin");

        let resolved = resolve_enabled_operators_from(vec![spec], OperatorRegistryFile::default());

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].alias, "fastqc");
        assert_eq!(resolved[0].spec.source.source_plugin, "bio@builtin");
    }

    #[test]
    fn disabled_registry_entry_does_not_suppress_plugin_auto_exposure() {
        let tmp = TempDir::new().unwrap();
        let spec = simple_operator_spec(&tmp, "fastqc", "1", "bio@builtin");
        let registry = OperatorRegistryFile {
            enabled: BTreeMap::from([(
                "fastqc".to_string(),
                OperatorRegistryEntry::Full {
                    operator_id: Some("fastqc".to_string()),
                    source_plugin: Some("bio@builtin".to_string()),
                    version: Some("1".to_string()),
                    enabled: Some(false),
                },
            )]),
        };

        let resolved = resolve_enabled_operators_from(vec![spec], registry);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].alias, "fastqc");
    }

    #[test]
    fn explicit_registry_alias_does_not_duplicate_same_operator() {
        let tmp = TempDir::new().unwrap();
        let spec = simple_operator_spec(&tmp, "fastqc", "1", "bio@builtin");
        let registry = OperatorRegistryFile {
            enabled: BTreeMap::from([(
                "fastqc_legacy".to_string(),
                OperatorRegistryEntry::Full {
                    operator_id: Some("fastqc".to_string()),
                    source_plugin: Some("bio@builtin".to_string()),
                    version: Some("1".to_string()),
                    enabled: Some(true),
                },
            )]),
        };

        let resolved = resolve_enabled_operators_from(vec![spec], registry);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].alias, "fastqc_legacy");
    }

    #[test]
    fn operator_system_section_tells_agents_to_use_operator_execute() {
        let tmp = TempDir::new().unwrap();
        let section =
            format_enabled_operator_tools_system_section_from_resolved(vec![ResolvedOperator {
                alias: "demo_program".to_string(),
                spec: simple_operator_spec(&tmp, "demo_program", "1", "demo-plugin@test-market"),
            }])
            .expect("operator tools prompt section");

        assert!(section.contains("Plugin operator execution"));
        assert!(section.contains("operator_execute"));
        assert!(section.contains("Subcommands are operations, not separate tools"));
        assert!(section.contains("do not ask the user to manually register"));
    }

    #[test]
    fn operator_tool_schema_uses_dynamic_operator_prefix_for_compatibility() {
        let tmp = TempDir::new().unwrap();
        let schema = operator_tool_schema(ResolvedOperator {
            alias: "fastqc".to_string(),
            spec: simple_operator_spec(&tmp, "fastqc", "1", "bio@builtin"),
        });

        assert_eq!(schema.name, "operator__fastqc");
    }

    #[test]
    fn operator_parameters_schema_adds_operation_enum_for_multi_operation_specs() {
        let tmp = TempDir::new().unwrap();
        let manifest = tmp.path().join("operator.yaml");
        fs::write(
            &manifest,
            r#"
apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: demo_program
  version: "1"
operations:
  sample:
    description: Sample reads
    category: ngs/sequence-processing
    group: Sequence Processing
    stage: NGS / Sequence Processing
  comp:
    description: Summarize composition
    category: ngs/quality-control
    group: Quality Control
    stage: NGS / Quality Control
interface:
  inputs:
    reads:
      kind: file_array
      required: true
execution:
  argv: ["demo-program", "${params.operation}", "${inputs.reads}"]
"#,
        )
        .unwrap();

        let spec = load_operator_manifest(&manifest, "p@m", tmp.path()).unwrap();
        let schema = operator_parameters_schema(&spec);
        let operation = &schema["properties"]["params"]["properties"]["operation"];

        assert_eq!(operation["type"], "string");
        assert_eq!(operation["enum"], json!(["comp", "sample"]));
        assert_eq!(
            spec.operations["sample"].category.as_deref(),
            Some("ngs/sequence-processing")
        );
        assert_eq!(
            operator_operation_groups_for_spec(&spec)
                .iter()
                .map(|group| group.label.as_str())
                .collect::<Vec<_>>(),
            vec!["NGS / Quality Control", "NGS / Sequence Processing"]
        );
    }

    #[test]
    fn registry_requires_disambiguation_for_conflicts() {
        let tmp = TempDir::new().unwrap();
        let registry = OperatorRegistryFile {
            enabled: BTreeMap::from([(
                "fastqc".to_string(),
                OperatorRegistryEntry::Version("1".to_string()),
            )]),
        };
        assert!(resolve_enabled_operators_from(
            vec![
                simple_operator_spec(&tmp, "fastqc", "1", "a"),
                simple_operator_spec(&tmp, "fastqc", "1", "b")
            ],
            registry
        )
        .is_empty());
    }

    #[test]
    fn registry_update_pins_resolved_source_and_version() {
        let tmp = TempDir::new().unwrap();
        let spec = simple_operator_spec(&tmp, "fastqc", "0.12.1", "bio@builtin");
        let mut registry = OperatorRegistryFile::default();
        apply_operator_registry_update(
            &mut registry,
            vec![spec],
            OperatorRegistryUpdate {
                alias: "fastqc".to_string(),
                operator_id: None,
                source_plugin: None,
                version: None,
                enabled: true,
            },
        )
        .unwrap();
        match registry.enabled.get("fastqc").unwrap() {
            OperatorRegistryEntry::Full {
                operator_id,
                source_plugin,
                version,
                enabled,
            } => {
                assert_eq!(operator_id.as_deref(), Some("fastqc"));
                assert_eq!(source_plugin.as_deref(), Some("bio@builtin"));
                assert_eq!(version.as_deref(), Some("0.12.1"));
                assert_eq!(*enabled, Some(true));
            }
            other => panic!("expected full registry entry, got {other:?}"),
        }
    }

    #[test]
    fn expands_array_inputs_as_multiple_argv_tokens() {
        let tmp = TempDir::new().unwrap();
        let spec = argv_operator_spec(&tmp, &["cat", "${inputs.files}"]);
        let argv = expand_argv(
            &spec,
            &BTreeMap::from([("files".to_string(), json!(["a.txt", "b.txt"]))]),
            &BTreeMap::new(),
            &BTreeMap::new(),
            "/run",
        )
        .unwrap();
        assert_eq!(argv, vec!["cat", "a.txt", "b.txt"]);
    }

    #[test]
    fn expands_missing_optional_argv_fields_as_empty_strings() {
        let tmp = TempDir::new().unwrap();
        let mut spec = argv_operator_spec(
            &tmp,
            &[
                "Rscript",
                "plot.R",
                "${inputs.metadata}",
                "{{ params.label }}",
                "${resources.cache_dir}",
            ],
        );
        spec.interface = OperatorInterfaceSpec {
            inputs: BTreeMap::from([(
                "metadata".to_string(),
                OperatorFieldSpec {
                    kind: OperatorFieldKind::File,
                    required: false,
                    ..OperatorFieldSpec::default()
                },
            )]),
            params: BTreeMap::from([(
                "label".to_string(),
                OperatorFieldSpec {
                    kind: OperatorFieldKind::String,
                    required: false,
                    ..OperatorFieldSpec::default()
                },
            )]),
            ..OperatorInterfaceSpec::default()
        };
        spec.resources = BTreeMap::from([(
            "cache_dir".to_string(),
            OperatorResourceSpec {
                exposed: false,
                ..OperatorResourceSpec::default()
            },
        )]);
        let argv = expand_argv(
            &spec,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            "/run",
        )
        .unwrap();
        assert_eq!(argv, vec!["Rscript", "plot.R", "", "", ""]);

        let canonical = canonicalize_inputs(
            &crate::domain::tools::ToolContext::new(tmp.path()),
            &spec,
            BTreeMap::from([("metadata".to_string(), json!(""))]),
            false,
        )
        .unwrap();
        assert!(!canonical.contains_key("metadata"));
    }

    #[test]
    fn validates_params_resources_and_container_runtime_support() {
        let tmp = TempDir::new().unwrap();
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "container_op".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec {
                params: BTreeMap::from([(
                    "repeat".to_string(),
                    OperatorFieldSpec {
                        kind: OperatorFieldKind::Integer,
                        required: true,
                        minimum: Some(1.0),
                        maximum: Some(2.0),
                        ..OperatorFieldSpec::default()
                    },
                )]),
                ..OperatorInterfaceSpec::default()
            },
            operations: BTreeMap::new(),
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["true".to_string()],
            },
            preflight: None,
            runtime: Some(json!({
                "placement": { "supported": ["local"] },
                "container": { "supported": ["docker"] },
                "scheduler": { "supported": ["none"] }
            })),
            cache: None,
            resources: BTreeMap::from([(
                "cpu".to_string(),
                OperatorResourceSpec {
                    default: Some(json!(1)),
                    min: Some(json!(1)),
                    max: Some(json!(4)),
                    exposed: true,
                },
            )]),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "p".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };

        let docker_ctx = crate::domain::tools::ToolContext::new(tmp.path())
            .with_execution_environment("sandbox")
            .with_sandbox_backend("docker");
        assert!(runtime_supported(&docker_ctx, &spec));
        assert!(!runtime_supported(
            &crate::domain::tools::ToolContext::new(tmp.path()),
            &spec
        ));

        let params = BTreeMap::from([("repeat".to_string(), json!(3))]);
        let err = validate_field_values("params", &spec.interface.params, &params).unwrap_err();
        assert_eq!(err.field.as_deref(), Some("params.repeat"));

        let err = apply_resource_defaults_and_overrides(
            &spec,
            BTreeMap::from([("cpu".to_string(), json!(8))]),
        )
        .unwrap_err();
        assert_eq!(err.field.as_deref(), Some("resources.cpu"));
    }

    #[test]
    fn retry_policy_only_retries_retryable_infrastructure_errors() {
        let tmp = TempDir::new().unwrap();
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "retry_op".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec::default(),
            operations: BTreeMap::new(),
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["true".to_string()],
            },
            preflight: None,
            runtime: Some(json!({
                "placement": { "supported": ["local"] },
                "container": { "supported": ["none"] },
                "scheduler": { "supported": ["none"] },
                "retry": { "maxAttempts": 4 }
            })),
            cache: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "p".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };
        let policy = operator_retry_policy(&spec);
        assert_eq!(policy.max_attempts, 4);

        let infra = OperatorToolError::new("execution_infra_error", true, "backend failed");
        assert!(should_retry_operator_error(&infra, &policy, 1));
        assert!(!should_retry_operator_error(&infra, &policy, 4));

        let tool_exit = OperatorToolError::new("tool_exit_nonzero", false, "exit 2");
        assert!(!should_retry_operator_error(&tool_exit, &policy, 1));

        let validation = OperatorToolError::new("input_validation_failed", true, "bad input");
        assert!(!should_retry_operator_error(&validation, &policy, 1));
    }

    #[test]
    fn retry_metadata_is_recorded_in_status_and_failure_payloads() {
        let previous = OperatorRetryAttemptSummary {
            attempt: 1,
            kind: "environment_unavailable".to_string(),
            retryable: true,
            message: "temporary backend issue".to_string(),
        };
        let retry = OperatorRetryState {
            attempt: 2,
            max_attempts: 3,
            previous_errors: vec![previous.clone()],
        };
        let metadata = OperatorRunStatusMetadata {
            run_id: "oprun_20260507000000_retry".to_string(),
            location: "local".to_string(),
            operator: OperatorRunIdentity {
                alias: "retry_op".to_string(),
                id: "retry_op".to_string(),
                version: "1".to_string(),
                source_plugin: "p".to_string(),
                manifest_path: "/tmp/operator.yaml".to_string(),
            },
            run_dir: "/tmp/oprun_retry".to_string(),
            run_context: None,
            retry: Some(retry.clone()),
        };
        let mut status = json!({"status": "running"});
        apply_status_metadata(&mut status, Some(&metadata));
        assert_eq!(status["attempt"], json!(2));
        assert_eq!(status["maxAttempts"], json!(3));
        assert_eq!(
            status["previousErrors"][0]["kind"],
            "environment_unavailable"
        );

        let error = OperatorToolError::new("execution_infra_error", true, "backend failed")
            .with_retry_state(&retry);
        let raw = failure_json("retry_op", None, Some("/tmp/oprun_retry"), None, error);
        let payload = serde_json::from_str::<JsonValue>(&raw).unwrap();
        assert_eq!(payload["error"]["attempt"], json!(2));
        assert_eq!(payload["error"]["maxAttempts"], json!(3));
        assert_eq!(payload["error"]["previousErrors"][0]["attempt"], json!(1));
    }

    #[test]
    fn failure_json_includes_provisioning_failure_when_present() {
        let error = OperatorToolError::new("tool_exit_nonzero", false, "exit 127")
            .with_provisioning_failure(Some(
                crate::domain::environment_fallback::ProvisioningFailure {
                    kind: crate::domain::environment_fallback::ProvisioningFailureKind::DockerRuntimeMissing,
                    suggestions: vec![crate::domain::environment_fallback::FallbackSuggestion {
                        title: "start docker".to_string(),
                        detail: "start docker daemon".to_string(),
                        action: "start_docker_daemon".to_string(),
                    }],
                },
            ));
        let raw = failure_json("x", None, Some("/tmp/oprun_pf"), None, error);
        let payload = serde_json::from_str::<JsonValue>(&raw).unwrap();
        assert_eq!(
            payload["error"]["provisioningFailure"]["kind"],
            "dockerRuntimeMissing"
        );
        assert!(!payload["error"]["provisioningFailure"]["suggestions"]
            .as_array()
            .expect("provisioning suggestions")
            .is_empty());
    }

    #[test]
    fn provisioning_failure_can_be_built_from_stderr_marker() {
        let provisioning_failure = provisioning_failure_for_error(
            Some(127),
            Some("Docker runtime is required for this Operator environment but `docker` was not found."),
        );
        assert_eq!(
            provisioning_failure.as_ref().map(|entry| &entry.kind),
            Some(
                &crate::domain::environment_fallback::ProvisioningFailureKind::DockerRuntimeMissing
            )
        );
        let error = OperatorToolError::new("tool_exit_nonzero", false, "exit 127")
            .with_provisioning_failure(provisioning_failure);
        let raw = failure_json("x", None, Some("/tmp/oprun_pf"), None, error);
        let payload = serde_json::from_str::<JsonValue>(&raw).unwrap();
        assert_eq!(
            payload["error"]["provisioningFailure"]["kind"],
            "dockerRuntimeMissing"
        );
        assert!(!payload["error"]["provisioningFailure"]["suggestions"]
            .as_array()
            .expect("provisioning suggestions")
            .is_empty());
    }

    #[test]
    fn failure_json_omits_provisioning_failure_when_absent() {
        let error = OperatorToolError::new("tool_exit_nonzero", false, "exit 2");
        let raw = failure_json("x", None, Some("/tmp/oprun_pf"), None, error);
        let payload = serde_json::from_str::<JsonValue>(&raw).unwrap();
        assert!(payload["error"]["provisioningFailure"].is_null());
    }

    #[test]
    fn parses_remote_sha256_and_falls_back_to_reference_fingerprint() {
        let checksum = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let parsed = parse_remote_path_fingerprint(
            "ssh",
            "/remote/input.txt",
            &format!("__OMIGA_FILE__\n12\n1770000000\n{checksum}\n"),
        );
        assert_eq!(parsed["mode"], "sha256");
        assert_eq!(parsed["algorithm"], "sha256");
        assert_eq!(parsed["sha256"], checksum);
        assert_eq!(parsed["fingerprint"], format!("sha256:{checksum}"));
        assert_eq!(parsed["size"], json!(12));
        assert_eq!(parsed["modifiedUnixSecs"], json!(1770000000_u64));

        let missing =
            parse_remote_path_fingerprint("ssh", "/remote/missing.txt", "__OMIGA_MISSING__\n");
        assert_eq!(missing["mode"], "reference");
        assert_eq!(missing["available"], false);

        let no_checksum = parse_remote_path_fingerprint(
            "ssh",
            "/remote/input.txt",
            "__OMIGA_FILE__\n12\n1770000000\n\n",
        );
        assert_eq!(no_checksum["mode"], "stat");
        assert!(no_checksum.get("sha256").is_none());
    }

    #[test]
    fn ssh_operator_run_dirs_use_session_workspace_root() {
        let ctx = crate::domain::tools::ToolContext::new("/remote/work/data/query")
            .with_execution_environment("ssh")
            .with_ssh_server(Some("gpu".to_string()));

        let run_surface = OperatorExecutionSurface::for_context(&ctx, "oprun_123");
        assert_eq!(run_surface.kind, OperatorExecutionSurfaceKind::Ssh);
        assert_eq!(
            run_surface.run_dir,
            "/remote/work/data/query/.omiga/runs/oprun_123"
        );

        let runs_surface = OperatorExecutionSurface::for_runs_root(&ctx);
        assert_eq!(runs_surface.kind, OperatorExecutionSurfaceKind::Ssh);
        assert_eq!(runs_surface.run_dir, "/remote/work/data/query/.omiga/runs");
    }

    #[test]
    fn builds_docker_operator_command_for_local_container_runtime() {
        let tmp = TempDir::new().unwrap();
        let input = tmp.path().join("data.txt");
        fs::write(&input, "hello\n").unwrap();
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "container_op".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec {
                inputs: BTreeMap::from([(
                    "input".to_string(),
                    OperatorFieldSpec {
                        kind: OperatorFieldKind::File,
                        required: true,
                        ..OperatorFieldSpec::default()
                    },
                )]),
                ..OperatorInterfaceSpec::default()
            },
            operations: BTreeMap::new(),
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["/bin/cat".to_string(), "${inputs.input}".to_string()],
            },
            preflight: None,
            runtime: Some(json!({
                "placement": { "supported": ["local"] },
                "container": {
                    "supported": ["docker"],
                    "image": "alpine:3.19"
                },
                "scheduler": { "supported": ["none"] }
            })),
            cache: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "p".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path()).with_sandbox_backend("docker");
        assert!(runtime_supported(&ctx, &spec));
        let inputs = BTreeMap::from([(
            "input".to_string(),
            JsonValue::String(input.to_string_lossy().into_owned()),
        )]);
        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_container",
            &["/bin/cat".to_string(), input.to_string_lossy().into_owned()],
            &inputs,
        );

        assert!(command.contains("'docker' 'run' '--rm'"));
        assert!(command.contains("'alpine:3.19'"));
        assert!(command.contains("'/tmp/oprun_container:/tmp/oprun_container'"));
        assert!(command.contains(&format!(
            "'{}:{}:ro'",
            input.to_string_lossy(),
            input.to_string_lossy()
        )));
        assert!(command.contains("logs/stdout.txt"));
    }

    #[test]
    fn builds_singularity_operator_command_from_manifest_image() {
        let tmp = TempDir::new().unwrap();
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "singularity_op".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec::default(),
            operations: BTreeMap::new(),
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["/bin/true".to_string()],
            },
            preflight: None,
            runtime: Some(json!({
                "placement": { "supported": ["local"] },
                "container": {
                    "supported": ["singularity"],
                    "images": { "singularity": "docker://alpine:3.19" }
                },
                "scheduler": { "supported": ["none"] }
            })),
            cache: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "p".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };
        let ctx =
            crate::domain::tools::ToolContext::new(tmp.path()).with_sandbox_backend("singularity");
        assert!(runtime_supported(&ctx, &spec));

        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_singularity",
            &["/bin/true".to_string()],
            &BTreeMap::new(),
        );

        assert!(command.contains("'singularity' 'exec'"));
        assert!(command.contains("'--pwd' '/tmp/oprun_singularity'"));
        assert!(command.contains("'docker://alpine:3.19'"));
        assert!(command.contains("logs/stdout.txt"));
    }

    fn write_conda_environment_profile(plugin_root: &Path, id: &str) {
        let env_dir = plugin_root.join("environments").join(id);
        fs::create_dir_all(&env_dir).unwrap();
        fs::write(
            env_dir.join("environment.yaml"),
            format!(
                r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: {id}
  version: 0.1.0
runtime:
  type: conda
  condaEnvFile: ./conda.yaml
diagnostics:
  checkCommand: [demoalign, --version]
"#
            ),
        )
        .unwrap();
        fs::write(
            env_dir.join("conda.yaml"),
            "channels:\n  - conda-forge\n  - bioconda\ndependencies:\n  - demoalign =1.0\n",
        )
        .unwrap();
    }

    #[test]
    fn local_operator_command_wraps_conda_environment_ref() {
        let tmp = TempDir::new().unwrap();
        write_conda_environment_profile(tmp.path(), "alignment-env");
        let mut spec = argv_operator_spec(&tmp, &["demoalign", "index", "ref.fa"]);
        spec.runtime = Some(json!({
            "envRef": "alignment-env",
            "placement": { "supported": ["local"] },
            "container": { "supported": ["none"] }
        }));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_conda",
            &spec.execution.argv,
            &BTreeMap::new(),
        );

        assert!(command.contains("$HOME/.omiga/bin/micromamba"));
        assert!(command.contains("OMIGA_MICROMAMBA"));
        assert!(command.contains("Automatic micromamba installation failed"));
        assert!(command.contains("env create -y -p"));
        assert!(command.contains("run -p"));
        assert!(command.contains(".omiga/operator-envs/conda"));
        assert!(command.contains("demoalign"));
        assert!(command.contains("ref.fa"));
        assert!(command.contains("omiga_bootstrap_micromamba"));
        assert!(command.contains("OMIGA_DISABLE_MICROMAMBA_BOOTSTRAP"));
        assert!(command.contains(".micromamba.tmp-$$"));
        assert!(command.contains("if [ -z \"$OMIGA_CONDA_BIN\" ]; then"));
        assert!(command.contains("omiga_bootstrap_micromamba || true"));
        assert!(command.contains("omiga_missing_conda_manager"));
        let find_pos = command
            .find("omiga_find_conda_manager || true")
            .expect("find manager check");
        let bootstrap_pos = find_pos
            + command[find_pos..]
                .find("omiga_bootstrap_micromamba || true")
                .expect("bootstrap call exists");
        let missing_pos = bootstrap_pos
            + command[bootstrap_pos..]
                .find("omiga_missing_conda_manager")
                .expect("missing fallback exists");
        assert!(find_pos < bootstrap_pos);
        assert!(bootstrap_pos < missing_pos);
    }

    #[test]
    fn conda_environment_shell_script_filters_sensitive_env_vars() {
        let selection = OperatorCondaEnvironmentSelection {
            env_prefix: "/tmp/oprun_conda_envs/alignment".to_string(),
            env_yaml_b64: "Y29uZGEtZW52".to_string(),
            env_hash: "abcd1234".to_string(),
            env_vars: BTreeMap::from([
                ("MY_API_KEY".to_string(), "x".to_string()),
                ("NORMAL_VAR".to_string(), "y".to_string()),
            ]),
        };
        let _keep_guard = ScopedEnvKeep::unset();

        let command = conda_environment_shell_script(&selection, "/tmp/oprun_conda", "demoalign");

        assert!(command.contains("export NORMAL_VAR='y'"));
        assert!(!command.contains("MY_API_KEY"));
    }

    #[tokio::test]
    async fn execute_env_command_filters_sensitive_env_vars_for_operator_run() {
        let _lock = OPERATOR_ENV_HYGIENE_LOCK.lock().unwrap();
        let _keep = ScopedEnvVar::remove("OMIGA_ENV_KEEP");
        let _token = ScopedEnvVar::set("FAKE_SECRET_TOKEN", "absent-if-visible");

        let tmp = TempDir::new().unwrap();
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let result = execute_env_command(
            &ctx,
            &tmp.path().to_string_lossy(),
            "sh -c 'echo ${FAKE_SECRET_TOKEN:-absent}'",
            30,
        )
        .await
        .unwrap();

        assert_eq!(result.output.trim(), "absent");
    }

    #[tokio::test]
    async fn execute_env_command_keeps_allowed_vars_and_preserves_path_home() {
        let _lock = OPERATOR_ENV_HYGIENE_LOCK.lock().unwrap();
        let _token = ScopedEnvVar::set("FAKE_SECRET_TOKEN", "allowed-token");
        let _keep = ScopedEnvVar::set("OMIGA_ENV_KEEP", "FAKE_SECRET_TOKEN");
        let _path = ScopedEnvVar::set("PATH", "/bin:/tmp/operator-path");
        let _home = ScopedEnvVar::set("HOME", "/tmp/operator-home");

        let tmp = TempDir::new().unwrap();
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let value = execute_env_command(
            &ctx,
            &tmp.path().to_string_lossy(),
            r#"sh -c 'echo $FAKE_SECRET_TOKEN:$PATH:$HOME'"#,
            30,
        )
        .await
        .unwrap();

        let output = value.output.trim().to_string();
        let mut parts = output.split(':').collect::<Vec<_>>();
        assert!(parts.len() >= 3);
        assert_eq!(parts.remove(0), "allowed-token");
        assert_eq!(parts.pop(), Some("/tmp/operator-home"));
        assert!(parts.join(":").contains("/tmp/operator-path"));
    }

    #[test]
    fn provisioning_failure_markers_in_generated_scripts_match_classifier() {
        let tmp = TempDir::new().unwrap();
        let operator_script = conda_environment_shell_script(
            &OperatorCondaEnvironmentSelection {
                env_prefix: tmp
                    .path()
                    .join("oprun_conda_env")
                    .to_string_lossy()
                    .into_owned(),
                env_yaml_b64: "Y29uZGEtZW52".to_string(),
                env_hash: "abcd1234".to_string(),
                env_vars: BTreeMap::new(),
            },
            "/tmp/oprun_conda",
            "demoalign",
        );
        let plugin_script = crate::domain::plugins::conda_environment_check_shell_script(
            &tmp.path().join("conda_prefix"),
            &tmp.path().join("conda.yaml"),
            "abcd1234",
            &BTreeMap::new(),
            "demoalign",
        );
        let docker_preflight = container_runtime_preflight_script(OperatorContainerKind::Docker);
        let singularity_preflight =
            container_runtime_preflight_script(OperatorContainerKind::Singularity);

        let checks = [
            (
                operator_script.as_str(),
                "Automatic micromamba installation failed (reason above).",
                ProvisioningFailureKind::MicromambaBootstrapFailed,
            ),
            (
                plugin_script.as_str(),
                "No micromamba, mamba, or conda executable was found in the active PATH/base environment/virtual environment.",
                ProvisioningFailureKind::CondaManagerMissing,
            ),
            (
                docker_preflight,
                "Docker runtime is required for this Operator environment but `docker` was not found in the active PATH/base environment/virtual environment.",
                ProvisioningFailureKind::DockerRuntimeMissing,
            ),
            (
                singularity_preflight,
                "Singularity/Apptainer runtime is required for this Operator environment but neither `singularity` nor `apptainer` was found in the active PATH/base environment/virtual environment.",
                ProvisioningFailureKind::SingularityRuntimeMissing,
            ),
        ];

        for (script, marker, expected) in checks {
            assert!(script.contains(marker), "marker missing: {marker}");
            assert_eq!(
                classify_provisioning_failure(Some(127), marker),
                Some(expected),
                "unexpected classification for marker: {marker}"
            );
        }
    }

    fn fake_tool_dir(base: &Path, commands: &[(&str, &str)]) -> PathBuf {
        let bin = base.join("bin");
        fs::create_dir_all(&bin).unwrap();
        for (name, script) in commands {
            let path = bin.join(name);
            fs::write(&path, script).unwrap();
            let output = std::process::Command::new("chmod")
                .arg("+x")
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        bin
    }

    #[test]
    fn conda_bootstrap_shell_script_downloads_fake_micromamba() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let bin = fake_tool_dir(
            tmp.path(),
            &[
                (
                    "curl",
                    "#!/bin/sh\n\nout=\n\nwhile [ \"$1\" != \"\" ]; do\n  if [ \"$1\" = \"-o\" ]; then\n    out=\"$2\"\n  fi\n  shift\ndone\n[ -n \"$out\" ] || exit 1\nprintf '%s\\n' '#!/bin/sh' > \"$out\"\nprintf '%s\\n' 'echo 1.0.0' >> \"$out\"\nchmod +x \"$out\"\n",                
                ),
            ],
        );
        let script = format!(
            "{}\nif omiga_bootstrap_micromamba; then :; else exit 1; fi",
            MICROMAMBA_BOOTSTRAP_SHELL
        );

        let output = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .current_dir(&tmp.path())
            .env("HOME", &home)
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env(
                "OMIGA_MICROMAMBA_URL",
                "https://example.invalid/micromamba-linux-64",
            )
            .output()
            .unwrap();

        assert!(output.status.success(), "{:?}", output);
        let target = home.join(".omiga/bin/micromamba");
        assert!(target.is_file(), "missing target: {}", target.display());
        #[cfg(unix)]
        {
            let metadata = fs::metadata(&target).unwrap();
            use std::os::unix::fs::PermissionsExt;
            assert!(metadata.permissions().mode() & 0o111 != 0);
        }
        let script_text = fs::read_to_string(&target).unwrap();
        assert!(script_text.contains("#!/bin/sh"));
        assert!(script_text.contains("echo 1.0.0"));
    }

    #[test]
    fn conda_bootstrap_shell_script_disables_with_env_switch() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let bin = fake_tool_dir(
            tmp.path(),
            &[("uname", "#!/bin/sh\nprintf 'Linux'\nprintf 'x86_64'\n")],
        );
        let script = format!(
            "{}\nif omiga_bootstrap_micromamba; then :; else exit 1; fi",
            MICROMAMBA_BOOTSTRAP_SHELL
        );

        let output = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .env("HOME", &home)
            .env("PATH", format!("{}", bin.display()))
            .env("OMIGA_DISABLE_MICROMAMBA_BOOTSTRAP", "1")
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(1));
        assert!(!home.join(".omiga/bin/micromamba").is_file());
    }

    #[test]
    fn conda_bootstrap_shell_script_fails_without_downloaders() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let bin = fake_tool_dir(
            tmp.path(),
            &[
                (
                    "uname",
                    "#!/bin/sh\ncase \"$1\" in -s) echo Linux ;; -m) echo x86_64 ;; esac",
                ),
                ("mkdir", "#!/bin/sh\n/bin/mkdir -p \"$@\"\n"),
                ("rm", "#!/bin/sh\n/bin/rm -f \"$@\"\n"),
            ],
        );
        let script = format!(
            "{}\nif omiga_bootstrap_micromamba; then :; else exit 1; fi",
            MICROMAMBA_BOOTSTRAP_SHELL
        );

        let output = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .current_dir(&tmp.path())
            .env("HOME", &home)
            .env("PATH", bin.display().to_string())
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("no supported downloader"));
        assert!(!home.join(".omiga/bin/micromamba").is_file());
    }

    #[test]
    fn conda_environment_ref_rejects_non_yaml_environment_file() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().join("environments").join("bad-conda");
        fs::create_dir_all(&env_dir).unwrap();
        fs::write(
            env_dir.join("environment.yaml"),
            r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: bad-conda
  version: 0.1.0
runtime:
  type: conda
  condaEnvFile: ./requirements.txt
"#,
        )
        .unwrap();
        fs::write(env_dir.join("requirements.txt"), "demoalign\n").unwrap();
        let mut spec = argv_operator_spec(&tmp, &["demoalign", "index", "ref.fa"]);
        spec.runtime = Some(json!({
            "envRef": "bad-conda",
            "placement": { "supported": ["local"] }
        }));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_bad_conda",
            &spec.execution.argv,
            &BTreeMap::new(),
        );

        assert!(command.contains("must use a `.yaml` or `.yml` file"));
        assert!(command.contains("requirements.txt"));
    }

    #[test]
    fn environment_profile_can_select_container_runtime() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().join("environments").join("docker-env");
        fs::create_dir_all(&env_dir).unwrap();
        fs::write(
            env_dir.join("environment.yaml"),
            r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: docker-env
  version: 0.1.0
runtime:
  type: docker
  image: alpine:3.19
"#,
        )
        .unwrap();
        let mut spec = argv_operator_spec(&tmp, &["echo", "hello"]);
        spec.runtime = Some(json!({
            "envRef": "docker-env",
            "placement": { "supported": ["local"] }
        }));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let selection =
            operator_container_for_command(&ctx, &spec, OperatorExecutionSurfaceKind::Local)
                .expect("container selection");

        assert_eq!(selection.kind, OperatorContainerKind::Docker);
        assert_eq!(selection.image, "alpine:3.19");
    }

    #[test]
    fn docker_environment_profile_builds_from_standard_dockerfile() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().join("environments").join("docker-env");
        fs::create_dir_all(&env_dir).unwrap();
        fs::write(
            env_dir.join("environment.yaml"),
            r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: docker-env
  version: 0.1.0
runtime:
  type: docker
  dockerfile: ./Dockerfile
"#,
        )
        .unwrap();
        fs::write(env_dir.join("Dockerfile"), "FROM alpine:3.19\n").unwrap();
        let mut spec = argv_operator_spec(&tmp, &["echo", "hello"]);
        spec.runtime = Some(json!({
            "envRef": "docker-env",
            "placement": { "supported": ["local"] }
        }));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_docker_env",
            &spec.execution.argv,
            &BTreeMap::new(),
        );

        assert!(command.contains("command -v docker"));
        assert!(command.contains("docker version"));
        assert!(command.contains("docker build -t"));
        assert!(command.contains("'docker' 'run'"));
        assert!(command.contains("omiga-env-"));
        assert!(command.contains("docker-env"));
        assert!(command.contains("logs/stderr.txt"));
    }

    #[test]
    fn singularity_environment_profile_builds_from_standard_definition() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().join("environments").join("singularity-env");
        fs::create_dir_all(&env_dir).unwrap();
        fs::write(
            env_dir.join("environment.yaml"),
            r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: singularity-env
  version: 0.1.0
runtime:
  type: singularity
  definitionFile: ./singularity.def
"#,
        )
        .unwrap();
        fs::write(
            env_dir.join("singularity.def"),
            "Bootstrap: docker\nFrom: alpine:3.19\n",
        )
        .unwrap();
        let mut spec = argv_operator_spec(&tmp, &["echo", "hello"]);
        spec.runtime = Some(json!({
            "envRef": "singularity-env",
            "placement": { "supported": ["local"] }
        }));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_singularity_env",
            &spec.execution.argv,
            &BTreeMap::new(),
        );

        assert!(command.contains("command -v singularity"));
        assert!(command.contains("command -v apptainer"));
        assert!(command.contains("singularity build"));
        assert!(command.contains("'singularity' 'exec'"));
        assert!(command.contains(".omiga/operator-envs/singularity"));
    }

    #[test]
    fn container_environment_profile_without_image_or_file_reports_guidance() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().join("environments").join("docker-env");
        fs::create_dir_all(&env_dir).unwrap();
        fs::write(
            env_dir.join("environment.yaml"),
            r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: docker-env
  version: 0.1.0
runtime:
  type: docker
"#,
        )
        .unwrap();
        let mut spec = argv_operator_spec(&tmp, &["echo", "hello"]);
        spec.runtime = Some(json!({
            "envRef": "docker-env",
            "placement": { "supported": ["local"] }
        }));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_missing_container_env",
            &spec.execution.argv,
            &BTreeMap::new(),
        );

        assert!(command.contains("requires runtime.image or a standard `Dockerfile`"));
        assert!(!command.contains("'echo' 'hello' > logs/stdout.txt"));
    }

    #[test]
    fn local_runtime_prefers_no_container_when_manifest_allows_none() {
        let tmp = TempDir::new().unwrap();
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "portable_op".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec::default(),
            operations: BTreeMap::new(),
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["/bin/true".to_string()],
            },
            preflight: None,
            runtime: Some(json!({
                "placement": { "supported": ["local"] },
                "container": { "supported": ["none", "docker"] },
                "scheduler": { "supported": ["none"] }
            })),
            cache: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "p".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path()).with_sandbox_backend("docker");
        assert!(runtime_supported(&ctx, &spec));

        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_none",
            &["/bin/true".to_string()],
            &BTreeMap::new(),
        );

        assert!(!command.contains("'docker' 'run'"));
        assert!(command.starts_with("set +e"));
    }

    #[test]
    fn lists_and_reads_local_operator_runs() {
        let tmp = TempDir::new().unwrap();
        let runs_root = tmp.path().join(".omiga/runs");
        let succeeded = runs_root.join("oprun_20260506_success");
        let failed = runs_root.join("oprun_20260506_failed");
        fs::create_dir_all(&succeeded).unwrap();
        fs::create_dir_all(&failed).unwrap();
        write_json_file(
            &succeeded.join("provenance.json"),
            &json!({
                "status": "succeeded",
                "runId": "oprun_20260506_success",
                "operator": {
                    "alias": "write_text_report",
                    "id": "write_text_report",
                    "version": "0.1.0",
                    "sourcePlugin": "operator-smoke@omiga-curated"
                },
                "runDir": succeeded.to_string_lossy(),
                "provenancePath": succeeded.join("provenance.json").to_string_lossy(),
                "outputs": {
                    "report": [
                        { "location": "local", "path": succeeded.join("out/report.txt").to_string_lossy() }
                    ]
                },
                "structuredOutputs": {
                    "summary": { "lineCount": 2 },
                    "ok": true
                },
                "cache": {
                    "key": "sha256:test-cache-key",
                    "hit": true,
                    "sourceRunId": "oprun_20260506_source",
                    "sourceRunDir": succeeded.parent().unwrap().join("oprun_20260506_source").to_string_lossy()
                }
            }),
        )
        .unwrap();
        write_json_file(
            &succeeded.join("status.json"),
            &json!({
                "status": "succeeded",
                "updatedAt": "2026-05-06T12:00:00Z",
                "error": null
            }),
        )
        .unwrap();
        write_json_file(
            &failed.join("status.json"),
            &json!({
                "status": "failed",
                "updatedAt": "2026-05-06T11:00:00Z",
                "operator": {
                    "alias": "write_text_report",
                    "id": "write_text_report",
                    "version": "0.1.0",
                    "sourcePlugin": "operator-smoke@omiga-curated"
                },
                "runContext": {
                    "kind": "smoke",
                    "smokeTestId": "default",
                    "smokeTestName": "Write text report smoke"
                },
                "error": {
                    "kind": "tool_exit_nonzero",
                    "retryable": false,
                    "message": "bad input",
                    "suggestedAction": "Inspect stdout/stderr, then adjust inputs or params and retry.",
                    "stdoutTail": "partial stdout\n",
                    "stderrTail": "bad flag\n"
                }
            }),
        )
        .unwrap();

        let runs = list_local_operator_runs(tmp.path(), 10);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].run_id, "oprun_20260506_success");
        assert_eq!(runs[0].operator_alias.as_deref(), Some("write_text_report"));
        assert_eq!(
            runs[0].source_plugin.as_deref(),
            Some("operator-smoke@omiga-curated")
        );
        assert_eq!(runs[0].output_count, 1);
        assert_eq!(runs[0].structured_output_count, 2);
        assert_eq!(runs[0].cache_key.as_deref(), Some("sha256:test-cache-key"));
        assert_eq!(runs[0].cache_hit, Some(true));
        assert_eq!(
            runs[0].cache_source_run_id.as_deref(),
            Some("oprun_20260506_source")
        );
        assert!(runs[0]
            .cache_source_run_dir
            .as_deref()
            .unwrap_or_default()
            .ends_with("oprun_20260506_source"));
        assert_eq!(runs[1].status, "failed");
        assert_eq!(runs[1].operator_alias.as_deref(), Some("write_text_report"));
        assert_eq!(runs[1].run_kind.as_deref(), Some("smoke"));
        assert_eq!(runs[1].smoke_test_id.as_deref(), Some("default"));
        assert_eq!(runs[1].error_message.as_deref(), Some("bad input"));
        assert_eq!(runs[1].error_kind.as_deref(), Some("tool_exit_nonzero"));
        assert_eq!(runs[1].retryable, Some(false));
        assert_eq!(
            runs[1].suggested_action.as_deref(),
            Some("Inspect stdout/stderr, then adjust inputs or params and retry.")
        );
        assert_eq!(runs[1].stdout_tail.as_deref(), Some("partial stdout\n"));
        assert_eq!(runs[1].stderr_tail.as_deref(), Some("bad flag\n"));

        let detail = read_local_operator_run(tmp.path(), "oprun_20260506_success").unwrap();
        assert_eq!(detail["operator"]["id"], "write_text_report");
        assert!(read_local_operator_run(tmp.path(), "../oprun_escape").is_err());
    }

    #[tokio::test]
    async fn reads_local_operator_run_detail_and_log_through_context() {
        let tmp = TempDir::new().unwrap();
        let run_id = "oprun_20260506_detail";
        let run_dir = tmp.path().join(".omiga/runs").join(run_id);
        fs::create_dir_all(run_dir.join("logs")).unwrap();
        fs::create_dir_all(run_dir.join("out")).unwrap();
        fs::write(run_dir.join("logs/stdout.txt"), "operator stdout\n").unwrap();
        fs::write(run_dir.join("logs/stderr.txt"), "").unwrap();
        fs::write(run_dir.join("out/report.txt"), "hello\n").unwrap();
        write_json_file(
            &run_dir.join("provenance.json"),
            &json!({
                "status": "succeeded",
                "runId": run_id,
                "location": "local",
                "operator": {
                    "alias": "write_text_report",
                    "id": "write_text_report",
                    "version": "0.1.0",
                    "sourcePlugin": "operator-smoke@omiga-curated"
                },
                "runDir": run_dir.to_string_lossy(),
                "provenancePath": run_dir.join("provenance.json").to_string_lossy(),
                "outputs": {
                    "report": [
                        { "location": "local", "path": run_dir.join("out/report.txt").to_string_lossy() }
                    ]
                }
            }),
        )
        .unwrap();

        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let runs = list_operator_runs_for_context(&ctx, 10).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run_id, run_id);

        let detail = read_operator_run_for_context(&ctx, run_id).await.unwrap();
        assert_eq!(detail.location, "local");
        assert_eq!(detail.document["runId"], run_id);
        assert!(detail.source_path.ends_with("provenance.json"));

        let log = read_operator_run_log_for_context(&ctx, run_id, "stdout", 1024)
            .await
            .unwrap();
        assert_eq!(log.location, "local");
        assert_eq!(log.log_name, "stdout");
        assert_eq!(log.content, "operator stdout\n");

        let verification = verify_operator_run_for_context(&ctx, run_id).await.unwrap();
        assert!(verification.ok);
        assert!(verification
            .checks
            .iter()
            .any(|check| check.name == "output_artifact:report" && check.ok));
    }

    #[tokio::test]
    async fn cleanup_operator_runs_previews_and_deletes_workspace_scoped_candidates() {
        fn write_run(
            root: &Path,
            run_id: &str,
            status: &str,
            updated_at: &str,
            cache: Option<JsonValue>,
            operator_id: Option<&str>,
        ) -> PathBuf {
            let operator_id = operator_id.unwrap_or("write_text_report");
            let run_dir = root.join(".omiga/runs").join(run_id);
            fs::create_dir_all(run_dir.join("out")).unwrap();
            fs::create_dir_all(run_dir.join("logs")).unwrap();
            fs::write(run_dir.join("out/report.txt"), format!("{run_id}\n")).unwrap();
            fs::write(run_dir.join("logs/stdout.txt"), "").unwrap();
            fs::write(run_dir.join("logs/stderr.txt"), "").unwrap();
            let mut document = json!({
                "status": status,
                "runId": run_id,
                "location": "local",
                "operator": {
                    "alias": operator_id,
                    "id": operator_id,
                    "version": "0.1.0",
                    "sourcePlugin": "operator-smoke@omiga-curated"
                },
                "runDir": run_dir.to_string_lossy(),
                "provenancePath": run_dir.join("provenance.json").to_string_lossy(),
                "outputs": {
                    "report": [
                        { "location": "local", "path": run_dir.join("out/report.txt").to_string_lossy() }
                    ]
                }
            });
            if let Some(cache) = cache {
                document["cache"] = cache;
            }
            write_json_file(&run_dir.join("provenance.json"), &document).unwrap();
            write_json_file(
                &run_dir.join("status.json"),
                &json!({
                    "status": status,
                    "updatedAt": updated_at
                }),
            )
            .unwrap();
            run_dir
        }

        let tmp = TempDir::new().unwrap();
        let latest = write_run(
            tmp.path(),
            "oprun_20990101_latest",
            "succeeded",
            "2099-01-01T00:00:00Z",
            None,
            None,
        );
        let old_success = write_run(
            tmp.path(),
            "oprun_20000101_success",
            "succeeded",
            "2000-01-01T00:00:00Z",
            None,
            None,
        );
        let old_failed = write_run(
            tmp.path(),
            "oprun_20000101_failed",
            "failed",
            "2000-01-01T00:00:00Z",
            None,
            None,
        );
        let cache_hit = write_run(
            tmp.path(),
            "oprun_20000101_cache",
            "succeeded",
            "2000-01-02T00:00:00Z",
            Some(json!({
                "key": "sha256:test",
                "hit": true,
                "sourceRunId": "oprun_20000101_success",
                "sourceRunDir": old_success.to_string_lossy()
            })),
            None,
        );
        let other_operator = write_run(
            tmp.path(),
            "oprun_20000101_other",
            "succeeded",
            "2000-01-01T00:00:00Z",
            None,
            Some("other_operator"),
        );
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let request = OperatorRunCleanupRequest {
            dry_run: true,
            keep_latest: Some(1),
            max_age_days: Some(30),
            include_cache_hits: true,
            include_failed: true,
            include_succeeded: true,
            limit: Some(50),
            operator_alias: None,
            operator_id: Some("write_text_report".to_string()),
            operator_version: Some("0.1.0".to_string()),
            source_plugin: Some("operator-smoke@omiga-curated".to_string()),
        };

        let preview = cleanup_operator_runs_for_context(&ctx, request.clone())
            .await
            .unwrap();
        assert!(preview.dry_run);
        assert_eq!(preview.location, "local");
        assert_eq!(preview.scanned_count, 5);
        assert_eq!(preview.matched_count, 3);
        assert_eq!(preview.deleted_count, 0);
        assert!(preview.estimated_bytes.unwrap_or_default() > 0);
        let preview_ids = preview
            .candidates
            .iter()
            .map(|candidate| candidate.run_id.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            preview_ids,
            BTreeSet::from([
                "oprun_20000101_cache",
                "oprun_20000101_failed",
                "oprun_20000101_success",
            ])
        );
        assert!(latest.is_dir());
        assert!(old_success.is_dir());
        assert!(old_failed.is_dir());
        assert!(cache_hit.is_dir());
        assert!(other_operator.is_dir());

        let result = cleanup_operator_runs_for_context(
            &ctx,
            OperatorRunCleanupRequest {
                dry_run: false,
                ..request
            },
        )
        .await
        .unwrap();
        assert_eq!(result.deleted_count, 3);
        assert_eq!(result.skipped_count, 0);
        assert!(latest.is_dir());
        assert!(!old_success.exists());
        assert!(!old_failed.exists());
        assert!(!cache_hit.exists());
        assert!(other_operator.is_dir());
    }

    #[tokio::test]
    async fn executes_temp_smoke_operator_locally() {
        let tmp = TempDir::new().unwrap();
        let (plugin_root, manifest) = smoke_operator_paths(&tmp);
        let spec =
            load_operator_manifest(&manifest, "operator-smoke@omiga-curated", plugin_root).unwrap();
        let smoke_invocation = spec.smoke_tests[0].arguments.clone();
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let result = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "write_text_report".to_string(),
                spec,
            },
            smoke_invocation,
            Some(OperatorRunContext {
                kind: Some("smoke".to_string()),
                smoke_test_id: Some("default".to_string()),
                smoke_test_name: Some("Write text report smoke".to_string()),
                parent_execution_id: None,
                bypass_cache: false,
            }),
        )
        .await
        .unwrap();

        assert_eq!(result.status, "succeeded");
        assert_eq!(
            result
                .run_context
                .as_ref()
                .and_then(|context| context.kind.as_deref()),
            Some("smoke")
        );
        let runs = list_local_operator_runs(tmp.path(), 10);
        assert_eq!(runs[0].run_kind.as_deref(), Some("smoke"));
        assert_eq!(runs[0].smoke_test_id.as_deref(), Some("default"));
        let report_path = Path::new(&result.outputs["report"][0].path);
        assert_eq!(
            fs::read_to_string(report_path).unwrap(),
            "hello operator smoke\nhello operator smoke\n"
        );
        let export_dir = result
            .export_dir
            .as_deref()
            .expect("successful runs should export results to the session workspace");
        assert_eq!(
            fs::read_to_string(Path::new(export_dir).join("operator-report.txt")).unwrap(),
            "hello operator smoke\nhello operator smoke\n"
        );
        assert!(Path::new(&format!("{}/status.json", result.run_dir)).is_file());
        assert!(Path::new(&format!("{}/provenance.json", result.run_dir)).is_file());
    }

    #[tokio::test]
    async fn cacheable_local_operator_reuses_workspace_run_outputs() {
        let tmp = TempDir::new().unwrap();
        let input = tmp.path().join("input.txt");
        let marker = tmp.path().join("executions.txt");
        fs::write(&input, "first\n").unwrap();
        let spec = cached_report_operator_spec(&tmp, &marker, Some(json!({"enabled": true})));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let invocation = input_file_invocation("input.txt");

        let first = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "cached_report".to_string(),
                spec: spec.clone(),
            },
            invocation.clone(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(first.status, "succeeded");
        assert_eq!(
            first.cache.as_ref().map(|cache| cache.hit),
            Some(false),
            "fresh cache-enabled runs should record their cache key without claiming a hit"
        );
        assert!(first
            .run_dir
            .starts_with(&tmp.path().join(".omiga/runs").to_string_lossy().to_string()));
        assert_eq!(fs::read_to_string(&marker).unwrap(), "run\n");

        let second = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "cached_report".to_string(),
                spec: spec.clone(),
            },
            invocation.clone(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(second.status, "succeeded");
        assert_ne!(second.run_id, first.run_id);
        let second_cache = second.cache.as_ref().expect("second run cache metadata");
        assert!(second_cache.hit);
        assert_eq!(
            second_cache.source_run_id.as_deref(),
            Some(first.run_id.as_str())
        );
        assert_eq!(
            second_cache.source_run_dir.as_deref(),
            Some(first.run_dir.as_str())
        );
        assert_eq!(
            second.outputs["report"][0].path, first.outputs["report"][0].path,
            "cache hits reuse the prior workspace artifact reference instead of copying outputs"
        );
        assert_eq!(
            fs::read_to_string(&marker).unwrap(),
            "run\n",
            "cache hit must not execute the operator command again"
        );
        let second_stdout = fs::read_to_string(Path::new(&second.run_dir).join("logs/stdout.txt"))
            .expect("cache hit stdout log");
        assert!(
            second_stdout.contains(&format!("Operator cache hit: reused run {}.", first.run_id))
        );

        fs::write(&input, "changed\n").unwrap();
        let third = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "cached_report".to_string(),
                spec,
            },
            invocation,
            None,
        )
        .await
        .unwrap();
        assert_eq!(third.status, "succeeded");
        assert_eq!(third.cache.as_ref().map(|cache| cache.hit), Some(false));
        assert_ne!(
            third.outputs["report"][0].path,
            first.outputs["report"][0].path
        );
        assert_eq!(
            fs::read_to_string(&marker).unwrap(),
            "run\nrun\n",
            "changed input fingerprint should miss the workspace cache"
        );
    }

    #[tokio::test]
    async fn smoke_operator_runs_bypass_cache() {
        let tmp = TempDir::new().unwrap();
        let input = tmp.path().join("input.txt");
        let marker = tmp.path().join("smoke-executions.txt");
        fs::write(&input, "smoke\n").unwrap();
        let spec = cached_report_operator_spec(&tmp, &marker, Some(json!({"enabled": true})));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let invocation = input_file_invocation("input.txt");
        let run_context = Some(OperatorRunContext {
            kind: Some("smoke".to_string()),
            smoke_test_id: Some("default".to_string()),
            smoke_test_name: Some("Cache bypass smoke".to_string()),
            parent_execution_id: None,
            bypass_cache: false,
        });

        let first = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "cached_report".to_string(),
                spec: spec.clone(),
            },
            invocation.clone(),
            run_context.clone(),
        )
        .await
        .unwrap();
        let second = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "cached_report".to_string(),
                spec,
            },
            invocation,
            run_context,
        )
        .await
        .unwrap();

        assert!(first.cache.is_none());
        assert!(second.cache.is_none());
        assert_ne!(second.run_id, first.run_id);
        assert_ne!(
            second.outputs["report"][0].path,
            first.outputs["report"][0].path
        );
        assert_eq!(fs::read_to_string(&marker).unwrap(), "run\nrun\n");
    }

    #[tokio::test]
    async fn executes_local_operator_and_collects_outputs() {
        let tmp = TempDir::new().unwrap();
        let input = tmp.path().join("input.txt");
        fs::write(&input, "hello operator\n").unwrap();
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "render_report".to_string(),
                version: "1".to_string(),
                name: None,
                description: Some("render input to report".to_string()),
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec {
                inputs: BTreeMap::from([(
                    "input".to_string(),
                    OperatorFieldSpec {
                        kind: OperatorFieldKind::File,
                        required: true,
                        ..OperatorFieldSpec::default()
                    },
                )]),
                outputs: BTreeMap::from([(
                    "report".to_string(),
                    OperatorFieldSpec {
                        kind: OperatorFieldKind::FileArray,
                        required: true,
                        glob: Some("report.txt".to_string()),
                        ..OperatorFieldSpec::default()
                    },
                ), (
                    "summary".to_string(),
                    OperatorFieldSpec {
                        kind: OperatorFieldKind::Json,
                        required: true,
                        ..OperatorFieldSpec::default()
                    },
                ), (
                    "ok".to_string(),
                    OperatorFieldSpec {
                        kind: OperatorFieldKind::Boolean,
                        required: true,
                        ..OperatorFieldSpec::default()
                    },
                )]),
                ..OperatorInterfaceSpec::default()
            },
            operations: BTreeMap::new(),
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    r#"cat ${inputs.input} > ${outdir}/report.txt; printf '%s\n' '{"summary":{"lineCount":1},"ok":true}' > ${outdir}/outputs.json"#.to_string(),
                ],
            },
            preflight: None,
            runtime: None,
            cache: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "test@local".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let result = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "render_report".to_string(),
                spec,
            },
            OperatorInvocation {
                operation: None,
                inputs: BTreeMap::from([(
                    "input".to_string(),
                    JsonValue::String("input.txt".to_string()),
                )]),
                params: BTreeMap::new(),
                resources: BTreeMap::new(),
                metadata: BTreeMap::new(),
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.status, "succeeded");
        assert_eq!(result.outputs["report"].len(), 1);
        assert!(Path::new(&result.outputs["report"][0].path).is_file());
        let structured_outputs = result.structured_outputs.as_ref().unwrap();
        assert_eq!(structured_outputs["summary"]["lineCount"], json!(1));
        assert_eq!(structured_outputs["ok"], json!(true));
        assert_eq!(
            result.effective_inputs["input"],
            json!(input.canonicalize().unwrap().to_string_lossy().into_owned())
        );
        assert_eq!(result.input_fingerprints["input"]["mode"], "sha256");
        assert_eq!(result.input_fingerprints["input"]["location"], "local");
        assert_eq!(result.input_fingerprints["input"]["algorithm"], "sha256");
        let expected_sha256 = sha256_file(&input.to_string_lossy()).unwrap();
        assert_eq!(
            result.input_fingerprints["input"]["sha256"],
            expected_sha256
        );
        assert_eq!(
            result.input_fingerprints["input"]["fingerprint"],
            format!("sha256:{expected_sha256}")
        );
    }

    #[test]
    fn rejects_invalid_local_structured_output_manifest() {
        let tmp = TempDir::new().unwrap();
        let run_dir = tmp.path().join(".omiga/runs/oprun_structured_invalid");
        let out_dir = run_dir.join("out");
        fs::create_dir_all(&out_dir).unwrap();

        fs::write(out_dir.join(OPERATOR_STRUCTURED_OUTPUTS_FILE), "[]").unwrap();
        let error =
            read_local_structured_outputs(&out_dir, &run_dir.to_string_lossy()).unwrap_err();
        assert_eq!(error.kind, "output_validation_failed");
        assert_eq!(error.field.as_deref(), Some("structuredOutputs"));
        assert!(error.message.contains("JSON object"));

        fs::write(out_dir.join(OPERATOR_STRUCTURED_OUTPUTS_FILE), "{not json").unwrap();
        let error =
            read_local_structured_outputs(&out_dir, &run_dir.to_string_lossy()).unwrap_err();
        assert_eq!(error.kind, "output_validation_failed");
        assert_eq!(error.field.as_deref(), Some("structuredOutputs"));
        assert!(error.message.contains("parse structured output manifest"));
    }

    #[test]
    fn validates_structured_outputs_against_manifest_fields() {
        let tmp = TempDir::new().unwrap();
        let run_dir = tmp.path().join(".omiga/runs/oprun_structured_schema");
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "structured_report".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec {
                outputs: BTreeMap::from([
                    (
                        "report".to_string(),
                        OperatorFieldSpec {
                            kind: OperatorFieldKind::File,
                            required: true,
                            glob: Some("report.txt".to_string()),
                            ..OperatorFieldSpec::default()
                        },
                    ),
                    (
                        "summary".to_string(),
                        OperatorFieldSpec {
                            kind: OperatorFieldKind::Json,
                            required: true,
                            ..OperatorFieldSpec::default()
                        },
                    ),
                    (
                        "passed".to_string(),
                        OperatorFieldSpec {
                            kind: OperatorFieldKind::Boolean,
                            required: true,
                            ..OperatorFieldSpec::default()
                        },
                    ),
                    (
                        "score".to_string(),
                        OperatorFieldSpec {
                            kind: OperatorFieldKind::Number,
                            minimum: Some(0.0),
                            maximum: Some(1.0),
                            ..OperatorFieldSpec::default()
                        },
                    ),
                ]),
                ..OperatorInterfaceSpec::default()
            },
            operations: BTreeMap::new(),
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["true".to_string()],
            },
            preflight: None,
            runtime: None,
            cache: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "test@local".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };

        let valid = validate_structured_outputs_against_manifest(
            Some(json!({
                "summary": { "lineCount": 2 },
                "passed": true,
                "score": 0.75,
                "extra": "allowed metadata"
            })),
            &spec,
            &run_dir.to_string_lossy(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(valid["summary"]["lineCount"], json!(2));

        let error = validate_structured_outputs_against_manifest(
            Some(json!({ "summary": { "lineCount": 2 }, "passed": "yes" })),
            &spec,
            &run_dir.to_string_lossy(),
        )
        .unwrap_err();
        assert_eq!(error.kind, "output_validation_failed");
        assert_eq!(error.field.as_deref(), Some("structuredOutputs.passed"));

        let error = validate_structured_outputs_against_manifest(
            Some(json!({ "passed": true })),
            &spec,
            &run_dir.to_string_lossy(),
        )
        .unwrap_err();
        assert_eq!(error.kind, "output_validation_failed");
        assert_eq!(error.field.as_deref(), Some("structuredOutputs.summary"));

        let error =
            validate_structured_outputs_against_manifest(None, &spec, &run_dir.to_string_lossy())
                .unwrap_err();
        assert_eq!(error.kind, "output_validation_failed");
        assert_eq!(error.field.as_deref(), Some("structuredOutputs.passed"));
    }

    #[test]
    fn rejects_output_globs_that_escape_session_outdir() {
        let tmp = TempDir::new().unwrap();
        let run_dir = tmp.path().join(".omiga/runs/oprun_escape");
        let out_dir = run_dir.join("out");
        fs::create_dir_all(&out_dir).unwrap();

        for glob in ["../*.txt", "/tmp/*.txt"] {
            let spec = OperatorSpec {
                api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
                kind: OPERATOR_KIND.to_string(),
                metadata: OperatorMetadata {
                    id: "bounded_outputs".to_string(),
                    version: "1".to_string(),
                    name: None,
                    description: None,
                    tags: Vec::new(),
                },
                interface: OperatorInterfaceSpec {
                    outputs: BTreeMap::from([(
                        "report".to_string(),
                        OperatorFieldSpec {
                            kind: OperatorFieldKind::File,
                            required: true,
                            glob: Some(glob.to_string()),
                            ..OperatorFieldSpec::default()
                        },
                    )]),
                    ..OperatorInterfaceSpec::default()
                },
                operations: BTreeMap::new(),
                smoke_tests: Vec::new(),
                execution: OperatorExecutionSpec {
                    argv: vec!["true".to_string()],
                },
                preflight: None,
                runtime: None,
                cache: None,
                resources: BTreeMap::new(),
                bindings: Vec::new(),
                permissions: None,
                source: OperatorSource {
                    source_plugin: "test@local".to_string(),
                    plugin_root: tmp.path().to_path_buf(),
                    manifest_path: tmp.path().join("operator.yaml"),
                },
            };

            let error =
                collect_local_outputs(&spec, &run_dir.to_string_lossy(), &out_dir).unwrap_err();
            assert_eq!(error.kind, "output_validation_failed");
            assert_eq!(error.field.as_deref(), Some("outputs.report"));
            assert!(error.message.contains("must stay relative"));
        }
    }
}
