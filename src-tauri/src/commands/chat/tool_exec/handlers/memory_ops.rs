use super::super::dispatch::ToolDispatchContext;
use crate::domain::permissions::canonical_permission_tool_name;

pub(super) fn working_memory_query_text(
    tool_name: &str,
    arguments: &str,
    skill_task_context: Option<&str>,
) -> Option<String> {
    let trimmed = arguments.trim();
    let canonical = canonical_permission_tool_name(tool_name).to_ascii_lowercase();

    let preferred_keys: &[&str] = match canonical.as_str() {
        "recall" | "search" => &["query"],
        "query" => &["query", "id", "url", "accession"],
        "fetch" | "read_mcp_resource" => &["url", "uri"],
        _ => &[
            "query", "prompt", "message", "url", "uri", "path", "title", "text",
        ],
    };

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(found) = extract_first_string_field(&value, preferred_keys) {
            return Some(found);
        }
    } else if !trimmed.is_empty() && !trimmed.starts_with('{') && !trimmed.starts_with('[') {
        return Some(trimmed.to_string());
    }

    skill_task_context
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn extract_first_string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    for key in keys {
        let found = object
            .get(*key)
            .and_then(|candidate| candidate.as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty());
        if let Some(found) = found {
            return Some(found.to_string());
        }
    }
    None
}

pub(super) fn is_memory_tool(tool_name: &str) -> bool {
    matches!(
        canonical_permission_tool_name(tool_name).as_str(),
        "recall"
            | "memory"
            | "memory_read"
            | "memory_write"
            | "working_memory"
            | "working_memory_read"
            | "working_memory_write"
    )
}

pub(super) async fn handle_memory_tool(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
    super::execute_domain_tool(ctx).await
}

#[cfg(test)]
mod tests {
    use super::working_memory_query_text;

    #[test]
    fn working_memory_query_prefers_recall_query_field() {
        let query = working_memory_query_text(
            "recall",
            r#"{"query":"氧化还原节律","scope":"all","limit":5}"#,
            Some("fallback task"),
        );

        assert_eq!(query.as_deref(), Some("氧化还原节律"));
    }

    #[test]
    fn working_memory_query_reads_query_tool_identifiers() {
        let query = working_memory_query_text(
            "query",
            r#"{"category":"dataset","operation":"fetch","id":"GSE12345"}"#,
            None,
        );

        assert_eq!(query.as_deref(), Some("GSE12345"));
    }

    #[test]
    fn working_memory_query_falls_back_to_task_context_when_needed() {
        let query =
            working_memory_query_text("todo_write", r#"{"todos":[]}"#, Some("整理记忆分层"));

        assert_eq!(query.as_deref(), Some("整理记忆分层"));
    }
}
