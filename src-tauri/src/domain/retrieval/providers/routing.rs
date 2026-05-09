use super::builtin::BuiltinProvider;
use super::plugin_provider::PluginRetrievalProvider;
use crate::domain::plugins::{enabled_plugin_retrieval_plugins, PluginRetrievalRegistration};
use crate::domain::retrieval::registry::RetrievalRouteRegistry;
use crate::domain::retrieval::{
    RetrievalError, RetrievalOperation, RetrievalProvider, RetrievalProviderKind,
    RetrievalProviderOutput, RetrievalRequest,
};
use crate::domain::tools::ToolContext;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct RoutedRetrievalProvider {
    registrations: Vec<PluginRetrievalRegistration>,
    builtin: BuiltinProvider,
    plugin: PluginRetrievalProvider,
}

impl RoutedRetrievalProvider {
    pub fn new(registrations: Vec<PluginRetrievalRegistration>) -> Self {
        Self {
            plugin: PluginRetrievalProvider::new(registrations.clone()),
            registrations,
            builtin: BuiltinProvider,
        }
    }

    pub fn from_enabled_plugins() -> Self {
        Self::new(enabled_plugin_retrieval_plugins())
    }
}

impl Default for RoutedRetrievalProvider {
    fn default() -> Self {
        Self::from_enabled_plugins()
    }
}

#[async_trait]
impl RetrievalProvider for RoutedRetrievalProvider {
    async fn execute(
        &self,
        ctx: &ToolContext,
        request: RetrievalRequest,
    ) -> Result<RetrievalProviderOutput, RetrievalError> {
        if self.should_use_plugin(ctx, &request)? {
            self.plugin.execute(ctx, request).await
        } else {
            self.builtin.execute(ctx, request).await
        }
    }
}

impl RoutedRetrievalProvider {
    fn should_use_plugin(
        &self,
        ctx: &ToolContext,
        request: &RetrievalRequest,
    ) -> Result<bool, RetrievalError> {
        if request.source == "auto" || self.registrations.is_empty() {
            return Ok(false);
        }
        let explicit_plugin_source = has_plugin_source(&self.registrations, request);
        let registry = RetrievalRouteRegistry::new(self.registrations.clone());
        match registry.resolve_request(request, &ctx.web_search_api_keys) {
            Ok(route) => Ok(route.provider == RetrievalProviderKind::Plugin),
            Err(error) if explicit_plugin_source => Err(error),
            Err(_) => Ok(false),
        }
    }
}

pub(crate) fn has_plugin_source(
    registrations: &[PluginRetrievalRegistration],
    request: &RetrievalRequest,
) -> bool {
    registrations.iter().any(|registration| {
        registration.retrieval.sources.iter().any(|source| {
            source.category == request.category
                && source.id == request.source
                && source
                    .capabilities
                    .iter()
                    .any(|capability| capability_supports_operation(capability, request.operation))
        })
    })
}

fn capability_supports_operation(capability: &str, operation: RetrievalOperation) -> bool {
    match operation {
        RetrievalOperation::Search => capability == "search",
        RetrievalOperation::Fetch | RetrievalOperation::Resolve => capability == "fetch",
        RetrievalOperation::Query | RetrievalOperation::DownloadSummary => capability == "query",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::plugin_runtime::retrieval::manifest::{
        PluginRetrievalManifest, PluginRetrievalRuntime, PluginRetrievalSource,
    };
    use crate::domain::retrieval::types::RetrievalTool;
    use crate::domain::tools::WebSearchApiKeys;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn request(source: &str) -> RetrievalRequest {
        RetrievalRequest {
            request_id: "req".to_string(),
            tool: RetrievalTool::Search,
            operation: RetrievalOperation::Search,
            category: "dataset".to_string(),
            source: source.to_string(),
            subcategory: None,
            query: Some("q".to_string()),
            id: None,
            url: None,
            result: None,
            params: None,
            max_results: Some(5),
            prompt: None,
            web: None,
        }
    }

    fn registration(source_id: &str) -> PluginRetrievalRegistration {
        PluginRetrievalRegistration {
            plugin_id: "mock".to_string(),
            plugin_root: PathBuf::from("/tmp/mock"),
            retrieval: PluginRetrievalManifest {
                protocol_version: 1,
                runtime: PluginRetrievalRuntime {
                    command: PathBuf::from("/tmp/mock/plugin"),
                    args: vec![],
                    env: HashMap::new(),
                    cwd: PathBuf::from("/tmp/mock"),
                    idle_ttl_ms: None,
                    request_timeout_ms: None,
                    cancel_grace_ms: None,
                    concurrency: 1,
                },
                sources: vec![PluginRetrievalSource {
                    id: source_id.to_string(),
                    category: "dataset".to_string(),
                    label: source_id.to_string(),
                    description: String::new(),
                    aliases: vec![],
                    subcategories: vec![],
                    capabilities: vec!["search".to_string()],
                    required_credential_refs: vec![],
                    optional_credential_refs: vec![],
                    risk_level: "low".to_string(),
                    risk_notes: vec![],
                    default_enabled: false,
                    replaces_builtin: false,
                    parameters: vec![],
                }],
            },
        }
    }

    fn enabled_ctx(source: &str) -> ToolContext {
        let mut enabled = HashMap::new();
        enabled.insert("dataset".to_string(), vec![source.to_string()]);
        ToolContext::new("/tmp").with_web_search_api_keys(WebSearchApiKeys {
            enabled_sources_by_category: Some(enabled),
            ..WebSearchApiKeys::default()
        })
    }

    #[test]
    fn routes_enabled_plugin_source_to_plugin() {
        let provider = RoutedRetrievalProvider::new(vec![registration("mock_source")]);

        assert!(provider
            .should_use_plugin(&enabled_ctx("mock_source"), &request("mock_source"))
            .unwrap());
    }

    #[test]
    fn keeps_builtin_source_on_builtin_path() {
        let provider = RoutedRetrievalProvider::new(vec![registration("mock_source")]);

        assert!(!provider
            .should_use_plugin(&ToolContext::new("/tmp"), &request("geo"))
            .unwrap());
    }

    #[test]
    fn disabled_plugin_source_returns_routing_error() {
        let provider = RoutedRetrievalProvider::new(vec![registration("mock_source")]);
        let err = provider
            .should_use_plugin(&ToolContext::new("/tmp"), &request("mock_source"))
            .unwrap_err();

        assert!(matches!(err, RetrievalError::SourceDisabled { .. }));
    }
}
