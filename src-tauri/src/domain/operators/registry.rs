use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use crate::domain::tools::ToolSchema;
use serde_json::Value as JsonValue;

use super::{
    chains, discover_manifest_paths, load_operator_manifest, operator_operation_names,
    validate_operator_id, OperatorCandidateSummary, OperatorOperationSummary,
    OperatorRegistryEntry, OperatorRegistryFile, OperatorRegistryUpdate, OperatorSpec,
    OperatorToolError, ResolvedOperator, OPERATOR_TOOL_PREFIX,
};

pub mod operator_favorites {
    use super::super::{current_epoch_ms, OPERATOR_STATE_DIR_NAME};
    use std::fs;
    use std::path::{Path, PathBuf};

    const FAVORITES_FILE_NAME: &str = "operator-favorites.json";

    pub fn favorites_path() -> PathBuf {
        let path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(OPERATOR_STATE_DIR_NAME)
            .join(FAVORITES_FILE_NAME);
        if let Some(parent) = path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                tracing::warn!("create operator favorites dir {:?}: {}", parent, err);
            }
        }
        path
    }

    pub fn list_favorites() -> Vec<String> {
        list_favorites_at_path(&favorites_path())
    }

    pub fn set_favorite(alias: &str, pinned: bool) -> Result<Vec<String>, String> {
        set_favorite_at_path(&favorites_path(), alias, pinned)
    }

    fn list_favorites_at_path(path: &Path) -> Vec<String> {
        read_favorites(path).unwrap_or_default()
    }

    fn set_favorite_at_path(path: &Path, alias: &str, pinned: bool) -> Result<Vec<String>, String> {
        let alias = alias.trim();
        if alias.is_empty() {
            return Err("operator alias cannot be empty".to_string());
        }

        let mut favorites = read_favorites(path).unwrap_or_default();
        if pinned {
            favorites.push(alias.to_string());
        } else {
            favorites.retain(|value| value != alias);
        }
        favorites = normalize_favorites(favorites);
        write_favorites(path, &favorites)?;
        Ok(favorites)
    }

    fn read_favorites(path: &Path) -> Result<Vec<String>, String> {
        let raw = fs::read_to_string(path)
            .map_err(|err| format!("read operator favorites {}: {err}", path.display()))?;
        let parsed = serde_json::from_str::<Vec<String>>(&raw)
            .map_err(|err| format!("parse operator favorites {}: {err}", path.display()))?;
        Ok(normalize_favorites(parsed))
    }

    fn normalize_favorites(values: Vec<String>) -> Vec<String> {
        let mut aliases: Vec<String> = values
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        aliases.sort();
        aliases.dedup();
        aliases
    }

    fn write_favorites(path: &Path, favorites: &[String]) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("create operator favorites dir: {err}"))?;
        }
        let raw = serde_json::to_string_pretty(favorites).map_err(|err| err.to_string())?;
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(FAVORITES_FILE_NAME);
        let tmp_path = path.with_file_name(format!(
            ".{file_name}.tmp-{}-{}",
            std::process::id(),
            current_epoch_ms()
        ));
        fs::write(&tmp_path, format!("{raw}\n"))
            .map_err(|err| format!("write operator favorites temp file: {err}"))?;
        fs::rename(&tmp_path, path).map_err(|err| {
            let _ = fs::remove_file(&tmp_path);
            format!("replace operator favorites file: {err}")
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use tempfile::TempDir;

        #[test]
        fn list_favorites_returns_empty_for_missing_or_invalid_file() {
            let tmp = TempDir::new().expect("tempdir");
            let path = tmp.path().join(".omiga/operator-favorites.json");

            assert!(list_favorites_at_path(&path).is_empty());

            fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
            fs::write(&path, "{not json").expect("write invalid json");

            assert!(list_favorites_at_path(&path).is_empty());
        }

        #[test]
        fn set_favorite_adds_removes_dedupes_and_persists_sorted_aliases() {
            let tmp = TempDir::new().expect("tempdir");
            let path = tmp.path().join(".omiga/operator-favorites.json");

            assert_eq!(
                set_favorite_at_path(&path, "zeta", true).expect("pin zeta"),
                vec!["zeta".to_string()]
            );
            assert_eq!(
                set_favorite_at_path(&path, "alpha", true).expect("pin alpha"),
                vec!["alpha".to_string(), "zeta".to_string()]
            );
            assert_eq!(
                set_favorite_at_path(&path, " zeta ", true).expect("dedupe zeta"),
                vec!["alpha".to_string(), "zeta".to_string()]
            );
            assert_eq!(
                set_favorite_at_path(&path, "alpha", false).expect("unpin alpha"),
                vec!["zeta".to_string()]
            );
            assert_eq!(list_favorites_at_path(&path), vec!["zeta".to_string()]);
        }
    }
}

const REGISTRY_RELATIVE_PATH: &str = "operators/registry.json";

pub fn registry_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(super::OPERATOR_STATE_DIR_NAME)
        .join(REGISTRY_RELATIVE_PATH)
}

pub fn load_registry_file() -> OperatorRegistryFile {
    fs::read_to_string(registry_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<OperatorRegistryFile>(&raw).ok())
        .unwrap_or_default()
}

pub(crate) fn write_registry_file(registry: &OperatorRegistryFile) -> Result<(), String> {
    let path = registry_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create operator registry dir: {err}"))?;
    }
    let raw = serde_json::to_string_pretty(registry).map_err(|err| err.to_string())?;
    fs::write(&path, format!("{raw}\n")).map_err(|err| format!("write operator registry: {err}"))
}

pub(crate) fn discover_operator_candidates_from_plugins<'a>(
    plugins: impl IntoIterator<Item = &'a crate::domain::plugins::LoadedPlugin>,
) -> Vec<OperatorSpec> {
    let mut out = Vec::new();
    for plugin in plugins.into_iter().filter(|plugin| plugin.is_active()) {
        for manifest_path in discover_manifest_paths(&plugin.root) {
            match load_operator_manifest(&manifest_path, plugin.id.clone(), plugin.root.clone()) {
                Ok(spec) => out.push(spec),
                Err(err) => tracing::warn!(
                    plugin_id = %plugin.id,
                    manifest = %manifest_path.display(),
                    "ignoring invalid operator manifest: {err}"
                ),
            }
        }
    }
    out.sort_by(|left, right| {
        left.metadata
            .id
            .cmp(&right.metadata.id)
            .then_with(|| left.metadata.version.cmp(&right.metadata.version))
            .then_with(|| left.source.source_plugin.cmp(&right.source.source_plugin))
    });
    out
}

pub fn discover_operator_candidates() -> Vec<OperatorSpec> {
    let outcome = crate::domain::plugins::plugin_load_outcome();
    let mut candidates = discover_operator_candidates_from_plugins(outcome.plugins());
    candidates.extend(chains::discover_user_operator_candidates());
    candidates
}

pub fn resolve_enabled_operators() -> Vec<ResolvedOperator> {
    resolve_enabled_operators_from(discover_operator_candidates(), load_registry_file())
}

pub fn set_operator_enabled(update: OperatorRegistryUpdate) -> Result<(), String> {
    let mut registry = load_registry_file();
    apply_operator_registry_update(&mut registry, discover_operator_candidates(), update)?;
    write_registry_file(&registry)
}

pub(crate) fn apply_operator_registry_update(
    registry: &mut OperatorRegistryFile,
    candidates: Vec<OperatorSpec>,
    update: OperatorRegistryUpdate,
) -> Result<(), String> {
    validate_operator_id(&update.alias)?;
    let operator_id = update
        .operator_id
        .as_deref()
        .unwrap_or(update.alias.as_str())
        .trim()
        .to_string();
    validate_operator_id(&operator_id)?;

    if !update.enabled {
        registry.enabled.insert(
            update.alias,
            OperatorRegistryEntry::Full {
                operator_id: Some(operator_id),
                source_plugin: update.source_plugin,
                version: update.version,
                enabled: Some(false),
            },
        );
        return Ok(());
    }

    let matches = candidates
        .into_iter()
        .filter(|candidate| candidate.metadata.id == operator_id)
        .filter(|candidate| {
            update
                .version
                .as_deref()
                .map(|version| candidate.metadata.version == version)
                .unwrap_or(true)
        })
        .filter(|candidate| {
            update
                .source_plugin
                .as_deref()
                .map(|plugin| candidate.source.source_plugin == plugin)
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();

    let selected = match matches.as_slice() {
        [only] => only,
        [] => {
            return Err(format!(
                "operator `{operator_id}` could not be resolved from installed enabled plugins"
            ))
        }
        many => {
            return Err(format!(
                "operator `{operator_id}` is ambiguous across {} candidates; specify sourcePlugin and version",
                many.len()
            ))
        }
    };

    registry.enabled.insert(
        update.alias,
        OperatorRegistryEntry::Full {
            operator_id: Some(selected.metadata.id.clone()),
            source_plugin: Some(selected.source.source_plugin.clone()),
            version: Some(selected.metadata.version.clone()),
            enabled: Some(true),
        },
    );
    Ok(())
}

fn registry_entry_matches_candidate(
    alias: &str,
    entry: &OperatorRegistryEntry,
    candidate: &OperatorSpec,
) -> bool {
    candidate.metadata.id == entry.operator_id(alias)
        && entry
            .version()
            .map(|version| candidate.metadata.version == version)
            .unwrap_or(true)
        && entry
            .source_plugin()
            .map(|plugin| candidate.source.source_plugin == plugin)
            .unwrap_or(true)
}

fn operator_candidate_key(candidate: &OperatorSpec) -> (String, String, String, PathBuf) {
    (
        candidate.source.source_plugin.clone(),
        candidate.metadata.id.clone(),
        candidate.metadata.version.clone(),
        candidate.source.manifest_path.clone(),
    )
}

pub(crate) fn resolve_enabled_operators_from(
    candidates: Vec<OperatorSpec>,
    registry: OperatorRegistryFile,
) -> Vec<ResolvedOperator> {
    let mut resolved = Vec::new();
    let mut resolved_aliases = HashSet::new();
    let mut resolved_candidates = HashSet::new();
    let mut candidate_id_counts: HashMap<String, usize> = HashMap::new();

    for candidate in &candidates {
        *candidate_id_counts
            .entry(candidate.metadata.id.clone())
            .or_default() += 1;
    }

    for (alias, entry) in registry.enabled {
        if !entry.enabled() {
            continue;
        }
        if validate_operator_id(&alias).is_err() {
            tracing::warn!(alias = %alias, "ignoring invalid operator registry alias");
            continue;
        }
        let wanted_id = entry.operator_id(&alias);
        let matches = candidates
            .iter()
            .filter(|candidate| registry_entry_matches_candidate(&alias, &entry, candidate))
            .cloned()
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [only] => {
                if resolved_aliases.insert(alias.clone()) {
                    resolved_candidates.insert(operator_candidate_key(only));
                    resolved.push(ResolvedOperator {
                        alias,
                        spec: only.clone(),
                    });
                }
            }
            [] => tracing::warn!(
                alias = %alias,
                operator_id = %wanted_id,
                "enabled operator could not be resolved"
            ),
            many => tracing::warn!(
                alias = %alias,
                operator_id = %wanted_id,
                candidates = many.len(),
                "enabled operator is ambiguous; set sourcePlugin and version"
            ),
        }
    }

    for candidate in candidates {
        let alias = candidate.metadata.id.clone();
        if candidate_id_counts.get(&alias).copied().unwrap_or_default() != 1 {
            tracing::warn!(
                alias = %alias,
                "operator auto-exposure skipped because multiple active plugins define the same operator id"
            );
            continue;
        }
        if resolved_candidates.contains(&operator_candidate_key(&candidate)) {
            continue;
        }
        if resolved_aliases.insert(alias.clone()) {
            resolved_candidates.insert(operator_candidate_key(&candidate));
            resolved.push(ResolvedOperator {
                alias,
                spec: candidate,
            });
        }
    }

    resolved.sort_by(|left, right| left.alias.cmp(&right.alias));
    resolved
}

pub fn resolve_operator_alias(alias: &str) -> Result<ResolvedOperator, OperatorToolError> {
    let alias = alias
        .strip_prefix(OPERATOR_TOOL_PREFIX)
        .unwrap_or(alias)
        .trim();
    for resolved in resolve_enabled_operators() {
        if resolved.alias == alias {
            return Ok(resolved);
        }
    }
    Err(OperatorToolError::new(
        "unknown_operator",
        false,
        format!("Operator `{alias}` is not enabled or could not be resolved."),
    )
    .with_suggested_action("Run operator_list to inspect installed/enabled operators."))
}

pub fn describe_operator(
    id_or_alias: &str,
) -> Result<(Option<String>, OperatorSpec), OperatorToolError> {
    if let Ok(resolved) = resolve_operator_alias(id_or_alias) {
        return Ok((Some(resolved.alias), resolved.spec));
    }
    let id = id_or_alias
        .strip_prefix(OPERATOR_TOOL_PREFIX)
        .unwrap_or(id_or_alias)
        .trim();
    let matches = discover_operator_candidates()
        .into_iter()
        .filter(|candidate| candidate.metadata.id == id)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [only] => Ok((None, only.clone())),
        [] => Err(OperatorToolError::new(
            "unknown_operator",
            false,
            format!("Operator `{id}` is not installed or enabled."),
        )
        .with_suggested_action("Run operator_list to inspect installed operators.")),
        many => Err(OperatorToolError::new(
            "operator_version_unresolved",
            false,
            format!(
                "Operator `{id}` has {} installed candidates; enable one alias in the operator registry first.",
                many.len()
            ),
        )
        .with_suggested_action("Resolve the operator source/version conflict in settings or registry.json.")),
    }
}

pub fn list_operator_summaries() -> Vec<OperatorCandidateSummary> {
    let candidates = discover_operator_candidates();
    let enabled = resolve_enabled_operators();
    let mut enabled_by_key: HashMap<(String, String, String), Vec<String>> = HashMap::new();
    for item in enabled {
        enabled_by_key
            .entry((
                item.spec.metadata.id.clone(),
                item.spec.metadata.version.clone(),
                item.spec.source.source_plugin.clone(),
            ))
            .or_default()
            .push(item.alias);
    }
    candidates
        .into_iter()
        .map(|candidate| {
            let aliases = enabled_by_key
                .remove(&(
                    candidate.metadata.id.clone(),
                    candidate.metadata.version.clone(),
                    candidate.source.source_plugin.clone(),
                ))
                .unwrap_or_default();
            operator_candidate_summary(candidate, aliases)
        })
        .collect()
}

pub fn operator_candidate_summary(
    candidate: OperatorSpec,
    enabled_aliases: Vec<String>,
) -> OperatorCandidateSummary {
    let exposed = !enabled_aliases.is_empty();
    let operations = operator_operation_summaries(&candidate, exposed);
    let environment_ref = candidate.runtime.as_ref().and_then(|runtime| {
        runtime
            .get("envRef")
            .or_else(|| runtime.get("env_ref"))
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    });
    OperatorCandidateSummary {
        id: candidate.metadata.id,
        version: candidate.metadata.version,
        name: candidate.metadata.name,
        description: candidate.metadata.description,
        tags: candidate.metadata.tags,
        source_plugin: candidate.source.source_plugin,
        manifest_path: candidate
            .source
            .manifest_path
            .to_string_lossy()
            .into_owned(),
        interface: candidate.interface,
        operations,
        execution: candidate.execution,
        preflight: candidate.preflight,
        runtime: candidate.runtime,
        resources: candidate.resources,
        smoke_tests: candidate.smoke_tests,
        environment_ref,
        exposed,
        enabled_aliases,
        unavailable_reason: None,
    }
}

pub(crate) fn operator_operation_summaries(
    candidate: &OperatorSpec,
    exposed: bool,
) -> Vec<OperatorOperationSummary> {
    candidate
        .operations
        .iter()
        .map(|(id, operation)| OperatorOperationSummary {
            id: id.clone(),
            name: operation.name.clone(),
            description: operation.description.clone(),
            category: operation.category.clone(),
            group: operation.group.clone(),
            stage: operation.stage.clone(),
            tags: operation.tags.clone(),
            interface: operation.interface.clone(),
            runtime: operation.runtime.clone(),
            resources: operation.resources.clone(),
            exposed,
        })
        .collect()
}

pub fn enabled_operator_tool_schemas() -> Vec<ToolSchema> {
    Vec::new()
}

pub(crate) fn format_enabled_operator_tools_system_section_from_resolved(
    resolved: Vec<ResolvedOperator>,
) -> Option<String> {
    if resolved.is_empty() {
        return None;
    }

    let program_count = resolved.len();
    let operation_count = resolved
        .iter()
        .map(|operator| operator_operation_names(&operator.spec).len().max(1))
        .sum::<usize>();
    let mut lines = vec![
        "## Plugin operator execution".to_string(),
        format!(
            "Active plugins register {program_count} Operator programs with {operation_count} operations. Use `unit_search` / `unit_describe` or `operator_describe` to narrow candidates, then call `operator_execute` with `operator` and `operation`. Subcommands are operations, not separate tools; do not ask the user to manually register operator tools."
        ),
        String::new(),
    ];
    lines.push(
        "Default routing flow: search/describe first, execute second. Legacy dynamic aliases remain available only for older callers and should not be used for new routing.".to_string(),
    );

    Some(lines.join("\n"))
}

pub fn format_enabled_operator_tools_system_section() -> Option<String> {
    format_enabled_operator_tools_system_section_from_resolved(resolve_enabled_operators())
}
