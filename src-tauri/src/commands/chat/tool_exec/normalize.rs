use super::*;

fn normalize_legacy_web_tool_name(tool_name: &str) -> String {
    normalize_legacy_retrieval_tool_name(tool_name)
}

fn normalize_legacy_web_tool_arguments(
    original_tool_name: &str,
    normalized_tool_name: &str,
    arguments: &str,
) -> String {
    normalize_legacy_retrieval_tool_arguments(original_tool_name, normalized_tool_name, arguments)
}

pub(super) fn normalize_runtime_tool_call(tool_name: &str, arguments: &str) -> (String, String) {
    let normalized_name = normalize_legacy_web_tool_name(tool_name);
    let normalized_arguments =
        normalize_legacy_web_tool_arguments(tool_name, &normalized_name, arguments);
    (normalized_name, normalized_arguments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_normalizes_legacy_pubmed_mcp_to_unified_search() {
        let (name, args) = normalize_runtime_tool_call(
            "mcp__pubmed__pubmed_search_articles",
            r#"{"term":"lung cancer","retmax":4}"#,
        );

        assert_eq!(name, "search");
        let value: serde_json::Value = serde_json::from_str(&args).unwrap();
        assert_eq!(value["category"], "literature");
        assert_eq!(value["source"], "pubmed");
        assert_eq!(value["query"], "lung cancer");
        assert_eq!(value["max_results"], 4);
    }
}
