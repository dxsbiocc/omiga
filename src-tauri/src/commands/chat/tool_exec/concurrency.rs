use super::*;

fn concurrency_safe_tool_names_from_schemas<'a>(
    schemas: impl IntoIterator<Item = &'a ToolSchema>,
) -> HashSet<String> {
    let mut names = HashSet::new();
    for schema in schemas {
        if schema.concurrency_safe {
            names.insert(schema.name.clone());
            names.insert(canonical_permission_tool_name(&schema.name));
        }
    }
    names
}

fn tool_name_declared_concurrency_safe(
    tool_name: &str,
    concurrency_safe_tool_names: &HashSet<String>,
) -> bool {
    concurrency_safe_tool_names.contains(tool_name)
        || concurrency_safe_tool_names.contains(&canonical_permission_tool_name(tool_name))
}

pub(super) fn partition_tool_call_indices_by_concurrency(
    indices: impl IntoIterator<Item = usize>,
    tool_calls: &[(String, String, String)],
    concurrency_safe_tool_names: &HashSet<String>,
) -> (Vec<usize>, Vec<usize>) {
    let mut parallel_indices = Vec::new();
    let mut sequential_indices = Vec::new();

    for idx in indices {
        let (_, tool_name, _) = &tool_calls[idx];
        if tool_name_declared_concurrency_safe(tool_name, concurrency_safe_tool_names) {
            parallel_indices.push(idx);
        } else {
            sequential_indices.push(idx);
        }
    }

    (parallel_indices, sequential_indices)
}

async fn current_mcp_tool_schemas(app: &AppHandle, project_root: &Path) -> Vec<ToolSchema> {
    let app_state = app.state::<OmigaAppState>();
    let config_signature = crate::domain::mcp::merged_mcp_servers_signature(project_root);
    if let Some(schemas) = {
        let cache = app_state.chat.mcp_tool_cache.lock().await;
        cache
            .get(project_root)
            .filter(|cached| {
                cached.cached_at.elapsed() < MCP_TOOL_CACHE_TTL
                    && cached.config_signature == config_signature
            })
            .map(|cached| cached.schemas.clone())
    } {
        return schemas;
    }

    let mcp_timeout = std::time::Duration::from_secs(10);
    let schemas =
        crate::domain::mcp::tool_pool::discover_mcp_tool_schemas(project_root, mcp_timeout).await;
    app_state.chat.mcp_tool_cache.lock().await.insert(
        project_root.to_path_buf(),
        McpToolCache {
            schemas: schemas.clone(),
            cached_at: std::time::Instant::now(),
            config_signature,
        },
    );
    schemas
}

pub(super) async fn load_concurrency_safe_tool_names(
    app: &AppHandle,
    project_root: &Path,
    include_mcp_tools: bool,
) -> HashSet<String> {
    let mut schemas = all_tool_schemas(true);
    schemas.extend(crate::domain::operators::enabled_operator_tool_schemas());
    if include_mcp_tools {
        schemas.extend(current_mcp_tool_schemas(app, project_root).await);
    }
    concurrency_safe_tool_names_from_schemas(&schemas)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tool_call(name: &str) -> (String, String, String) {
        (format!("call-{name}"), name.to_string(), "{}".to_string())
    }

    #[test]
    fn declared_concurrency_safe_tool_lands_in_parallel_batch() {
        let schemas = vec![
            ToolSchema::new("file_read", "Read", serde_json::json!({})).concurrency_safe(),
            ToolSchema::new("file_write", "Write", serde_json::json!({})),
        ];
        let safe_names = concurrency_safe_tool_names_from_schemas(&schemas);
        let tool_calls = vec![test_tool_call("file_read"), test_tool_call("file_write")];

        let (parallel, sequential) = partition_tool_call_indices_by_concurrency(
            0..tool_calls.len(),
            &tool_calls,
            &safe_names,
        );

        assert_eq!(parallel, vec![0]);
        assert_eq!(sequential, vec![1]);
    }

    #[test]
    fn tool_without_concurrency_safe_declaration_runs_serially() {
        let schemas = vec![ToolSchema::new("file_read", "Read", serde_json::json!({}))];
        let safe_names = concurrency_safe_tool_names_from_schemas(&schemas);
        let tool_calls = vec![test_tool_call("file_read")];

        let (parallel, sequential) = partition_tool_call_indices_by_concurrency(
            0..tool_calls.len(),
            &tool_calls,
            &safe_names,
        );

        assert!(parallel.is_empty());
        assert_eq!(sequential, vec![0]);
    }

    #[test]
    fn legacy_alias_uses_canonical_concurrency_safe_schema() {
        let schemas =
            vec![ToolSchema::new("file_read", "Read", serde_json::json!({})).concurrency_safe()];
        let safe_names = concurrency_safe_tool_names_from_schemas(&schemas);
        let tool_calls = vec![test_tool_call("Read")];

        let (parallel, sequential) = partition_tool_call_indices_by_concurrency(
            0..tool_calls.len(),
            &tool_calls,
            &safe_names,
        );

        assert_eq!(parallel, vec![0]);
        assert!(sequential.is_empty());
    }
}
