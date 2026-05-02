use crate::domain::plugins::{enabled_plugin_retrieval_plugins, PluginRetrievalRegistration};
use crate::domain::retrieval::credentials::project_credentials;
use crate::domain::retrieval::plugin::process::PluginProcess;
use crate::domain::retrieval::registry::RetrievalRouteRegistry;
use crate::domain::retrieval::{
    RetrievalError, RetrievalProvider, RetrievalProviderKind, RetrievalProviderOutput,
    RetrievalRequest,
};
use crate::domain::tools::ToolContext;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct PluginRetrievalProvider {
    registrations: Vec<PluginRetrievalRegistration>,
}

impl PluginRetrievalProvider {
    pub fn new(registrations: Vec<PluginRetrievalRegistration>) -> Self {
        Self { registrations }
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

        request.category = route.category;
        request.source = route.id;
        let mut process = PluginProcess::start(plugin_id, registration.retrieval.clone()).await?;
        let result = process.execute(&request, credentials).await;
        process.shutdown().await;
        result.map(Into::into)
    }
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
        RetrievalRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            tool: RetrievalTool::Search,
            operation: RetrievalOperation::Search,
            category: "dataset".to_string(),
            source: source.to_string(),
            subcategory: None,
            query: Some("hello".to_string()),
            id: None,
            url: None,
            result: None,
            params: None,
            max_results: Some(5),
            prompt: None,
            web: None,
        }
    }

    fn keys_with_enabled(source: &str) -> WebSearchApiKeys {
        let mut map = HashMap::new();
        map.insert("dataset".to_string(), vec![normalize_id(source)]);
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

    const MOCK_PLUGIN: &str = r#"#!/usr/bin/env python3
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
                "capabilities": ["search", "fetch", "query"]
            }]
        }), flush=True)
    elif msg_type == "execute":
        req = msg["request"]
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
                    "metadata": {"credential_keys": sorted(req.get("credentials", {}).keys())}
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
        let provider = PluginRetrievalProvider::new(vec![registration]);
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
    async fn provider_reports_builtin_routes_as_unavailable() {
        let provider = PluginRetrievalProvider::new(vec![]);
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
        let provider = PluginRetrievalProvider::new(vec![registration]);
        let mut keys = keys_with_enabled("mock_source");
        keys.pubmed_email = None;
        let ctx = ToolContext::new("/tmp").with_web_search_api_keys(keys);

        let result = provider.execute(&ctx, request("mock_source")).await;
        let Err(err) = result else {
            panic!("expected missing credentials error");
        };

        assert!(matches!(err, RetrievalError::MissingCredentials { .. }));
    }
}
