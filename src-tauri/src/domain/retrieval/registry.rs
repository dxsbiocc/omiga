//! Runtime retrieval registry overlay.
//!
//! Built-in source metadata remains in `domain::retrieval_registry`. This module
//! creates an owned routing view that can merge enabled plugin sources, enforce
//! collision rules, and choose the provider for a request.

use super::normalize::{normalize_id, normalized_category};
use super::types::{RetrievalError, RetrievalOperation, RetrievalProviderKind, RetrievalRequest};
use crate::domain::plugins::PluginRetrievalRegistration;
use crate::domain::retrieval_registry::{self, RetrievalCapability};
use crate::domain::tools::WebSearchApiKeys;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalRouteRegistry {
    entries: HashMap<SourceKey, RegistryEntry>,
    errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceKey {
    pub category: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RegistryEntry {
    Builtin(SourceRegistration),
    Plugin(SourceRegistration),
    BuiltinWithReplacement {
        builtin: SourceRegistration,
        replacement: SourceRegistration,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRegistration {
    pub category: String,
    pub id: String,
    pub label: String,
    pub capabilities: Vec<String>,
    pub provider: RetrievalProviderKind,
    pub plugin_id: Option<String>,
    pub plugin_root: Option<PathBuf>,
    pub replaces_builtin: bool,
    pub default_enabled: bool,
    pub required_credential_refs: Vec<String>,
    pub optional_credential_refs: Vec<String>,
}

impl RetrievalRouteRegistry {
    pub fn new(plugin_registrations: Vec<PluginRetrievalRegistration>) -> Self {
        let mut registry = Self::builtin_only();
        for plugin in plugin_registrations {
            registry.insert_plugin_registration(plugin);
        }
        registry
    }

    pub fn builtin_only() -> Self {
        let mut entries = HashMap::new();
        for source in retrieval_registry::registry().sources {
            let registration = SourceRegistration::from_builtin(source);
            entries.insert(registration.key(), RegistryEntry::Builtin(registration));
        }
        Self {
            entries,
            errors: Vec::new(),
        }
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    pub fn source_count(&self) -> usize {
        self.entries.len()
    }

    pub fn resolve_request(
        &self,
        request: &RetrievalRequest,
        keys: &WebSearchApiKeys,
    ) -> Result<SourceRegistration, RetrievalError> {
        let category = normalized_category(&request.category);
        let source = normalize_id(&request.source);
        if source == "auto" {
            return Err(RetrievalError::InvalidRequest {
                message: "retrieval registry cannot resolve source=auto yet".to_string(),
            });
        }
        let key = SourceKey {
            category: category.clone(),
            id: source.clone(),
        };
        let Some(entry) = self.entries.get(&key) else {
            return Err(RetrievalError::InvalidRequest {
                message: format!("Unsupported retrieval resource: {category}.{source}"),
            });
        };
        let selected = entry.select(keys);
        if !selected.supports_operation(request.operation) {
            return Err(RetrievalError::InvalidRequest {
                message: format!(
                    "retrieval resource `{}.{}` does not support {}",
                    selected.category,
                    selected.id,
                    request.operation.as_str()
                ),
            });
        }
        if !is_enabled(keys, &selected) {
            return Err(RetrievalError::SourceDisabled {
                category: selected.category.clone(),
                source_id: selected.id.clone(),
                message: format!(
                    "{}.{} is disabled. Enable it in Settings → Search.",
                    selected.category, selected.id
                ),
            });
        }
        Ok(selected)
    }

    fn insert_plugin_registration(&mut self, plugin: PluginRetrievalRegistration) {
        for source in plugin.retrieval.resources.clone() {
            let registration = SourceRegistration::from_plugin(&plugin, source);
            let key = registration.key();
            match self.entries.get(&key).cloned() {
                Some(RegistryEntry::Builtin(builtin)) if registration.replaces_builtin => {
                    self.entries.insert(
                        key,
                        RegistryEntry::BuiltinWithReplacement {
                            builtin,
                            replacement: registration,
                        },
                    );
                }
                Some(_) => self.errors.push(format!(
                    "plugin retrieval resource `{}` from `{}` conflicts with an existing resource; set replacesBuiltin=true only for intentional replacements",
                    key.display(), plugin.plugin_id
                )),
                None => {
                    self.entries.insert(key, RegistryEntry::Plugin(registration));
                }
            }
        }
    }
}

impl RegistryEntry {
    fn select(&self, keys: &WebSearchApiKeys) -> SourceRegistration {
        match self {
            Self::Builtin(source) | Self::Plugin(source) => source.clone(),
            Self::BuiltinWithReplacement {
                builtin,
                replacement,
            } => {
                if is_plugin_source_explicitly_enabled(keys, replacement) {
                    replacement.clone()
                } else {
                    builtin.clone()
                }
            }
        }
    }
}

impl SourceRegistration {
    fn from_builtin(source: retrieval_registry::RetrievalSourceDefinition) -> Self {
        Self {
            category: source.category.to_string(),
            id: source.id.to_string(),
            label: source.label.to_string(),
            capabilities: source
                .capabilities
                .iter()
                .map(|capability| match capability {
                    RetrievalCapability::Search => "search".to_string(),
                    RetrievalCapability::Fetch => "fetch".to_string(),
                    RetrievalCapability::Query => "query".to_string(),
                })
                .collect(),
            provider: RetrievalProviderKind::Builtin,
            plugin_id: None,
            plugin_root: None,
            replaces_builtin: false,
            default_enabled: source.default_enabled,
            required_credential_refs: source
                .required_credential_refs
                .iter()
                .map(|v| (*v).to_string())
                .collect(),
            optional_credential_refs: source
                .optional_credential_refs
                .iter()
                .map(|v| (*v).to_string())
                .collect(),
        }
    }

    fn from_plugin(
        plugin: &PluginRetrievalRegistration,
        source: crate::domain::plugin_runtime::retrieval::manifest::PluginRetrievalResource,
    ) -> Self {
        Self {
            category: source.category,
            id: source.id,
            label: source.label,
            capabilities: source.capabilities,
            provider: RetrievalProviderKind::Plugin,
            plugin_id: Some(plugin.plugin_id.clone()),
            plugin_root: Some(plugin.plugin_root.clone()),
            replaces_builtin: source.replaces_builtin,
            default_enabled: source.default_enabled,
            required_credential_refs: source.required_credential_refs,
            optional_credential_refs: source.optional_credential_refs,
        }
    }

    pub fn key(&self) -> SourceKey {
        SourceKey {
            category: self.category.clone(),
            id: self.id.clone(),
        }
    }

    pub fn supports_operation(&self, operation: RetrievalOperation) -> bool {
        let required = match operation {
            RetrievalOperation::Search => "search",
            RetrievalOperation::Fetch | RetrievalOperation::Resolve => "fetch",
            RetrievalOperation::Query | RetrievalOperation::DownloadSummary => "query",
        };
        self.capabilities
            .iter()
            .any(|capability| capability == required)
    }
}

impl SourceKey {
    fn display(&self) -> String {
        format!("{}.{}", self.category, self.id)
    }
}

fn is_enabled(keys: &WebSearchApiKeys, source: &SourceRegistration) -> bool {
    match source.provider {
        RetrievalProviderKind::Builtin => keys
            .enabled_sources_for_category(&source.category)
            .iter()
            .any(|id| id == &source.id),
        RetrievalProviderKind::Plugin => is_plugin_source_explicitly_enabled(keys, source),
    }
}

fn is_plugin_source_explicitly_enabled(
    keys: &WebSearchApiKeys,
    source: &SourceRegistration,
) -> bool {
    keys.enabled_sources_by_category
        .as_ref()
        .and_then(|map| map.get(&source.category))
        .map(|values| {
            values
                .iter()
                .map(|value| normalize_id(value))
                .any(|id| id == source.id)
        })
        .unwrap_or(source.default_enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::plugin_runtime::retrieval::manifest::{
        PluginRetrievalManifest, PluginRetrievalResource, PluginRetrievalRuntime,
    };
    use crate::domain::retrieval::types::{RetrievalRequest, RetrievalTool};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn request(category: &str, source: &str, operation: RetrievalOperation) -> RetrievalRequest {
        RetrievalRequest {
            request_id: "req".to_string(),
            tool: RetrievalTool::Search,
            operation,
            category: category.to_string(),
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

    fn plugin_registration(source_id: &str, replaces_builtin: bool) -> PluginRetrievalRegistration {
        PluginRetrievalRegistration {
            plugin_id: "mock@tests".to_string(),
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
                resources: vec![PluginRetrievalResource {
                    id: source_id.to_string(),
                    category: "dataset".to_string(),
                    label: source_id.to_string(),
                    description: String::new(),
                    aliases: vec![],
                    subcategories: vec![],
                    capabilities: vec![
                        "search".to_string(),
                        "fetch".to_string(),
                        "query".to_string(),
                    ],
                    required_credential_refs: vec![],
                    optional_credential_refs: vec![],
                    risk_level: "low".to_string(),
                    risk_notes: vec![],
                    default_enabled: false,
                    replaces_builtin,
                    parameters: vec![],
                }],
            },
        }
    }

    fn keys_with_enabled(category: &str, sources: &[&str]) -> WebSearchApiKeys {
        let mut map = HashMap::new();
        map.insert(
            category.to_string(),
            sources.iter().map(|source| (*source).to_string()).collect(),
        );
        WebSearchApiKeys {
            enabled_sources_by_category: Some(map),
            ..WebSearchApiKeys::default()
        }
    }

    #[test]
    fn plugin_source_requires_explicit_enablement() {
        let registry = RetrievalRouteRegistry::new(vec![plugin_registration("mock_source", false)]);
        assert!(registry.errors().is_empty());

        let disabled = registry.resolve_request(
            &request("dataset", "mock_source", RetrievalOperation::Search),
            &WebSearchApiKeys::default(),
        );
        assert!(matches!(
            disabled,
            Err(RetrievalError::SourceDisabled { .. })
        ));

        let enabled = registry
            .resolve_request(
                &request("dataset", "mock_source", RetrievalOperation::Search),
                &keys_with_enabled("dataset", &["mock_source"]),
            )
            .unwrap();
        assert_eq!(enabled.provider, RetrievalProviderKind::Plugin);
        assert_eq!(enabled.plugin_id.as_deref(), Some("mock@tests"));
    }

    #[test]
    fn plugin_cannot_collide_with_builtin_without_replaces_builtin() {
        let registry = RetrievalRouteRegistry::new(vec![plugin_registration("geo", false)]);

        assert_eq!(registry.errors().len(), 1);
        let selected = registry
            .resolve_request(
                &request("dataset", "geo", RetrievalOperation::Search),
                &WebSearchApiKeys::default(),
            )
            .unwrap();
        assert_eq!(selected.provider, RetrievalProviderKind::Builtin);
    }

    #[test]
    fn replacement_plugin_only_wins_when_explicitly_enabled() {
        let registry = RetrievalRouteRegistry::new(vec![plugin_registration("geo", true)]);
        assert!(registry.errors().is_empty());

        let builtin = registry
            .resolve_request(
                &request("dataset", "geo", RetrievalOperation::Search),
                &WebSearchApiKeys::default(),
            )
            .unwrap();
        assert_eq!(builtin.provider, RetrievalProviderKind::Builtin);

        let plugin = registry
            .resolve_request(
                &request("dataset", "geo", RetrievalOperation::Search),
                &keys_with_enabled("dataset", &["geo"]),
            )
            .unwrap();
        assert_eq!(plugin.provider, RetrievalProviderKind::Plugin);
    }

    #[test]
    fn unsupported_operation_is_rejected() {
        let mut plugin = plugin_registration("mock_source", false);
        plugin.retrieval.resources[0].capabilities = vec!["search".to_string()];
        let registry = RetrievalRouteRegistry::new(vec![plugin]);
        let err = registry
            .resolve_request(
                &request("dataset", "mock_source", RetrievalOperation::Fetch),
                &keys_with_enabled("dataset", &["mock_source"]),
            )
            .unwrap_err();

        assert!(matches!(err, RetrievalError::InvalidRequest { .. }));
    }
}
