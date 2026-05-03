use crate::domain::plugins::{enabled_plugin_retrieval_plugins, PluginRetrievalRegistration};
use crate::domain::retrieval::credentials::project_credentials;
use crate::domain::retrieval::plugin::lifecycle::{
    PluginLifecycleKey, PluginLifecyclePolicy, PluginLifecycleState,
};
use crate::domain::retrieval::plugin::process::PluginProcess;
use crate::domain::retrieval::registry::RetrievalRouteRegistry;
use crate::domain::retrieval::{
    RetrievalError, RetrievalProvider, RetrievalProviderKind, RetrievalProviderOutput,
    RetrievalRequest,
};
use crate::domain::tools::ToolContext;
use async_trait::async_trait;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, OnceLock};
use tokio::time::Instant;

#[derive(Debug, Clone)]
pub struct PluginRetrievalProvider {
    registrations: Vec<PluginRetrievalRegistration>,
    lifecycle: PluginLifecycleState,
    process_pool: PluginProcessPool,
}

impl PluginRetrievalProvider {
    pub fn new(registrations: Vec<PluginRetrievalRegistration>) -> Self {
        Self {
            registrations,
            lifecycle: PluginLifecycleState::global(),
            process_pool: PluginProcessPool::global(),
        }
    }

    pub fn new_with_lifecycle_state(
        registrations: Vec<PluginRetrievalRegistration>,
        lifecycle: PluginLifecycleState,
    ) -> Self {
        Self {
            registrations,
            lifecycle,
            process_pool: PluginProcessPool::default(),
        }
    }

    pub fn from_enabled_plugins() -> Self {
        Self::new(enabled_plugin_retrieval_plugins())
    }

    pub fn registrations(&self) -> &[PluginRetrievalRegistration] {
        &self.registrations
    }
}

impl Default for PluginRetrievalProvider {
    fn default() -> Self {
        Self::from_enabled_plugins()
    }
}

pub type PluginProcessProvider = PluginRetrievalProvider;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginProcessPoolRouteStatus {
    pub plugin_id: String,
    pub category: String,
    pub source_id: String,
    pub route: String,
    pub plugin_root: String,
    pub remaining_ms: u64,
}

pub async fn global_plugin_process_pool_statuses() -> Vec<PluginProcessPoolRouteStatus> {
    PluginProcessPool::global().statuses(Instant::now()).await
}

pub async fn clear_global_plugin_process_pool() -> usize {
    PluginProcessPool::global().clear().await
}

#[async_trait]
impl RetrievalProvider for PluginRetrievalProvider {
    async fn execute(
        &self,
        ctx: &ToolContext,
        mut request: RetrievalRequest,
    ) -> Result<RetrievalProviderOutput, RetrievalError> {
        let registry = RetrievalRouteRegistry::new(self.registrations.clone());
        let route = registry.resolve_request(&request, &ctx.web_search_api_keys)?;
        if route.provider != RetrievalProviderKind::Plugin {
            return Err(RetrievalError::ProviderUnavailable {
                message: format!(
                    "retrieval source {}.{} is handled by the built-in provider",
                    route.category, route.id
                ),
            });
        }
        let plugin_id =
            route
                .plugin_id
                .clone()
                .ok_or_else(|| RetrievalError::ProviderUnavailable {
                    message: format!(
                        "plugin route {}.{} did not include a plugin id",
                        route.category, route.id
                    ),
                })?;
        let registration = self
            .registrations
            .iter()
            .find(|registration| registration.plugin_id == plugin_id)
            .ok_or_else(|| RetrievalError::ProviderUnavailable {
                message: format!("plugin retrieval registration `{plugin_id}` is unavailable"),
            })?;
        let credentials = project_credentials(
            &ctx.web_search_api_keys,
            &route.category,
            &route.id,
            &route.required_credential_refs,
            &route.optional_credential_refs,
        )?;

        let lifecycle_key =
            PluginLifecycleKey::new(plugin_id.clone(), route.category.clone(), route.id.clone());
        if let Some(status) = self
            .lifecycle
            .quarantine_status(&lifecycle_key, Instant::now())
        {
            return Err(RetrievalError::ProviderUnavailable {
                message: quarantine_message(&lifecycle_key, &status),
            });
        }
        let lifecycle_policy = PluginLifecyclePolicy::from_runtime(&registration.retrieval.runtime);

        let route_category = route.category.clone();
        let route_id = route.id.clone();
        request.category = route_category.clone();
        request.source = route_id.clone();
        if ctx.cancel.is_cancelled() {
            return Err(RetrievalError::Cancelled);
        }
        let pool_key = PluginProcessPoolKey::new(
            plugin_id.clone(),
            route_category,
            route_id,
            registration.plugin_root.to_string_lossy().into_owned(),
        );
        let mut process = match self.process_pool.take(&pool_key, Instant::now()).await {
            Some(process) => process,
            None => match PluginProcess::start_with_cancel(
                plugin_id.clone(),
                registration.retrieval.clone(),
                &ctx.cancel,
            )
            .await
            {
                Ok(process) => process,
                Err(err) => {
                    record_plugin_failure(&self.lifecycle, lifecycle_key, &lifecycle_policy, &err);
                    return Err(err);
                }
            },
        };
        let result = process
            .execute_with_cancel(&request, credentials, &ctx.cancel)
            .await;
        match result {
            Ok(response) => {
                self.lifecycle.record_success(&lifecycle_key);
                self.process_pool
                    .put(pool_key, process, &lifecycle_policy, Instant::now())
                    .await;
                Ok(response.into())
            }
            Err(err) => {
                if !matches!(err, RetrievalError::Cancelled) {
                    process.shutdown().await;
                }
                record_plugin_failure(&self.lifecycle, lifecycle_key, &lifecycle_policy, &err);
                Err(err)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PluginProcessPoolKey {
    plugin_id: String,
    category: String,
    source_id: String,
    plugin_root: String,
}

impl PluginProcessPoolKey {
    fn new(
        plugin_id: impl Into<String>,
        category: impl Into<String>,
        source_id: impl Into<String>,
        plugin_root: impl Into<String>,
    ) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            category: category.into(),
            source_id: source_id.into(),
            plugin_root: plugin_root.into(),
        }
    }

    fn route_display(&self) -> String {
        format!(
            "{}.{} via {}",
            self.category, self.source_id, self.plugin_id
        )
    }
}

struct PooledPluginProcess {
    process: PluginProcess,
    expires_at: Instant,
}

#[derive(Clone, Default)]
struct PluginProcessPool {
    inner: Arc<tokio::sync::Mutex<HashMap<PluginProcessPoolKey, PooledPluginProcess>>>,
}

impl fmt::Debug for PluginProcessPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginProcessPool").finish_non_exhaustive()
    }
}

impl PluginProcessPool {
    fn global() -> Self {
        static GLOBAL: OnceLock<PluginProcessPool> = OnceLock::new();
        GLOBAL.get_or_init(PluginProcessPool::default).clone()
    }

    async fn take(&self, key: &PluginProcessPoolKey, now: Instant) -> Option<PluginProcess> {
        let maybe_entry = {
            let mut guard = self.inner.lock().await;
            guard.remove(key)
        };
        let entry = maybe_entry?;
        if entry.expires_at > now {
            Some(entry.process)
        } else {
            let mut process = entry.process;
            process.shutdown().await;
            None
        }
    }

    async fn put(
        &self,
        key: PluginProcessPoolKey,
        process: PluginProcess,
        policy: &PluginLifecyclePolicy,
        now: Instant,
    ) {
        let expires_at = now + policy.idle_ttl;
        let cleanup_key = key.clone();
        let previous = {
            let mut guard = self.inner.lock().await;
            guard.insert(
                key,
                PooledPluginProcess {
                    process,
                    expires_at,
                },
            )
        };
        if let Some(previous) = previous {
            let mut process = previous.process;
            process.shutdown().await;
        }
        let pool = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep_until(expires_at).await;
            pool.shutdown_if_expired(cleanup_key).await;
        });
    }

    async fn shutdown_if_expired(&self, key: PluginProcessPoolKey) {
        let maybe_entry = {
            let mut guard = self.inner.lock().await;
            match guard.get(&key) {
                Some(entry) if entry.expires_at <= Instant::now() => guard.remove(&key),
                _ => None,
            }
        };
        if let Some(entry) = maybe_entry {
            let mut process = entry.process;
            process.shutdown().await;
        }
    }

    async fn statuses(&self, now: Instant) -> Vec<PluginProcessPoolRouteStatus> {
        let (expired, mut statuses) = {
            let mut guard = self.inner.lock().await;
            let expired_keys = guard
                .iter()
                .filter_map(|(key, entry)| (entry.expires_at <= now).then_some(key.clone()))
                .collect::<Vec<_>>();
            let expired = expired_keys
                .into_iter()
                .filter_map(|key| guard.remove(&key))
                .collect::<Vec<_>>();
            let statuses = guard
                .iter()
                .map(|(key, entry)| PluginProcessPoolRouteStatus {
                    plugin_id: key.plugin_id.clone(),
                    category: key.category.clone(),
                    source_id: key.source_id.clone(),
                    route: key.route_display(),
                    plugin_root: key.plugin_root.clone(),
                    remaining_ms: duration_millis(entry.expires_at.saturating_duration_since(now)),
                })
                .collect::<Vec<_>>();
            (expired, statuses)
        };
        for entry in expired {
            let mut process = entry.process;
            process.shutdown().await;
        }
        statuses.sort_by(|left, right| {
            (
                &left.plugin_id,
                &left.category,
                &left.source_id,
                &left.plugin_root,
            )
                .cmp(&(
                    &right.plugin_id,
                    &right.category,
                    &right.source_id,
                    &right.plugin_root,
                ))
        });
        statuses
    }

    async fn clear(&self) -> usize {
        let entries = {
            let mut guard = self.inner.lock().await;
            std::mem::take(&mut *guard)
        };
        let count = entries.len();
        for (_, entry) in entries {
            let mut process = entry.process;
            process.shutdown().await;
        }
        count
    }
}

fn duration_millis(duration: std::time::Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

fn record_plugin_failure(
    lifecycle: &PluginLifecycleState,
    key: PluginLifecycleKey,
    policy: &PluginLifecyclePolicy,
    error: &RetrievalError,
) {
    if should_count_plugin_failure(error) {
        lifecycle.record_failure(key, policy, Instant::now(), error.to_string());
    }
}

fn should_count_plugin_failure(error: &RetrievalError) -> bool {
    matches!(
        error,
        RetrievalError::ProviderUnavailable { .. }
            | RetrievalError::Protocol { .. }
            | RetrievalError::ExecutionFailed { .. }
            | RetrievalError::Timeout { .. }
    )
}

fn quarantine_message(
    key: &PluginLifecycleKey,
    status: &crate::domain::retrieval::plugin::lifecycle::PluginQuarantineStatus,
) -> String {
    let seconds = status.remaining.as_secs().max(1);
    let last_error = status
        .last_error
        .as_deref()
        .map(|error| format!(" Last error: {error}"))
        .unwrap_or_default();
    format!(
        "retrieval plugin route {} is quarantined for {seconds}s after {} consecutive failures.{last_error}",
        key.display(),
        status.consecutive_failures
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::retrieval::normalize::normalize_id;
    use crate::domain::retrieval::plugin::manifest::load_plugin_retrieval_manifest;
    use crate::domain::retrieval::types::{RetrievalOperation, RetrievalTool};
    use crate::domain::tools::WebSearchApiKeys;
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;

    #[cfg(unix)]
    fn make_executable(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }

    fn request(source: &str) -> RetrievalRequest {
        request_for_operation(
            source,
            RetrievalTool::Search,
            RetrievalOperation::Search,
            "hello",
        )
    }

    fn request_for_operation(
        source: &str,
        tool: RetrievalTool,
        operation: RetrievalOperation,
        marker: &str,
    ) -> RetrievalRequest {
        request_for_category_operation("dataset", source, tool, operation, marker)
    }

    fn request_for_category_operation(
        category: &str,
        source: &str,
        tool: RetrievalTool,
        operation: RetrievalOperation,
        marker: &str,
    ) -> RetrievalRequest {
        RetrievalRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            tool,
            operation,
            category: category.to_string(),
            source: source.to_string(),
            subcategory: None,
            query: matches!(
                operation,
                RetrievalOperation::Search | RetrievalOperation::Query
            )
            .then(|| marker.to_string()),
            id: matches!(operation, RetrievalOperation::Fetch).then(|| "mock-1".to_string()),
            url: None,
            result: None,
            params: None,
            max_results: Some(5),
            prompt: matches!(operation, RetrievalOperation::Fetch)
                .then(|| format!("fetch prompt {marker}")),
            web: None,
        }
    }

    fn response_from_output(
        output: RetrievalProviderOutput,
    ) -> crate::domain::retrieval::types::RetrievalResponse {
        let RetrievalProviderOutput::Response(response) = output else {
            panic!("expected response output");
        };
        *response
    }

    fn keys_with_enabled(source: &str) -> WebSearchApiKeys {
        keys_with_enabled_in("dataset", source)
    }

    fn keys_with_enabled_in(category: &str, source: &str) -> WebSearchApiKeys {
        let mut map = HashMap::new();
        map.insert(category.to_string(), vec![normalize_id(source)]);
        WebSearchApiKeys {
            enabled_sources_by_category: Some(map),
            pubmed_email: Some("dev@example.test".to_string()),
            ..WebSearchApiKeys::default()
        }
    }

    fn mock_registration(required: bool) -> (tempfile::TempDir, PluginRetrievalRegistration) {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("mock_plugin.py");
        fs::write(&script, MOCK_PLUGIN).unwrap();
        fs::File::open(&script).unwrap().sync_all().unwrap();
        #[cfg(unix)]
        make_executable(&script);
        let credential_field = if required {
            json!({"requiredCredentialRefs": ["pubmed_email"]})
        } else {
            json!({"optionalCredentialRefs": ["pubmed_email"]})
        };
        let mut source = json!({
            "id": "mock_source",
            "category": "dataset",
            "capabilities": ["search", "fetch", "query"]
        });
        source
            .as_object_mut()
            .unwrap()
            .extend(credential_field.as_object().unwrap().clone());
        let manifest = load_plugin_retrieval_manifest(
            dir.path(),
            json!({
                "protocolVersion": 1,
                "runtime": {
                    "command": "./mock_plugin.py",
                    "requestTimeoutMs": 5_000,
                    "cancelGraceMs": 10,
                    "concurrency": 1
                },
                "sources": [source]
            }),
        )
        .unwrap();
        (
            dir,
            PluginRetrievalRegistration {
                plugin_id: "mock-plugin".to_string(),
                plugin_root: manifest.runtime.cwd.clone(),
                retrieval: manifest,
            },
        )
    }

    fn bundled_public_dataset_registration() -> PluginRetrievalRegistration {
        bundled_registration(
            "public-dataset-sources@omiga-curated",
            "public-dataset-sources",
        )
    }

    fn bundled_public_literature_registration() -> PluginRetrievalRegistration {
        bundled_registration(
            "public-literature-sources@omiga-curated",
            "public-literature-sources",
        )
    }

    fn bundled_registration(plugin_id: &str, plugin_dir: &str) -> PluginRetrievalRegistration {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bundled_plugins/plugins")
            .join(plugin_dir);
        let plugin_json: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join(".omiga-plugin/plugin.json")).unwrap(),
        )
        .unwrap();
        let manifest = load_plugin_retrieval_manifest(
            &root,
            plugin_json
                .get("retrieval")
                .cloned()
                .expect("retrieval manifest"),
        )
        .unwrap();
        PluginRetrievalRegistration {
            plugin_id: plugin_id.to_string(),
            plugin_root: root,
            retrieval: manifest,
        }
    }

    const MOCK_PLUGIN: &str = r#"#!/usr/bin/env python3
import json
import os
import sys
import time

for line in sys.stdin:
    msg = json.loads(line)
    msg_type = msg.get("type")
    if msg_type == "initialize":
        print(json.dumps({
            "id": msg["id"],
            "type": "initialized",
            "protocolVersion": 1,
            "sources": [{
                "category": "dataset",
                "id": "mock_source",
                "capabilities": ["search", "fetch", "query"]
            }]
        }), flush=True)
    elif msg_type == "execute":
        req = msg["request"]
        if req.get("query") == "slow":
            time.sleep(0.15)
        print(json.dumps({
            "id": msg["id"],
            "type": "result",
            "response": {
                "ok": True,
                "operation": req.get("operation", "search"),
                "category": req.get("category", "dataset"),
                "source": req.get("source", "mock_source"),
                "effectiveSource": req.get("source", "mock_source"),
                "items": [{
                    "id": "mock-1",
                    "title": "Provider Mock Result",
                    "metadata": {
                        "credential_keys": sorted(req.get("credentials", {}).keys()),
                        "operation": req.get("operation"),
                        "pid": os.getpid()
                    }
                }],
                "total": 1
            }
        }), flush=True)
    elif msg_type == "shutdown":
        print(json.dumps({"id": msg["id"], "type": "shutdown"}), flush=True)
        break
"#;

    #[tokio::test]
    async fn provider_executes_enabled_plugin_source() {
        let (_dir, registration) = mock_registration(false);
        let provider = PluginRetrievalProvider::new_with_lifecycle_state(
            vec![registration],
            PluginLifecycleState::default(),
        );
        let ctx =
            ToolContext::new("/tmp").with_web_search_api_keys(keys_with_enabled("mock_source"));

        let response = provider
            .execute(&ctx, request("mock_source"))
            .await
            .unwrap();
        let RetrievalProviderOutput::Response(response) = response else {
            panic!("expected response output");
        };

        assert_eq!(response.provider, RetrievalProviderKind::Plugin);
        assert_eq!(response.plugin.as_deref(), Some("mock-plugin"));
        assert_eq!(
            response.items[0].title.as_deref(),
            Some("Provider Mock Result")
        );
        assert_eq!(
            response.items[0].metadata["credential_keys"],
            json!(["pubmed_email"])
        );
    }

    #[tokio::test]
    async fn provider_executes_bundled_replacement_dataset_sources_when_enabled() {
        let provider = PluginRetrievalProvider::new_with_lifecycle_state(
            vec![bundled_public_dataset_registration()],
            PluginLifecycleState::default(),
        );
        for source in [
            "geo",
            "ena",
            "ena_run",
            "ena_experiment",
            "ena_sample",
            "ena_analysis",
            "ena_assembly",
            "ena_sequence",
            "biosample",
            "arrayexpress",
            "ncbi_datasets",
            "gtex",
            "cbioportal",
        ] {
            let ctx = ToolContext::new("/tmp").with_web_search_api_keys(keys_with_enabled(source));
            let mut request = request_for_operation(
                source,
                RetrievalTool::Search,
                RetrievalOperation::Search,
                "validation",
            );
            request.params = Some(json!({"omigaValidation": true}));

            let response = provider.execute(&ctx, request).await.unwrap();
            let response = response_from_output(response);

            assert_eq!(response.provider, RetrievalProviderKind::Plugin);
            assert_eq!(
                response.plugin.as_deref(),
                Some("public-dataset-sources@omiga-curated")
            );
            assert_eq!(response.source, source);
            assert_eq!(response.effective_source, source);
            assert_eq!(response.items[0].metadata["validation"], json!(true));
        }
    }

    #[tokio::test]
    async fn provider_executes_bundled_replacement_literature_sources_when_enabled() {
        let provider = PluginRetrievalProvider::new_with_lifecycle_state(
            vec![bundled_public_literature_registration()],
            PluginLifecycleState::default(),
        );
        for source in ["pubmed", "semantic_scholar"] {
            let ctx = ToolContext::new("/tmp")
                .with_web_search_api_keys(keys_with_enabled_in("literature", source));
            let mut request = request_for_category_operation(
                "literature",
                source,
                RetrievalTool::Search,
                RetrievalOperation::Search,
                "validation",
            );
            request.params = Some(json!({"omigaValidation": true}));

            let response = provider.execute(&ctx, request).await.unwrap();
            let response = response_from_output(response);

            assert_eq!(response.provider, RetrievalProviderKind::Plugin);
            assert_eq!(
                response.plugin.as_deref(),
                Some("public-literature-sources@omiga-curated")
            );
            assert_eq!(response.source, source);
            assert_eq!(response.effective_source, source);
            assert_eq!(response.items[0].metadata["validation"], json!(true));
        }
    }

    #[tokio::test]
    async fn provider_reuses_process_within_idle_ttl_after_success() {
        let (_dir, registration) = mock_registration(false);
        let provider = PluginRetrievalProvider::new_with_lifecycle_state(
            vec![registration],
            PluginLifecycleState::default(),
        );
        let ctx =
            ToolContext::new("/tmp").with_web_search_api_keys(keys_with_enabled("mock_source"));

        let first = provider
            .execute(&ctx, request("mock_source"))
            .await
            .unwrap();
        let second = provider
            .execute(&ctx, request("mock_source"))
            .await
            .unwrap();
        let RetrievalProviderOutput::Response(first) = first else {
            panic!("expected first response output");
        };
        let RetrievalProviderOutput::Response(second) = second else {
            panic!("expected second response output");
        };

        assert_eq!(
            first.items[0].metadata["pid"],
            second.items[0].metadata["pid"]
        );
    }

    #[tokio::test]
    async fn provider_reuses_process_across_repeated_search_query_and_fetch_calls() {
        let (_dir, registration) = mock_registration(false);
        let provider = PluginRetrievalProvider::new_with_lifecycle_state(
            vec![registration],
            PluginLifecycleState::default(),
        );
        let ctx =
            ToolContext::new("/tmp").with_web_search_api_keys(keys_with_enabled("mock_source"));
        let operations = [
            (RetrievalTool::Search, RetrievalOperation::Search),
            (RetrievalTool::Query, RetrievalOperation::Query),
            (RetrievalTool::Fetch, RetrievalOperation::Fetch),
        ];
        let mut pids = Vec::new();

        for round in 0..3 {
            for (tool, operation) in operations {
                let output = provider
                    .execute(
                        &ctx,
                        request_for_operation(
                            "mock_source",
                            tool,
                            operation,
                            &format!("round-{round}"),
                        ),
                    )
                    .await
                    .unwrap();
                let response = response_from_output(output);

                assert_eq!(response.provider, RetrievalProviderKind::Plugin);
                assert_eq!(response.plugin.as_deref(), Some("mock-plugin"));
                assert_eq!(response.operation, operation);
                assert_eq!(
                    response.items[0].metadata["operation"],
                    json!(operation.as_str())
                );
                pids.push(response.items[0].metadata["pid"].clone());
            }
        }

        assert!(pids.windows(2).all(|pair| pair[0] == pair[1]));
        let statuses = provider.process_pool.statuses(Instant::now()).await;
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].route, "dataset.mock_source via mock-plugin");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn provider_concurrent_calls_do_not_share_in_flight_process_and_leave_one_pooled_process()
    {
        let (_dir, registration) = mock_registration(false);
        let provider = PluginRetrievalProvider::new_with_lifecycle_state(
            vec![registration],
            PluginLifecycleState::default(),
        );
        let ctx =
            ToolContext::new("/tmp").with_web_search_api_keys(keys_with_enabled("mock_source"));
        let first_provider = provider.clone();
        let second_provider = provider.clone();
        let first_ctx = ctx.clone();
        let second_ctx = ctx.clone();

        let first = tokio::spawn(async move {
            first_provider
                .execute(
                    &first_ctx,
                    request_for_operation(
                        "mock_source",
                        RetrievalTool::Search,
                        RetrievalOperation::Search,
                        "slow",
                    ),
                )
                .await
                .map(response_from_output)
        });
        let second = tokio::spawn(async move {
            second_provider
                .execute(
                    &second_ctx,
                    request_for_operation(
                        "mock_source",
                        RetrievalTool::Search,
                        RetrievalOperation::Search,
                        "slow",
                    ),
                )
                .await
                .map(response_from_output)
        });

        let first = first.await.unwrap().unwrap();
        let second = second.await.unwrap().unwrap();
        let first_pid = first.items[0].metadata["pid"].clone();
        let second_pid = second.items[0].metadata["pid"].clone();

        assert_ne!(first_pid, second_pid);
        let statuses = provider.process_pool.statuses(Instant::now()).await;
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].route, "dataset.mock_source via mock-plugin");
    }

    #[tokio::test]
    async fn process_pool_expires_idle_processes() {
        let (_dir, registration) = mock_registration(false);
        let pool = PluginProcessPool::default();
        let key = PluginProcessPoolKey::new(
            "mock-plugin",
            "dataset",
            "mock_source",
            registration.plugin_root.to_string_lossy(),
        );
        let mut policy = PluginLifecyclePolicy::from_runtime(&registration.retrieval.runtime);
        policy.idle_ttl = std::time::Duration::from_millis(20);
        let process = PluginProcess::start("mock-plugin", registration.retrieval.clone())
            .await
            .unwrap();

        pool.put(key.clone(), process, &policy, Instant::now())
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;

        assert!(pool.take(&key, Instant::now()).await.is_none());
    }

    #[tokio::test]
    async fn process_pool_reports_and_clears_active_processes() {
        let (_dir, registration) = mock_registration(false);
        let pool = PluginProcessPool::default();
        let key = PluginProcessPoolKey::new(
            "mock-plugin",
            "dataset",
            "mock_source",
            registration.plugin_root.to_string_lossy(),
        );
        let mut policy = PluginLifecyclePolicy::from_runtime(&registration.retrieval.runtime);
        policy.idle_ttl = std::time::Duration::from_secs(30);
        let process = PluginProcess::start("mock-plugin", registration.retrieval.clone())
            .await
            .unwrap();

        pool.put(key, process, &policy, Instant::now()).await;
        let statuses = pool.statuses(Instant::now()).await;

        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].plugin_id, "mock-plugin");
        assert_eq!(statuses[0].category, "dataset");
        assert_eq!(statuses[0].source_id, "mock_source");
        assert_eq!(statuses[0].route, "dataset.mock_source via mock-plugin");
        assert!(statuses[0].remaining_ms > 0);

        assert_eq!(pool.clear().await, 1);
        assert!(pool.statuses(Instant::now()).await.is_empty());
    }

    #[tokio::test]
    async fn provider_reports_builtin_routes_as_unavailable() {
        let provider = PluginRetrievalProvider::new_with_lifecycle_state(
            vec![],
            PluginLifecycleState::default(),
        );
        let ctx = ToolContext::new("/tmp");
        let result = provider.execute(&ctx, request("geo")).await;
        let Err(err) = result else {
            panic!("expected provider unavailable error");
        };

        assert!(matches!(err, RetrievalError::ProviderUnavailable { .. }));
        assert!(err.to_string().contains("built-in provider"));
    }

    #[tokio::test]
    async fn provider_checks_declared_required_credentials() {
        let (_dir, registration) = mock_registration(true);
        let provider = PluginRetrievalProvider::new_with_lifecycle_state(
            vec![registration],
            PluginLifecycleState::default(),
        );
        let mut keys = keys_with_enabled("mock_source");
        keys.pubmed_email = None;
        let ctx = ToolContext::new("/tmp").with_web_search_api_keys(keys);

        let result = provider.execute(&ctx, request("mock_source")).await;
        let Err(err) = result else {
            panic!("expected missing credentials error");
        };

        assert!(matches!(err, RetrievalError::MissingCredentials { .. }));
    }

    fn failing_registration() -> (tempfile::TempDir, PluginRetrievalRegistration) {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("failing_plugin.py");
        fs::write(&script, FAILING_PLUGIN).unwrap();
        fs::File::open(&script).unwrap().sync_all().unwrap();
        #[cfg(unix)]
        make_executable(&script);
        let manifest = load_plugin_retrieval_manifest(
            dir.path(),
            json!({
                "protocolVersion": 1,
                "runtime": {
                    "command": "./failing_plugin.py",
                    "requestTimeoutMs": 5_000,
                    "concurrency": 1
                },
                "sources": [{
                    "id": "mock_source",
                    "category": "dataset",
                    "capabilities": ["search"]
                }]
            }),
        )
        .unwrap();
        (
            dir,
            PluginRetrievalRegistration {
                plugin_id: "failing-plugin".to_string(),
                plugin_root: manifest.runtime.cwd.clone(),
                retrieval: manifest,
            },
        )
    }

    const FAILING_PLUGIN: &str = r#"#!/usr/bin/env python3
import json
import sys

for line in sys.stdin:
    msg = json.loads(line)
    msg_type = msg.get("type")
    if msg_type == "initialize":
        print(json.dumps({
            "id": msg["id"],
            "type": "initialized",
            "protocolVersion": 1,
            "sources": [{
                "category": "dataset",
                "id": "mock_source",
                "capabilities": ["search"]
            }]
        }), flush=True)
    elif msg_type == "execute":
        print(json.dumps({
            "id": msg["id"],
            "type": "error",
            "error": {
                "code": "upstream_failed",
                "message": "mock upstream failed"
            }
        }), flush=True)
    elif msg_type == "shutdown":
        print(json.dumps({"id": msg["id"], "type": "shutdown"}), flush=True)
        break
"#;

    #[tokio::test]
    async fn provider_quarantines_plugin_after_repeated_runtime_failures() {
        let (_dir, registration) = failing_registration();
        let provider = PluginRetrievalProvider::new_with_lifecycle_state(
            vec![registration],
            PluginLifecycleState::default(),
        );
        let ctx =
            ToolContext::new("/tmp").with_web_search_api_keys(keys_with_enabled("mock_source"));

        for _ in 0..3 {
            let result = provider.execute(&ctx, request("mock_source")).await;
            let Err(err) = result else {
                panic!("expected plugin execution failure");
            };
            assert!(matches!(err, RetrievalError::ExecutionFailed { .. }));
        }

        let result = provider.execute(&ctx, request("mock_source")).await;
        let Err(err) = result else {
            panic!("expected quarantine error");
        };

        assert!(matches!(err, RetrievalError::ProviderUnavailable { .. }));
        assert!(err.to_string().contains("quarantined"));
        assert!(err.to_string().contains("mock upstream failed"));
    }

    fn slow_registration() -> (tempfile::TempDir, PluginRetrievalRegistration) {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("slow_plugin.py");
        fs::write(&script, SLOW_PLUGIN).unwrap();
        fs::File::open(&script).unwrap().sync_all().unwrap();
        #[cfg(unix)]
        make_executable(&script);
        let manifest = load_plugin_retrieval_manifest(
            dir.path(),
            json!({
                "protocolVersion": 1,
                "runtime": {
                    "command": "./slow_plugin.py",
                    "requestTimeoutMs": 5_000,
                    "cancelGraceMs": 10,
                    "concurrency": 1
                },
                "sources": [{
                    "id": "mock_source",
                    "category": "dataset",
                    "capabilities": ["search"]
                }]
            }),
        )
        .unwrap();
        (
            dir,
            PluginRetrievalRegistration {
                plugin_id: "slow-plugin".to_string(),
                plugin_root: manifest.runtime.cwd.clone(),
                retrieval: manifest,
            },
        )
    }

    const SLOW_PLUGIN: &str = r#"#!/usr/bin/env python3
import json
import sys
import time

for line in sys.stdin:
    msg = json.loads(line)
    msg_type = msg.get("type")
    if msg_type == "initialize":
        print(json.dumps({
            "id": msg["id"],
            "type": "initialized",
            "protocolVersion": 1,
            "sources": [{
                "category": "dataset",
                "id": "mock_source",
                "capabilities": ["search"]
            }]
        }), flush=True)
    elif msg_type == "execute":
        time.sleep(5)
    elif msg_type == "shutdown":
        print(json.dumps({"id": msg["id"], "type": "shutdown"}), flush=True)
        break
"#;

    #[tokio::test]
    async fn provider_propagates_cancel_token_without_quarantining_route() {
        let (_dir, registration) = slow_registration();
        let lifecycle = PluginLifecycleState::default();
        let provider = PluginRetrievalProvider::new_with_lifecycle_state(
            vec![registration],
            lifecycle.clone(),
        );
        let cancel = tokio_util::sync::CancellationToken::new();
        let ctx = ToolContext::new("/tmp")
            .with_web_search_api_keys(keys_with_enabled("mock_source"))
            .with_cancel_token(cancel.clone());
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            trigger.cancel();
        });

        let result = provider.execute(&ctx, request("mock_source")).await;
        let Err(err) = result else {
            panic!("expected cancellation error");
        };

        assert!(matches!(err, RetrievalError::Cancelled));
        let status = lifecycle.route_status(
            &PluginLifecycleKey::new("slow-plugin", "dataset", "mock_source"),
            Instant::now(),
        );
        assert_eq!(status.consecutive_failures, 0);
        assert!(!status.quarantined);
    }
}
