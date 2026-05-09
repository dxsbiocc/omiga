use super::manifest::{
    load_plugin_retrieval_manifest, PluginRetrievalManifest, PluginRetrievalSource,
};
use super::process::PluginProcess;
use crate::domain::retrieval::types::{RetrievalOperation, RetrievalRequest, RetrievalTool};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

pub const RETRIEVAL_PLUGIN_PROTOCOL_DOC_PATH: &str = "docs/retrieval-plugin-protocol.md";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginValidationCheckStatus {
    Passed,
    Warning,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginValidationCheck {
    pub code: String,
    pub status: PluginValidationCheckStatus,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRetrievalValidationSourceSummary {
    pub category: String,
    pub source_id: String,
    pub label: String,
    pub capabilities: Vec<String>,
    pub required_credential_refs: Vec<String>,
    pub optional_credential_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRetrievalValidationSummary {
    pub protocol_version: u32,
    pub runtime_command: String,
    pub runtime_cwd: String,
    pub source_count: usize,
    pub sources: Vec<PluginRetrievalValidationSourceSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRetrievalSmokeResult {
    pub category: String,
    pub source_id: String,
    pub operation: String,
    pub status: PluginValidationCheckStatus,
    pub message: String,
    pub item_count: usize,
    pub has_detail: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRetrievalValidationReport {
    pub valid: bool,
    pub smoke_requested: bool,
    pub plugin_root: String,
    pub manifest_path: Option<String>,
    pub plugin_name: Option<String>,
    pub protocol_doc_path: String,
    pub retrieval: Option<PluginRetrievalValidationSummary>,
    pub checks: Vec<PluginValidationCheck>,
    pub smoke_results: Vec<PluginRetrievalSmokeResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidationPluginJson {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    retrieval: Option<JsonValue>,
}

pub async fn validate_retrieval_plugin_root(
    plugin_root: &Path,
    smoke: bool,
) -> PluginRetrievalValidationReport {
    let root = plugin_root
        .canonicalize()
        .unwrap_or_else(|_| plugin_root.to_path_buf());
    let mut checks = Vec::new();
    let mut smoke_results = Vec::new();

    checks.push(if root.is_dir() {
        passed(
            "plugin_root",
            format!("plugin root exists: {}", root.display()),
        )
    } else {
        failed(
            "plugin_root",
            format!("plugin root is not a directory: {}", root.display()),
        )
    });

    let manifest_path = crate::domain::plugins::plugin_manifest_path(&root);
    let Some(manifest_path) = manifest_path else {
        checks.push(failed(
            "manifest_found",
            "missing plugin.json, .omiga-plugin/plugin.json, or .codex-plugin/plugin.json"
                .to_string(),
        ));
        return report(root, None, None, smoke, None, checks, smoke_results);
    };
    checks.push(passed(
        "manifest_found",
        format!("found plugin manifest: {}", manifest_path.display()),
    ));

    let raw = match fs::read_to_string(&manifest_path) {
        Ok(raw) => raw,
        Err(err) => {
            checks.push(failed(
                "manifest_read",
                format!("read plugin manifest: {err}"),
            ));
            return report(
                root,
                Some(manifest_path),
                None,
                smoke,
                None,
                checks,
                smoke_results,
            );
        }
    };
    checks.push(passed("manifest_read", "plugin manifest is readable"));

    let parsed: ValidationPluginJson = match serde_json::from_str(&raw) {
        Ok(parsed) => parsed,
        Err(err) => {
            checks.push(failed(
                "manifest_json",
                format!("parse plugin manifest JSON: {err}"),
            ));
            return report(
                root,
                Some(manifest_path),
                None,
                smoke,
                None,
                checks,
                smoke_results,
            );
        }
    };
    checks.push(passed("manifest_json", "plugin manifest JSON is valid"));

    let plugin_name = parsed
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            root.file_name()
                .and_then(|value| value.to_str())
                .map(ToOwned::to_owned)
        });

    let Some(raw_retrieval) = parsed.retrieval else {
        checks.push(failed(
            "retrieval_manifest_found",
            "plugin manifest does not declare a retrieval section".to_string(),
        ));
        return report(
            root,
            Some(manifest_path),
            plugin_name,
            smoke,
            None,
            checks,
            smoke_results,
        );
    };
    checks.push(passed(
        "retrieval_manifest_found",
        "plugin manifest declares a retrieval section",
    ));

    let manifest = match load_plugin_retrieval_manifest(&root, raw_retrieval) {
        Ok(manifest) => manifest,
        Err(err) => {
            checks.push(failed(
                "retrieval_manifest_valid",
                format!("validate retrieval manifest: {err}"),
            ));
            return report(
                root,
                Some(manifest_path),
                plugin_name,
                smoke,
                None,
                checks,
                smoke_results,
            );
        }
    };
    checks.push(passed(
        "retrieval_manifest_valid",
        "retrieval manifest is valid for protocol version 1",
    ));

    let retrieval = Some(summary_for_manifest(&manifest));

    if smoke {
        let smoke_plugin_id = plugin_name.as_deref().unwrap_or("local-retrieval-plugin");
        smoke_results = smoke_manifest(smoke_plugin_id, manifest).await;
        if smoke_results
            .iter()
            .any(|result| matches!(result.status, PluginValidationCheckStatus::Failed))
        {
            checks.push(failed(
                "retrieval_smoke",
                "one or more retrieval smoke operations failed".to_string(),
            ));
        } else if smoke_results
            .iter()
            .any(|result| matches!(result.status, PluginValidationCheckStatus::Passed))
        {
            checks.push(passed(
                "retrieval_smoke",
                "retrieval smoke operations passed",
            ));
        } else {
            checks.push(skipped(
                "retrieval_smoke",
                "no credential-free search/query/fetch smoke operation was available".to_string(),
            ));
        }
    }

    report(
        root,
        Some(manifest_path),
        plugin_name,
        smoke,
        retrieval,
        checks,
        smoke_results,
    )
}

fn report(
    root: PathBuf,
    manifest_path: Option<PathBuf>,
    plugin_name: Option<String>,
    smoke_requested: bool,
    retrieval: Option<PluginRetrievalValidationSummary>,
    checks: Vec<PluginValidationCheck>,
    smoke_results: Vec<PluginRetrievalSmokeResult>,
) -> PluginRetrievalValidationReport {
    let valid = checks
        .iter()
        .all(|check| !matches!(check.status, PluginValidationCheckStatus::Failed))
        && smoke_results
            .iter()
            .all(|result| !matches!(result.status, PluginValidationCheckStatus::Failed));
    PluginRetrievalValidationReport {
        valid,
        smoke_requested,
        plugin_root: root.to_string_lossy().into_owned(),
        manifest_path: manifest_path.map(|path| path.to_string_lossy().into_owned()),
        plugin_name,
        protocol_doc_path: RETRIEVAL_PLUGIN_PROTOCOL_DOC_PATH.to_string(),
        retrieval,
        checks,
        smoke_results,
    }
}

fn summary_for_manifest(manifest: &PluginRetrievalManifest) -> PluginRetrievalValidationSummary {
    PluginRetrievalValidationSummary {
        protocol_version: manifest.protocol_version,
        runtime_command: manifest.runtime.command.to_string_lossy().into_owned(),
        runtime_cwd: manifest.runtime.cwd.to_string_lossy().into_owned(),
        source_count: manifest.sources.len(),
        sources: manifest
            .sources
            .iter()
            .map(|source| PluginRetrievalValidationSourceSummary {
                category: source.category.clone(),
                source_id: source.id.clone(),
                label: source.label.clone(),
                capabilities: source.capabilities.clone(),
                required_credential_refs: source.required_credential_refs.clone(),
                optional_credential_refs: source.optional_credential_refs.clone(),
            })
            .collect(),
    }
}

async fn smoke_manifest(
    plugin_id: &str,
    manifest: PluginRetrievalManifest,
) -> Vec<PluginRetrievalSmokeResult> {
    let operations = smoke_operations(&manifest.sources);
    if operations.is_empty() {
        return manifest
            .sources
            .iter()
            .filter(|source| !source.required_credential_refs.is_empty())
            .map(|source| PluginRetrievalSmokeResult {
                category: source.category.clone(),
                source_id: source.id.clone(),
                operation: "initialize".to_string(),
                status: PluginValidationCheckStatus::Skipped,
                message: format!(
                    "source requires credentials: {}",
                    source.required_credential_refs.join(", ")
                ),
                item_count: 0,
                has_detail: false,
            })
            .collect();
    }

    let cancel = CancellationToken::new();
    let mut process =
        match PluginProcess::start_with_cancel(plugin_id.to_string(), manifest, &cancel).await {
            Ok(process) => process,
            Err(err) => {
                return operations
                    .into_iter()
                    .map(|(source, operation)| PluginRetrievalSmokeResult {
                        category: source.category,
                        source_id: source.id,
                        operation: operation.as_str().to_string(),
                        status: PluginValidationCheckStatus::Failed,
                        message: format!("initialize plugin process: {err}"),
                        item_count: 0,
                        has_detail: false,
                    })
                    .collect();
            }
        };

    let mut results = Vec::new();
    for (source, operation) in operations {
        let request = smoke_request(&source, operation);
        match process
            .execute_with_cancel(&request, HashMap::new(), &cancel)
            .await
        {
            Ok(response) => results.push(PluginRetrievalSmokeResult {
                category: source.category,
                source_id: source.id,
                operation: operation.as_str().to_string(),
                status: PluginValidationCheckStatus::Passed,
                message: "smoke operation returned a valid retrieval response".to_string(),
                item_count: response.items.len(),
                has_detail: response.detail.is_some(),
            }),
            Err(err) => results.push(PluginRetrievalSmokeResult {
                category: source.category,
                source_id: source.id,
                operation: operation.as_str().to_string(),
                status: PluginValidationCheckStatus::Failed,
                message: err.to_string(),
                item_count: 0,
                has_detail: false,
            }),
        }
    }
    process.shutdown().await;
    results
}

fn smoke_operations(
    sources: &[PluginRetrievalSource],
) -> Vec<(PluginRetrievalSource, RetrievalOperation)> {
    sources
        .iter()
        .filter(|source| source.required_credential_refs.is_empty())
        .flat_map(|source| {
            source.capabilities.iter().filter_map(move |capability| {
                let operation = match capability.as_str() {
                    "search" => RetrievalOperation::Search,
                    "query" => RetrievalOperation::Query,
                    "fetch" => RetrievalOperation::Fetch,
                    _ => return None,
                };
                Some((source.clone(), operation))
            })
        })
        .collect()
}

fn smoke_request(
    source: &PluginRetrievalSource,
    operation: RetrievalOperation,
) -> RetrievalRequest {
    RetrievalRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        tool: match operation {
            RetrievalOperation::Fetch => RetrievalTool::Fetch,
            RetrievalOperation::Query => RetrievalTool::Query,
            _ => RetrievalTool::Search,
        },
        operation,
        category: source.category.clone(),
        source: source.id.clone(),
        subcategory: None,
        query: matches!(
            operation,
            RetrievalOperation::Search | RetrievalOperation::Query
        )
        .then(|| "omiga retrieval plugin validation".to_string()),
        id: matches!(operation, RetrievalOperation::Fetch)
            .then(|| "omiga-validation-id".to_string()),
        url: None,
        result: None,
        params: Some(serde_json::json!({"omigaValidation": true})),
        max_results: Some(1),
        prompt: matches!(operation, RetrievalOperation::Fetch)
            .then(|| "Omiga retrieval plugin validation fetch smoke call".to_string()),
        web: None,
    }
}

fn passed(code: impl Into<String>, message: impl Into<String>) -> PluginValidationCheck {
    PluginValidationCheck {
        code: code.into(),
        status: PluginValidationCheckStatus::Passed,
        message: message.into(),
    }
}

fn failed(code: impl Into<String>, message: impl Into<String>) -> PluginValidationCheck {
    PluginValidationCheck {
        code: code.into(),
        status: PluginValidationCheckStatus::Failed,
        message: message.into(),
    }
}

fn skipped(code: impl Into<String>, message: impl Into<String>) -> PluginValidationCheck {
    PluginValidationCheck {
        code: code.into(),
        status: PluginValidationCheckStatus::Skipped,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/retrieval-plugins/basic")
    }

    fn bundled_retrieval_plugin_root(plugin_name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bundled_plugins/plugins")
            .join(plugin_name)
    }

    fn bundled_retrieval_plugin_cases() -> Vec<(&'static str, Vec<&'static str>)> {
        vec![
            ("retrieval-dataset-geo", vec!["dataset.geo"]),
            (
                "retrieval-dataset-ena",
                vec![
                    "dataset.ena",
                    "dataset.ena_run",
                    "dataset.ena_experiment",
                    "dataset.ena_sample",
                    "dataset.ena_analysis",
                    "dataset.ena_assembly",
                    "dataset.ena_sequence",
                ],
            ),
            ("retrieval-dataset-biosample", vec!["dataset.biosample"]),
            (
                "retrieval-dataset-arrayexpress",
                vec!["dataset.arrayexpress"],
            ),
            (
                "retrieval-dataset-ncbi-datasets",
                vec!["dataset.ncbi_datasets"],
            ),
            ("retrieval-dataset-gtex", vec!["dataset.gtex"]),
            ("retrieval-dataset-cbioportal", vec!["dataset.cbioportal"]),
            ("retrieval-literature-pubmed", vec!["literature.pubmed"]),
            (
                "retrieval-literature-semantic-scholar",
                vec!["literature.semantic_scholar"],
            ),
            ("retrieval-knowledge-ncbi-gene", vec!["knowledge.ncbi_gene"]),
            ("retrieval-knowledge-ensembl", vec!["knowledge.ensembl"]),
            ("retrieval-knowledge-uniprot", vec!["knowledge.uniprot"]),
        ]
    }

    #[tokio::test]
    async fn validates_documented_fixture_without_smoke() {
        let report = validate_retrieval_plugin_root(&fixture_root(), false).await;

        assert!(report.valid);
        assert!(!report.smoke_requested);
        assert_eq!(report.protocol_doc_path, RETRIEVAL_PLUGIN_PROTOCOL_DOC_PATH);
        assert_eq!(
            report.plugin_name.as_deref(),
            Some("retrieval-protocol-example")
        );
        assert_eq!(
            report.retrieval.as_ref().map(|item| item.source_count),
            Some(1)
        );
        assert!(report.smoke_results.is_empty());
        assert!(report
            .checks
            .iter()
            .any(|check| check.code == "retrieval_manifest_valid"));
    }

    #[tokio::test]
    async fn validates_documented_fixture_with_search_query_fetch_smoke() {
        let report = validate_retrieval_plugin_root(&fixture_root(), true).await;

        assert!(report.valid, "report: {report:?}");
        assert!(report.smoke_requested);
        let operations = report
            .smoke_results
            .iter()
            .map(|result| result.operation.as_str())
            .collect::<Vec<_>>();
        assert_eq!(operations, vec!["search", "query", "fetch"]);
        assert!(report
            .smoke_results
            .iter()
            .all(|result| matches!(result.status, PluginValidationCheckStatus::Passed)));
        assert!(report
            .checks
            .iter()
            .any(|check| check.code == "retrieval_smoke"));
    }

    #[tokio::test]
    async fn reports_missing_manifest_as_invalid_without_erroring() {
        let dir = tempfile::tempdir().unwrap();
        let report = validate_retrieval_plugin_root(dir.path(), true).await;

        assert!(!report.valid);
        assert!(report.manifest_path.is_none());
        assert!(report
            .checks
            .iter()
            .any(|check| check.code == "manifest_found"
                && matches!(check.status, PluginValidationCheckStatus::Failed)));
    }

    #[tokio::test]
    async fn validates_bundled_individual_retrieval_source_plugins_with_offline_smoke() {
        for (plugin_name, expected_routes) in bundled_retrieval_plugin_cases() {
            let report =
                validate_retrieval_plugin_root(&bundled_retrieval_plugin_root(plugin_name), true)
                    .await;

            assert!(report.valid, "{plugin_name} report: {report:?}");
            assert_eq!(report.plugin_name.as_deref(), Some(plugin_name));
            let retrieval = report.retrieval.as_ref().expect("retrieval summary");
            assert_eq!(retrieval.source_count, expected_routes.len());
            let routes = retrieval
                .sources
                .iter()
                .map(|source| format!("{}.{}", source.category, source.source_id))
                .collect::<Vec<_>>();
            assert_eq!(routes, expected_routes);
            let smoke = report
                .smoke_results
                .iter()
                .map(|result| {
                    format!(
                        "{}.{}:{}",
                        result.category, result.source_id, result.operation
                    )
                })
                .collect::<Vec<_>>();
            let expected_smoke = expected_routes
                .iter()
                .flat_map(|route| {
                    ["search", "query", "fetch"].map(|operation| format!("{route}:{operation}"))
                })
                .collect::<Vec<_>>();
            assert_eq!(smoke, expected_smoke);
            assert!(report
                .smoke_results
                .iter()
                .all(|result| matches!(result.status, PluginValidationCheckStatus::Passed)));
        }
    }
}
